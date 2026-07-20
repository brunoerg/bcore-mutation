use crate::commands::{self, ProjectCommands};
use crate::db::Database;
use crate::error::{MutationError, Result};
use crate::project::Project;
use crate::report::generate_report;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;
use walkdir::WalkDir;

/// Killed/total counts produced by an analysis pass. Used to compute the
/// mutation score and apply the optional `--min-score` CI gate.
#[derive(Default, Clone, Copy)]
pub struct ScoreSummary {
    pub killed: u64,
    pub total: u64,
}

impl ScoreSummary {
    /// Mutation score as a fraction in `[0.0, 1.0]` (killed / total).
    /// Returns `0.0` when no mutants were analyzed.
    pub fn score(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.killed as f64 / self.total as f64
        }
    }

    /// Accumulate another pass's counts (used to aggregate across folders).
    fn add(&mut self, other: ScoreSummary) {
        self.killed += other.killed;
        self.total += other.total;
    }
}

/// Fail (return an error) when `min_score` is set and the achieved mutation
/// score is below it. This is the CI gate: an `Err` here propagates out of
/// `main` and exits the process with a non-zero status.
fn enforce_min_score(summary: ScoreSummary, min_score: Option<f64>) -> Result<()> {
    let Some(min) = min_score else {
        return Ok(());
    };

    let score = summary.score();
    println!(
        "\nOverall mutation score: {:.2}% ({}/{} killed); required minimum: {:.2}%",
        score * 100.0,
        summary.killed,
        summary.total,
        min * 100.0
    );

    if score < min {
        return Err(MutationError::ScoreBelowThreshold(format!(
            "mutation score {:.2}% is below the required minimum of {:.2}%",
            score * 100.0,
            min * 100.0
        )));
    }

    println!("Mutation score meets the required minimum ✅");
    Ok(())
}

pub async fn run_analysis(
    project: Project,
    folder: Option<PathBuf>,
    command: Option<String>,
    jobs: u32,
    timeout_secs: u64,
    survival_threshold: f64,
    min_score: Option<f64>,
    sqlite_path: Option<PathBuf>,
    run_id: Option<i64>,
    file_path: Option<String>,
    survivors_only: bool,
) -> Result<()> {
    println!("Analyzing mutants for project: {}", project.db_name());

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
        let summary = run_db_analysis(
            &db,
            rid,
            &command,
            timeout_secs,
            file_path.as_deref(),
            survivors_only,
        )
        .await?;
        return enforce_min_score(summary, min_score);
    }

    // Folder-based analysis mode (existing behaviour).
    let folders = if let Some(folder_path) = folder {
        vec![folder_path]
    } else {
        // Find all folders starting with "muts"
        find_mutation_folders()?
    };

    let project_commands = commands::for_project(project);

    // When we derive the test command ourselves (no --command), do the one-time
    // clean build up front rather than once per folder. Each mutant still
    // triggers an incremental rebuild inside its test command.
    if command.is_none() && !folders.is_empty() {
        run_build_command(project_commands.as_ref()).await?;
    }

    let mut overall = ScoreSummary::default();
    for folder_path in folders {
        let summary = analyze_folder(
            &folder_path,
            command.clone(),
            jobs,
            timeout_secs,
            survival_threshold,
            project_commands.as_ref(),
        )
        .await?;
        overall.add(summary);
    }

    enforce_min_score(overall, min_score)
}

