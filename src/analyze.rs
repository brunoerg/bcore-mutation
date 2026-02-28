use crate::db::Database;
use crate::error::{MutationError, Result};
use crate::report::generate_report;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;
use walkdir::WalkDir;

pub async fn run_analysis(
    folder: Option<PathBuf>,
    command: Option<String>,
    jobs: u32,
    timeout_secs: u64,
    survival_threshold: f64,
    sqlite_path: Option<PathBuf>,
    run_id: Option<i64>,
) -> Result<()> {
    // DB-based analysis mode: read mutants from DB and test them.
    if let (Some(ref path), Some(rid)) = (sqlite_path.as_ref(), run_id) {
        let command = command.ok_or_else(|| {
            MutationError::InvalidInput(
                "--command is required when using --sqlite with --run_id".to_string(),
            )
        })?;
        let db = Database::open(path)?;
        db.ensure_schema()?;
        db.seed_projects()?;
        return run_db_analysis(&db, rid, &command, timeout_secs).await;
    }

    // Folder-based analysis mode (existing behaviour).
    let folders = if let Some(folder_path) = folder {
        vec![folder_path]
    } else {
        // Find all folders starting with "muts"
        find_mutation_folders()?
    };

    for folder_path in folders {
        analyze_folder(
            &folder_path,
            command.clone(),
            jobs,
            timeout_secs,
            survival_threshold,
        )
        .await?;
    }

    Ok(())
}

/// Test all pending mutants in `run_id` from the database.
async fn run_db_analysis(
    db: &Database,
    run_id: i64,
    command: &str,
    timeout_secs: u64,
) -> Result<()> {
    let mutants = db.get_mutants_for_run(run_id)?;
    let total = mutants.len();

    println!("* {} MUTANTS in run_id={} *", total, run_id);

    if total == 0 {
        return Err(MutationError::InvalidInput(format!(
            "No mutants found for run_id={}",
            run_id
        )));
    }

    let mut num_killed: u64 = 0;
    let mut num_survived: u64 = 0;

    for (i, mutant) in mutants.iter().enumerate() {
        println!("[{}/{}] Analyzing mutant id={}", i + 1, total, mutant.id);

        // Determine the file path to restore later.
        let file_path = mutant.file_path.as_deref().unwrap_or("");

        // Ensure the file is at HEAD before applying the mutant diff.
        // A previous mutant may have been left applied if restore silently failed.
        if !file_path.is_empty() {
            if let Err(e) = restore_file(file_path).await {
                eprintln!("  Warning: pre-restore failed for {}: {}", file_path, e);
            }
        }

        // Update status to 'running' and record the command.
        db.update_mutant_status(mutant.id, "running", command)?;

        // Write the patch to a temp file and apply it with `git apply`.
        let apply_result = apply_diff(&mutant.diff).await;
        if let Err(ref e) = apply_result {
            eprintln!("  Failed to apply diff for mutant {}: {}", mutant.id, e);
            db.update_mutant_status(mutant.id, "error", command)?;
            continue;
        }

        // Run the test command.
        let killed = !run_command(command, timeout_secs).await?;

        let new_status = if killed {
            println!("  KILLED ✅");
            num_killed += 1;
            "killed"
        } else {
            println!("  NOT KILLED ❌");
            num_survived += 1;
            "survived"
        };

        db.update_mutant_status(mutant.id, new_status, command)?;

        // Restore the modified file.
        if !file_path.is_empty() {
            restore_file(file_path).await?;
        }
    }

    let score = if total > 0 {
        num_killed as f64 / total as f64
    } else {
        0.0
    };
    println!(
        "\nMUTATION SCORE: {:.2}% ({} killed / {} total)",
        score * 100.0,
        num_killed,
        total
    );
    println!("Survived: {}", num_survived);

    Ok(())
}

