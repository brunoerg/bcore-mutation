use bcore_mutation::db::{compute_patch_hash, Database, MutantData};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_verifies_nine_mutants_with_four_workers() {
    let repository = initialized_repository("value.txt");

    let commit = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    let database_path = repository.path().join("mutation.db");
    let run_id = {
        let mut database = Database::open(&database_path).unwrap();
        database.ensure_schema().unwrap();
        database.seed_projects().unwrap();
        let project_id = database.get_project_id("Bitcoin Core").unwrap();
        let run_id = database
            .create_run(project_id, &commit, "test", None, None)
            .unwrap();

        let mutants = (0..9)
            .map(|index| {
                let diff = format!(
                    "diff --git a/value.txt b/value.txt\n\
                     --- a/value.txt\n\
                     +++ b/value.txt\n\
                     @@ -1 +1 @@\n\
                     -base\n\
                     +mutant-{index}\n"
                );
                MutantData {
                    patch_hash: compute_patch_hash(&diff),
                    diff,
                    file_path: "value.txt".to_string(),
                    operator: "integration-test".to_string(),
                }
            })
            .collect::<Vec<_>>();
        database.insert_mutant_batch(run_id, &mutants).unwrap();
        run_id
    };

    let output = Command::new(env!("CARGO_BIN_EXE_bcore-mutation"))
        .current_dir(repository.path())
        .args([
            "analyze",
            "--sqlite",
            database_path.to_str().unwrap(),
            "--run-id",
            &run_id.to_string(),
            "--parallel",
            "4",
            "--command",
            "test \"$(cat value.txt)\" = base",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "parallel CLI failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Parallel analysis: 4 worker(s) for 9 mutant(s)"));
    assert!(stdout.contains("MUTATION SCORE: 100.00% (9 killed / 9 total)"));

    let connection = rusqlite::Connection::open(&database_path).unwrap();
    let killed: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM mutants WHERE run_id = ?1 AND status = 'killed'",
            [run_id],
            |row| row.get(0),
        )
        .unwrap();
    let running: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM mutants WHERE run_id = ?1 AND status = 'running'",
            [run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(killed, 9);
    assert_eq!(running, 0);
    assert_eq!(
        fs::read_to_string(repository.path().join("value.txt")).unwrap(),
        "base\n"
    );

    let worktree_list = git_stdout(repository.path(), &["worktree", "list", "--porcelain"]);
    assert_eq!(
        worktree_list
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
}

#[test]
fn folder_mode_uses_the_same_parallel_worker_pool() {
    let repository = initialized_repository("value.cpp");
    let mutation_folder = repository.path().join("muts-value");
    fs::create_dir(&mutation_folder).unwrap();
    fs::write(mutation_folder.join("original_file.txt"), "value.cpp\n").unwrap();
    for index in 0..7 {
        fs::write(
            mutation_folder.join(format!("mutant-{index}.cpp")),
            format!("mutant-{index}\n"),
        )
        .unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_bcore-mutation"))
        .current_dir(repository.path())
        .args([
            "analyze",
            "--folder",
            mutation_folder.to_str().unwrap(),
            "--parallel",
            "3",
            "--survival-threshold",
            "1.0",
            "--command",
            "test \"$(cat value.cpp)\" = base",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "parallel folder analysis failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Parallel analysis: 3 worker(s) for 7 mutant(s)"));
    assert!(stdout.contains("MUTATION SCORE: 100.00%"));
    assert_eq!(
        fs::read_to_string(repository.path().join("value.cpp")).unwrap(),
        "base\n"
    );

    let worktree_list = git_stdout(repository.path(), &["worktree", "list", "--porcelain"]);
    assert_eq!(
        worktree_list
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
}

#[test]
fn parallel_workers_can_use_referenced_sibling_assets() {
    let parent = tempdir().unwrap();
    let repository_path = parent.path().join("repository");
    fs::create_dir(&repository_path).unwrap();
    initialize_repository_at(&repository_path, "value.txt");

    let assets = parent.path().join("qa-assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("token.txt"), "asset\n").unwrap();

    let commit = git_stdout(&repository_path, &["rev-parse", "HEAD"]);
    let database_path = repository_path.join("mutation.db");
    let run_id = {
        let mut database = Database::open(&database_path).unwrap();
        database.ensure_schema().unwrap();
        database.seed_projects().unwrap();
        let project_id = database.get_project_id("Bitcoin Core").unwrap();
        let run_id = database
            .create_run(project_id, &commit, "test", None, None)
            .unwrap();

        let diff = "diff --git a/value.txt b/value.txt\n\
                    --- a/value.txt\n\
                    +++ b/value.txt\n\
                    @@ -1 +1 @@\n\
                    -base\n\
                    +mutant\n"
            .to_string();
        database
            .insert_mutant_batch(
                run_id,
                &[MutantData {
                    patch_hash: compute_patch_hash(&diff),
                    diff,
                    file_path: "value.txt".to_string(),
                    operator: "integration-test".to_string(),
                }],
            )
            .unwrap();
        run_id
    };

    let output = Command::new(env!("CARGO_BIN_EXE_bcore-mutation"))
        .current_dir(&repository_path)
        .args([
            "analyze",
            "--sqlite",
            database_path.to_str().unwrap(),
            "--run-id",
            &run_id.to_string(),
            "--parallel",
            "2",
            "--command",
            "test \"$(cat ../qa-assets/token.txt)\" = asset && test \"$(cat value.txt)\" = base",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "parallel CLI with sibling assets failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Parallel workers linked ../qa-assets"));
    assert!(stdout.contains("MUTATION SCORE: 100.00% (1 killed / 1 total)"));
}

fn initialized_repository(filename: &str) -> tempfile::TempDir {
    let repository = tempdir().unwrap();
    initialize_repository_at(repository.path(), filename);
    repository
}

fn initialize_repository_at(repository: &Path, filename: &str) {
    run_git(repository, &["init", "-q"]);
    run_git(repository, &["config", "user.email", "test@example.com"]);
    run_git(repository, &["config", "user.name", "Test"]);
    run_git(repository, &["config", "commit.gpgsign", "false"]);
    fs::write(repository.join(filename), "base\n").unwrap();
    run_git(repository, &["add", filename]);
    run_git(repository, &["commit", "-qm", "base"]);
}

fn run_git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