/// Test all pending mutants in `run_id` from the database, optionally filtered by `file_path`.
/// When `survivors_only` is true, only previously survived mutants are analyzed.
async fn run_db_analysis(
    db: &Database,
    run_id: i64,
    command: &str,
    timeout_secs: u64,
    file_path: Option<&str>,
    survivors_only: bool,
) -> Result<ScoreSummary> {
    let mutants = db.get_mutants_for_run(run_id, file_path, survivors_only)?;
    let total = mutants.len();

    match (file_path, survivors_only) {
        (Some(fp), true) => println!(
            "* {} SURVIVING MUTANTS in run_id={} (file: {}) *",
            total, run_id, fp
        ),
        (Some(fp), false) => println!("* {} MUTANTS in run_id={} (file: {}) *", total, run_id, fp),
        (None, true) => println!("* {} SURVIVING MUTANTS in run_id={} *", total, run_id),
        (None, false) => println!("* {} MUTANTS in run_id={} *", total, run_id),
    }

    if total == 0 {
        return Err(MutationError::InvalidInput(format!(
            "No mutants found for run_id={}",
            run_id
        )));
    }

    // Verify the test command is valid and passes on unmutated code before we
    // analyze a single mutant. Without this, a bad command (e.g. an invalid
    // --run_test filter that exits 200) fails for every mutant and is silently
    // recorded as "killed", yielding a bogus 100% score.
    check_baseline(command, timeout_secs).await?;

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

    Ok(ScoreSummary {
        killed: num_killed,
        total: total as u64,
    })
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
    project_commands: &dyn ProjectCommands,
) -> Result<ScoreSummary> {
    let mut num_killed: u64 = 0;
    let mut not_killed = Vec::new();

    // Read target file path
    let original_file_path = folder_path.join("original_file.txt");
    let target_file_path = fs::read_to_string(original_file_path)?;
    let target_file_path = target_file_path.trim();

    // Derive the test command when one isn't provided. The clean build is done
    // once by the caller (run_analysis); here we only build the per-file test
    // command, whose incremental `cmake --build` picks up each mutant.
    let test_command = if let Some(cmd) = command {
        cmd
    } else {
        project_commands.test_command(&target_file_path, jobs)?
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

    Ok(ScoreSummary {
        killed: num_killed,
        total: total_mutants as u64,
    })
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

/// Run the test command once against **unmutated** code before analyzing any
/// mutants. If it does not pass, abort the whole run with an error.

async fn check_baseline(command: &str, timeout_secs: u64) -> Result<()> {
    println!("Baseline check: running the test command on unmutated code...");
    let passed = run_command(command, timeout_secs).await?;
    if !passed {
        return Err(MutationError::InvalidInput(format!(
            "Test command failed before any mutant was applied. Fix the command and re-run. Command: {}",
            command
        )));
    }
    println!("Baseline check passed: the test command succeeds.\n");
    Ok(())
}

async fn run_build_command(project_commands: &dyn ProjectCommands) -> Result<()> {
    let build_command = project_commands.build_command();

    let success = run_command(&build_command, project_commands.build_timeout_secs()).await?;
    if !success {
        return Err(MutationError::Command("Build command failed".to_string()));
    }

    Ok(())
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
    fn test_enforce_min_score() {
        // No gate configured: always Ok regardless of score.
        assert!(enforce_min_score(
            ScoreSummary {
                killed: 0,
                total: 10
            },
            None
        )
        .is_ok());

        // Above threshold passes.
        assert!(enforce_min_score(
            ScoreSummary {
                killed: 9,
                total: 10
            },
            Some(0.8)
        )
        .is_ok());

        // Exactly at threshold passes (gate uses `<`, so >= passes).
        assert!(enforce_min_score(
            ScoreSummary {
                killed: 8,
                total: 10
            },
            Some(0.8)
        )
        .is_ok());

        // Below threshold fails with the dedicated error variant.
        let result = enforce_min_score(
            ScoreSummary {
                killed: 5,
                total: 10,
            },
            Some(0.8),
        );
        assert!(matches!(result, Err(MutationError::ScoreBelowThreshold(_))));
    }

    #[test]
    fn test_score_summary_aggregates() {
        let mut overall = ScoreSummary::default();
        overall.add(ScoreSummary {
            killed: 3,
            total: 4,
        });
        overall.add(ScoreSummary {
            killed: 1,
            total: 6,
        });
        assert_eq!(overall.killed, 4);
        assert_eq!(overall.total, 10);
        assert!((overall.score() - 0.4).abs() < f64::EPSILON);
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

    #[tokio::test]
    async fn test_check_baseline() {
        // Test a command that passes on unmutated code lets the run proceed.
        assert!(check_baseline("true", 5).await.is_ok());

        // Test a command that fails on unmutated code aborts the run.
        let result= check_baseline("exit 200", 5).await;
        assert!(matches!(result, Err(MutationError::InvalidInput(_))));

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