/// Apply a unified diff patch using `git apply`.
async fn apply_diff(diff: &str) -> Result<()> {
    use std::io::Write;

    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(diff.as_bytes())?;
    tmp.flush()?;

    let tmp_path = tmp.path().to_path_buf();
    // Keep `tmp` alive until after the command runs.
    let output = TokioCommand::new("git")
        .args(["apply", "--whitespace=nowarn", tmp_path.to_str().unwrap()])
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("git apply failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MutationError::Git(format!(
            "git apply error: {}",
            stderr.trim()
        )));
    }

    Ok(())
}

fn find_mutation_folders() -> Result<Vec<PathBuf>> {
    let mut folders = Vec::new();

    for entry in WalkDir::new(".").max_depth(1) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("muts") {
                    folders.push(entry.path().to_path_buf());
                }
            }
        }
    }

    Ok(folders)
}

pub async fn analyze_folder(
    folder_path: &Path,
    command: Option<String>,
    jobs: u32,
    timeout_secs: u64,
    survival_threshold: f64,
) -> Result<()> {
    let mut num_killed: u64 = 0;
    let mut not_killed = Vec::new();

    // Read target file path
    let original_file_path = folder_path.join("original_file.txt");
    let target_file_path = fs::read_to_string(original_file_path)?;
    let target_file_path = target_file_path.trim();

    // Setup command if not provided
    let test_command = if let Some(cmd) = command {
        cmd
    } else {
        run_build_command().await?;
        get_command_to_kill(&target_file_path, jobs)?
    };

    // Get list of mutant files
    let mut mutant_files = Vec::new();
    for entry in fs::read_dir(folder_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && !path.extension().map_or(true, |ext| ext == "txt") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                mutant_files.push(name.to_string());
            }
        }
    }

    let total_mutants = mutant_files.len();
    println!("* {} MUTANTS *", total_mutants);

    if total_mutants == 0 {
        return Err(MutationError::InvalidInput(format!(
            "No mutants in the provided folder path ({})",
            folder_path.display()
        )));
    }

    for (i, file_name) in mutant_files.iter().enumerate() {
        let current_survival_rate = not_killed.len() as f64 / total_mutants as f64;
        if current_survival_rate > survival_threshold {
            println!(
                "\nTerminating early: {:.2}% mutants surviving after {} iterations",
                current_survival_rate * 100.0,
                i + 1
            );
            println!(
                "Survival rate exceeds threshold of {:.0}%",
                survival_threshold * 100.0
            );
            break;
        }

        println!("[{}/{}] Analyzing {}", i + 1, total_mutants, file_name);

        let file_path = folder_path.join(file_name);

        // Read and apply mutant
        let mutant_content = fs::read_to_string(&file_path)?;
        fs::write(&target_file_path, &mutant_content)?;

        //println!("Running: {}", test_command);
        let result = run_command(&test_command, timeout_secs).await?;

        if result {
            println!("NOT KILLED ❌");
            not_killed.push(file_name.clone());
        } else {
            println!("KILLED ✅");
            num_killed += 1
        }
    }

    // Generate report
    let score = num_killed as f64 / total_mutants as f64;
    println!("\nMUTATION SCORE: {:.2}%", score * 100.0);

    generate_report(
        &not_killed,
        folder_path.to_str().unwrap(),
        &target_file_path,
        score,
    )
    .await?;

    // Restore the original file
    restore_file(&target_file_path).await?;

    Ok(())
}

async fn run_command(command: &str, timeout_secs: u64) -> Result<bool> {
    use std::process::Stdio;

    // Split command into shell and arguments for better cross-platform support
    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    println!("Executing command: {}", command);

    let mut cmd = TokioCommand::new(shell);
    cmd.arg(shell_arg)
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true); // Ensure child process is killed if parent dies

    let timeout_duration = Duration::from_secs(timeout_secs);

    match timeout(timeout_duration, cmd.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            println!("Command exit code: {}", output.status.code().unwrap_or(-1));

            if !stdout.is_empty() {
                println!("STDOUT:\n{}", stdout);
            }

            if !stderr.is_empty() {
                println!("STDERR:\n{}", stderr);
            }

            Ok(output.status.success())
        }
        Ok(Err(e)) => {
            println!("Command execution failed: {}", e);
            Ok(false)
        }
        Err(_) => {
            println!("Command timed out after {} seconds", timeout_secs);
            Ok(false)
        }
    }
}

async fn run_build_command() -> Result<()> {
    let build_command =
        "rm -rf build && cmake -B build -DENABLE_IPC=OFF && cmake --build build -j $(nproc)";

    let success = run_command(build_command, 3600).await?; // 1 hour timeout for build
    if !success {
        return Err(MutationError::Command("Build command failed".to_string()));
    }

    Ok(())
}

fn get_command_to_kill(target_file_path: &str, jobs: u32) -> Result<String> {
    let mut build_command = "cmake --build build".to_string();
    if jobs > 0 {
        build_command.push_str(&format!(" -j{}", jobs));
    }

    let command = if target_file_path.contains("functional") {
        format!("./build/{}", target_file_path)
    } else if target_file_path.contains("test") {
        let filename_with_extension = Path::new(target_file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| MutationError::InvalidInput("Invalid file path".to_string()))?;

        let test_to_run = filename_with_extension
            .rsplit('.')
            .nth(1)
            .ok_or_else(|| MutationError::InvalidInput("Cannot extract test name".to_string()))?;

        format!(
            "{} && ./build/bin/test_bitcoin --run_test={}",
            build_command, test_to_run
        )
    } else {
        format!(
            "{} && ctest --output-on-failure --stop-on-failure -C Release && CI_FAILFAST_TEST_LEAVE_DANGLING=1 ./build/test/functional/test_runner.py -F",
            build_command
        )
    };

    Ok(command)
}

async fn restore_file(target_file_path: &str) -> Result<()> {
    let restore_command = format!("git restore {}", target_file_path);
    let success = run_command(&restore_command, 30).await?;
    if !success {
        return Err(MutationError::Git(format!(
            "git restore failed for {}",
            target_file_path
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_get_command_to_kill() {
        // Test functional test
        let cmd = get_command_to_kill("test/functional/test_example.py", 4).unwrap();
        assert_eq!(cmd, "./build/test/functional/test_example.py");

        // Test unit test
        let cmd = get_command_to_kill("src/test/test_example.cpp", 0).unwrap();
        assert_eq!(
            cmd,
            "cmake --build build && ./build/bin/test_bitcoin --run_test=test_example"
        );

        // Test general case
        let cmd = get_command_to_kill("src/wallet/wallet.cpp", 2).unwrap();
        assert!(cmd.contains("cmake --build build -j2"));
        assert!(cmd.contains("ctest"));
        assert!(cmd.contains("test_runner.py"));
    }

    #[tokio::test]
    async fn test_run_command() {
        // Test successful command
        let result = run_command("echo 'test'", 5).await.unwrap();
        assert!(result);

        // Test failing command
        let result = run_command("false", 5).await.unwrap();
        assert!(!result);

        // Test command that should timeout (note: this might be flaky in CI)
        let result = run_command("sleep 10", 1).await.unwrap();
        assert!(!result);
    }

    #[test]
    fn test_find_mutation_folders() {
        let temp_dir = tempdir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Create some test directories
        fs::create_dir("muts-test-1").unwrap();
        fs::create_dir("muts-test-2").unwrap();
        fs::create_dir("not-muts").unwrap();
        fs::create_dir("another-dir").unwrap();

        let folders = find_mutation_folders().unwrap();
        assert_eq!(folders.len(), 2);

        let folder_names: Vec<String> = folders
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .map(|s| s.to_string())
            .collect();

        assert!(folder_names.contains(&"muts-test-1".to_string()));
        assert!(folder_names.contains(&"muts-test-2".to_string()));
        assert!(!folder_names.contains(&"not-muts".to_string()));
    }
}
