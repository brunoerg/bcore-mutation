//! Parallel mutant verification using isolated Git worktrees.

use crate::analyze::ScoreSummary;
use crate::commands::ProjectCommands;
use crate::db::Database;
use crate::error::{MutationError, Result};
use crate::report::generate_report;
use crate::workspace::{self, WorkerWorkspace, WorktreePool};
use futures::future::join_all;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[derive(Clone, Debug)]
enum MutantIdentity {
    Database(i64),
    Folder(String),
}

#[derive(Clone, Debug)]
enum MutantPayload {
    Diff(String),
    CompleteFile(String),
}

#[derive(Clone, Debug)]
struct MutantJob {
    sequence: usize,
    identity: MutantIdentity,
    target_file: PathBuf,
    payload: MutantPayload,
    command: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutantStatus {
    Killed,
    Survived,
    Error,
}

impl MutantStatus {
    fn database_value(self) -> &'static str {
        match self {
            Self::Killed => "killed",
            Self::Survived => "survived",
            Self::Error => "error",
        }
    }
}

#[derive(Debug)]
struct CommandExecution {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    spawn_error: Option<String>,
}

#[derive(Debug)]
struct MutantResult {
    worker_id: usize,
    job: MutantJob,
    status: MutantStatus,
    command: Option<CommandExecution>,
    error: Option<String>,
    workspace_usable: bool,
}

struct WorkerMessage {
    result: MutantResult,
}

struct ParallelRun {
    results: Vec<MutantResult>,
    skipped: usize,
}

struct FolderPlan {
    folder_path: PathBuf,
    target_file: PathBuf,
    command: String,
    jobs: Vec<MutantJob>,
}

/// Analyze database-backed mutants with an arbitrary number of isolated workers.
#[allow(clippy::too_many_arguments)]
pub async fn analyze_database(
    db: &Database,
    run_id: i64,
    command: &str,
    setup_command: Option<&str>,
    setup_timeout_secs: u64,
    timeout_secs: u64,
    file_path: Option<&str>,
    survivors_only: bool,
    parallel: usize,
    keep_worktrees: bool,
) -> Result<ScoreSummary> {
    let mutants = db.get_mutants_for_run(run_id, file_path, survivors_only)?;
    let total = mutants.len();

    print_database_header(total, run_id, file_path, survivors_only);
    if total == 0 {
        return Err(MutationError::InvalidInput(format!(
            "No mutants found for run_id={run_id}"
        )));
    }

    let repository_root = workspace::repository_root(Path::new(".")).await?;
    let recorded_commit = db.get_run_commit_hash(run_id)?;
    let base_commit = if recorded_commit == "unknown" {
        eprintln!(
            "Warning: run_id={run_id} has no recorded commit; using the current HEAD instead"
        );
        workspace::head_commit(&repository_root).await?
    } else {
        recorded_commit
    };

    let worker_count = effective_worker_count(parallel, total)?;
    print_parallelism(worker_count, parallel, total);

    let mut pool =
        WorktreePool::create(repository_root, &base_commit, worker_count, keep_worktrees).await?;
    let referenced_commands = setup_command
        .into_iter()
        .chain(std::iter::once(command))
        .collect::<Vec<_>>();
    pool.link_referenced_siblings(referenced_commands)?;

    let analysis_result = async {
        if let Some(setup) = setup_command {
            run_checked_on_all(pool.workspaces(), setup, setup_timeout_secs, "setup").await?;
        }
        run_checked_on_all(pool.workspaces(), command, timeout_secs, "baseline").await?;

        let jobs = mutants
            .into_iter()
            .enumerate()
            .map(|(sequence, mutant)| {
                let target = mutant.file_path.as_deref().ok_or_else(|| {
                    MutationError::InvalidInput(format!(
                        "mutant {} has no target file path",
                        mutant.id
                    ))
                })?;
                Ok(MutantJob {
                    sequence,
                    identity: MutantIdentity::Database(mutant.id),
                    target_file: validate_target_path(target)?,
                    payload: MutantPayload::Diff(mutant.diff),
                    command: command.to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let run = execute_parallel_jobs(
            pool.workspaces(),
            jobs,
            timeout_secs,
            None,
            |job| {
                if let MutantIdentity::Database(id) = job.identity {
                    db.update_mutant_status(id, "running", command)?;
                }
                Ok(())
            },
            |result| {
                if let MutantIdentity::Database(id) = result.job.identity {
                    db.update_mutant_status(id, result.status.database_value(), command)?;
                }
                Ok(())
            },
        )
        .await?;

        let killed = run
            .results
            .iter()
            .filter(|result| result.status == MutantStatus::Killed)
            .count() as u64;
        let survived = run
            .results
            .iter()
            .filter(|result| result.status == MutantStatus::Survived)
            .count();
        let errors = run
            .results
            .iter()
            .filter(|result| result.status == MutantStatus::Error)
            .count();

        let score = killed as f64 / total as f64;
        println!(
            "\nMUTATION SCORE: {:.2}% ({} killed / {} total)",
            score * 100.0,
            killed,
            total
        );
        println!("Survived: {survived}");
        if errors > 0 {
            println!("Errors: {errors}");
        }

        Ok(ScoreSummary {
            killed,
            total: total as u64,
        })
    }
    .await;

    finish_with_cleanup(analysis_result, &mut pool).await
}

/// Analyze folder-backed mutants with isolated worktrees.
#[allow(clippy::too_many_arguments)]
pub async fn analyze_folders(
    folder_paths: Vec<PathBuf>,
    command: Option<String>,
    setup_command: Option<String>,
    jobs: u32,
    timeout_secs: u64,
    survival_threshold: f64,
    parallel: usize,
    keep_worktrees: bool,
    project_commands: &dyn ProjectCommands,
) -> Result<ScoreSummary> {
    let repository_root = workspace::repository_root(Path::new(".")).await?;
    let uses_derived_commands = command.is_none();
    let plans = load_folder_plans(
        folder_paths,
        command,
        jobs,
        project_commands,
        &repository_root,
    )?;

    let maximum_folder_size = plans.iter().map(|plan| plan.jobs.len()).max().unwrap_or(0);
    if maximum_folder_size == 0 {
        return Err(MutationError::InvalidInput(
            "No mutants found in the selected folders".to_string(),
        ));
    }

    let worker_count = effective_worker_count(parallel, maximum_folder_size)?;
    let total_mutants: usize = plans.iter().map(|plan| plan.jobs.len()).sum();
    print_parallelism(worker_count, parallel, total_mutants);

    let base_commit = workspace::head_commit(&repository_root).await?;
    let setup = match setup_command.as_deref() {
        Some(command) => Some(command.to_string()),
        None if uses_derived_commands => Some(project_commands.build_command()),
        None => None,
    };
    let mut pool = WorktreePool::create(
        repository_root.clone(),
        &base_commit,
        worker_count,
        keep_worktrees,
    )
    .await?;
    let referenced_commands = setup
        .as_deref()
        .into_iter()
        .chain(plans.iter().map(|plan| plan.command.as_str()))
        .collect::<Vec<_>>();
    pool.link_referenced_siblings(referenced_commands)?;

    let analysis_result = async {
        if let Some(setup) = setup.as_deref() {
            run_checked_on_all(
                pool.workspaces(),
                setup,
                project_commands.build_timeout_secs(),
                "setup",
            )
            .await?;
        }

        let mut checked_commands = HashSet::new();
        let mut overall = ScoreSummary::default();

        for plan in plans {
            if checked_commands.insert(plan.command.clone()) {
                run_checked_on_all(pool.workspaces(), &plan.command, timeout_secs, "baseline")
                    .await?;
            }

            let total = plan.jobs.len();
            println!("* {total} MUTANTS in {} *", plan.folder_path.display());
            let run = execute_parallel_jobs(
                pool.workspaces(),
                plan.jobs,
                timeout_secs,
                Some((survival_threshold, total)),
                |_| Ok(()),
                |_| Ok(()),
            )
            .await?;

            let killed = run
                .results
                .iter()
                .filter(|result| result.status == MutantStatus::Killed)
                .count() as u64;
            let mut survivors = run
                .results
                .iter()
                .filter_map(|result| {
                    if result.status == MutantStatus::Survived {
                        match &result.job.identity {
                            MutantIdentity::Folder(name) => {
                                Some((result.job.sequence, name.clone()))
                            }
                            MutantIdentity::Database(_) => None,
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            survivors.sort_by_key(|(sequence, _)| *sequence);
            let survivor_names = survivors
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>();

            let score = killed as f64 / total as f64;
            println!("\nMUTATION SCORE: {:.2}%", score * 100.0);
            if run.skipped > 0 {
                println!(
                    "Skipped {} mutants after the survival threshold was exceeded",
                    run.skipped
                );
            }

            generate_report(
                &survivor_names,
                plan.folder_path.to_string_lossy().as_ref(),
                plan.target_file.to_string_lossy().as_ref(),
                score,
            )
            .await?;

            overall.killed += killed;
            overall.total += total as u64;
        }

        Ok(overall)
    }
    .await;

    finish_with_cleanup(analysis_result, &mut pool).await
}

async fn finish_with_cleanup<T>(analysis_result: Result<T>, pool: &mut WorktreePool) -> Result<T> {
    let cleanup_result = pool.cleanup().await;
    match (analysis_result, cleanup_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn load_folder_plans(
    folder_paths: Vec<PathBuf>,
    command: Option<String>,
    jobs: u32,
    project_commands: &dyn ProjectCommands,
    repository_root: &Path,
) -> Result<Vec<FolderPlan>> {
    let mut plans = Vec::with_capacity(folder_paths.len());

    for folder_path in folder_paths {
        let folder_path = if folder_path.is_absolute() {
            folder_path
        } else {
            std::env::current_dir()?.join(folder_path)
        };
        let original_file_path = folder_path.join("original_file.txt");
        let raw_target = fs::read_to_string(&original_file_path)?;
        let target_file = repository_relative_path(repository_root, raw_target.trim())?;
        ensure_target_clean(repository_root, &target_file)?;

        let test_command = match command.as_ref() {
            Some(command) => command.clone(),
            None => project_commands.test_command(target_file.to_string_lossy().as_ref(), jobs)?,
        };

        let mut mutant_files = fs::read_dir(&folder_path)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.extension().is_some_and(|extension| extension != "txt")
            })
            .collect::<Vec<_>>();
        mutant_files.sort();

        if mutant_files.is_empty() {
            return Err(MutationError::InvalidInput(format!(
                "No mutants in the provided folder path ({})",
                folder_path.display()
            )));
        }

        let jobs = mutant_files
            .into_iter()
            .enumerate()
            .map(|(sequence, path)| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        MutationError::InvalidInput(format!(
                            "invalid mutant filename: {}",
                            path.display()
                        ))
                    })?
                    .to_string();

                Ok(MutantJob {
                    sequence,
                    identity: MutantIdentity::Folder(name),
                    target_file: target_file.clone(),
                    payload: MutantPayload::CompleteFile(fs::read_to_string(path)?),
                    command: test_command.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        plans.push(FolderPlan {
            folder_path,
            target_file,
            command: test_command,
            jobs,
        });
    }

    Ok(plans)
}

fn ensure_target_clean(repository_root: &Path, target: &Path) -> Result<()> {
    for args in [
        vec!["diff", "--quiet", "--"],
        vec!["diff", "--cached", "--quiet", "--"],
    ] {
        let status = std::process::Command::new("git")
            .current_dir(repository_root)
            .args(args)
            .arg(target)
            .status()
            .map_err(|e| {
                MutationError::Git(format!(
                    "failed to check whether {} is clean: {e}",
                    target.display()
                ))
            })?;
        if !status.success() {
            return Err(MutationError::InvalidInput(format!(
                "parallel folder analysis requires a clean target file: {}",
                target.display()
            )));
        }
    }
    Ok(())
}

fn repository_relative_path(repository_root: &Path, raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        let relative = path.strip_prefix(repository_root).map_err(|_| {
            MutationError::InvalidInput(format!(
                "target file {} is outside repository {}",
                path.display(),
                repository_root.display()
            ))
        })?;
        validate_target_path(relative)
    } else {
        validate_target_path(raw_path)
    }
}

fn validate_target_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(MutationError::InvalidInput(format!(
            "mutant target must be a non-empty repository-relative path: {}",
            path.display()
        )));
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(MutationError::InvalidInput(format!(
            "mutant target escapes the repository: {}",
            path.display()
        )));
    }

    Ok(path.to_path_buf())
}

fn effective_worker_count(requested: usize, mutant_count: usize) -> Result<usize> {
    if requested == 0 {
        return Err(MutationError::InvalidInput(
            "--parallel must be at least 1".to_string(),
        ));
    }
    Ok(requested.min(mutant_count))
}

fn print_parallelism(worker_count: usize, requested: usize, mutant_count: usize) {
    println!(
        "Parallel analysis: {worker_count} worker(s) for {mutant_count} mutant(s) \
         (requested {requested})"
    );
}

fn print_database_header(total: usize, run_id: i64, file_path: Option<&str>, survivors_only: bool) {
    match (file_path, survivors_only) {
        (Some(path), true) => {
            println!("* {total} SURVIVING MUTANTS in run_id={run_id} (file: {path}) *")
        }
        (Some(path), false) => {
            println!("* {total} MUTANTS in run_id={run_id} (file: {path}) *")
        }
        (None, true) => println!("* {total} SURVIVING MUTANTS in run_id={run_id} *"),
        (None, false) => println!("* {total} MUTANTS in run_id={run_id} *"),
    }
}

async fn run_checked_on_all(
    workspaces: &[WorkerWorkspace],
    command: &str,
    timeout_secs: u64,
    phase: &str,
) -> Result<()> {
    println!(
        "Running {phase} command in {} worker workspace(s)...",
        workspaces.len()
    );

    let command_runs = join_all(workspaces.iter().map(|workspace| async move {
        (
            workspace.id,
            capture_command(&workspace.path, command, timeout_secs).await,
        )
    }));
    let executions = tokio::select! {
        executions = command_runs => executions,
        signal = tokio::signal::ctrl_c() => {
            let signal_note = signal
                .err()
                .map(|error| format!(": {error}"))
                .unwrap_or_default();
            return Err(MutationError::Command(format!(
                "parallel analysis cancelled during {phase}{signal_note}"
            )));
        }
    };

    for (worker_id, execution) in executions {
        print_command_execution(
            &format!("[worker {worker_id}][{phase}]"),
            command,
            &execution,
        );
        if !execution.success {
            let setup_hint = if phase == "baseline" {
                " A fresh parallel worktree has no existing build directory; use \
                 --setup-command if the test command does not configure it."
            } else {
                ""
            };
            return Err(MutationError::InvalidInput(format!(
                "{phase} command failed in worker {worker_id}.{setup_hint}"
            )));
        }
    }

    Ok(())
}

async fn execute_parallel_jobs<OnDispatch, OnResult>(
    workspaces: &[WorkerWorkspace],
    jobs: Vec<MutantJob>,
    timeout_secs: u64,
    survival_threshold: Option<(f64, usize)>,
    mut on_dispatch: OnDispatch,
    mut on_result: OnResult,
) -> Result<ParallelRun>
where
    OnDispatch: FnMut(&MutantJob) -> Result<()>,
    OnResult: FnMut(&MutantResult) -> Result<()>,
{
    if jobs.is_empty() {
        return Ok(ParallelRun {
            results: Vec::new(),
            skipped: 0,
        });
    }

    let worker_count = workspaces.len().min(jobs.len());
    let (result_tx, mut result_rx) = mpsc::channel::<WorkerMessage>(worker_count);
    let mut job_senders = Vec::with_capacity(worker_count);
    let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(worker_count);

    for workspace in workspaces.iter().take(worker_count).cloned() {
        let (job_tx, job_rx) = mpsc::channel::<MutantJob>(1);
        let worker_result_tx = result_tx.clone();
        handles.push(tokio::spawn(worker_loop(
            workspace,
            job_rx,
            worker_result_tx,
            timeout_secs,
        )));
        job_senders.push(job_tx);
    }
    drop(result_tx);

    let mut next_job = 0usize;
    let mut in_flight = 0usize;
    let mut active_jobs = vec![None; worker_count];
    for sender in &job_senders {
        let job = jobs[next_job].clone();
        if let Err(error) = on_dispatch(&job) {
            abort_workers(job_senders, handles).await;
            return Err(error);
        }
        active_jobs[next_job] = Some(job.clone());
        if sender.send(job).await.is_err() {
            abort_workers(job_senders, handles).await;
            return Err(MutationError::Command(
                "parallel worker stopped before receiving a job".to_string(),
            ));
        }
        next_job += 1;
        in_flight += 1;
    }

    let mut results = Vec::with_capacity(jobs.len());
    let mut stop_scheduling = false;
    let mut fatal_error = None;

    while in_flight > 0 {
        let message = tokio::select! {
            message = result_rx.recv() => {
                match message {
                    Some(message) => message,
                    None => {
                        abort_workers(job_senders, handles).await;
                        return Err(MutationError::Command(
                            "all parallel workers stopped before returning their results"
                                .to_string(),
                        ));
                    }
                }
            }
            signal = tokio::signal::ctrl_c() => {
                let signal_note = signal
                    .err()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default();
                for (worker_id, job) in active_jobs.iter().enumerate() {
                    if let Some(job) = job {
                        let cancelled = error_result(
                            worker_id,
                            job.clone(),
                            "analysis cancelled before the mutant completed".to_string(),
                            false,
                        );
                        let _ = on_result(&cancelled);
                    }
                }
                abort_workers(job_senders, handles).await;
                return Err(MutationError::Command(format!(
                    "parallel analysis cancelled{signal_note}"
                )));
            }
        };
        in_flight -= 1;

        print_mutant_result(&message.result, jobs.len());

        let worker_id = message.result.worker_id;
        active_jobs[worker_id] = None;
        if let Err(error) = on_result(&message.result) {
            stop_scheduling = true;
            if fatal_error.is_none() {
                fatal_error = Some(error);
            }
        }
        if !message.result.workspace_usable {
            stop_scheduling = true;
            if fatal_error.is_none() {
                fatal_error = Some(MutationError::Command(
                    message
                        .result
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("worker {worker_id} became unusable")),
                ));
            }
        }
        results.push(message.result);

        if let Some((threshold, total)) = survival_threshold {
            let survived = results
                .iter()
                .filter(|result| result.status == MutantStatus::Survived)
                .count();
            let rate = survived as f64 / total as f64;
            if !stop_scheduling && rate > threshold {
                println!(
                    "\nTerminating early: {:.2}% mutants surviving after {} completed mutants",
                    rate * 100.0,
                    results.len()
                );
                println!(
                    "Survival rate exceeds threshold of {:.0}%",
                    threshold * 100.0
                );
                stop_scheduling = true;
            }
        }

        if !stop_scheduling && next_job < jobs.len() {
            let job = jobs[next_job].clone();
            match on_dispatch(&job) {
                Ok(()) => {
                    active_jobs[worker_id] = Some(job.clone());
                    if job_senders[worker_id].send(job).await.is_err() {
                        active_jobs[worker_id] = None;
                        stop_scheduling = true;
                        if fatal_error.is_none() {
                            fatal_error = Some(MutationError::Command(format!(
                                "worker {worker_id} stopped before receiving its next job"
                            )));
                        }
                    } else {
                        next_job += 1;
                        in_flight += 1;
                    }
                }
                Err(error) => {
                    stop_scheduling = true;
                    if fatal_error.is_none() {
                        fatal_error = Some(error);
                    }
                }
            }
        }
    }

    drop(job_senders);
    for handle in handles {
        handle
            .await
            .map_err(|e| MutationError::Command(format!("parallel worker task failed: {e}")))?;
    }

    if let Some(error) = fatal_error {
        return Err(error);
    }

    results.sort_by_key(|result| result.job.sequence);
    Ok(ParallelRun {
        skipped: jobs.len() - next_job,
        results,
    })
}

async fn abort_workers(job_senders: Vec<mpsc::Sender<MutantJob>>, handles: Vec<JoinHandle<()>>) {
    drop(job_senders);
    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        let _ = handle.await;
    }
}

async fn worker_loop(
    workspace: WorkerWorkspace,
    mut jobs: mpsc::Receiver<MutantJob>,
    results: mpsc::Sender<WorkerMessage>,
    timeout_secs: u64,
) {
    while let Some(job) = jobs.recv().await {
        let result = execute_mutant(&workspace, job, timeout_secs).await;
        let usable = result.workspace_usable;
        if results.send(WorkerMessage { result }).await.is_err() || !usable {
            break;
        }
    }
}

async fn execute_mutant(
    workspace: &WorkerWorkspace,
    job: MutantJob,
    timeout_secs: u64,
) -> MutantResult {
    if let Err(error) = restore_file(&workspace.path, &job.target_file).await {
        return error_result(workspace.id, job, error.to_string(), false);
    }

    let apply_result = match &job.payload {
        MutantPayload::Diff(diff) => apply_diff(&workspace.path, diff).await,
        MutantPayload::CompleteFile(content) => {
            fs::write(workspace.path.join(&job.target_file), content).map_err(Into::into)
        }
    };

    if let Err(error) = apply_result {
        let restore_result = restore_file(&workspace.path, &job.target_file).await;
        let usable = restore_result.is_ok();
        let detail = match restore_result {
            Ok(()) => format!("failed to apply mutant: {error}"),
            Err(restore_error) => {
                format!("failed to apply mutant: {error}; restore also failed: {restore_error}")
            }
        };
        return error_result(workspace.id, job, detail, usable);
    }

    let execution = capture_command(&workspace.path, &job.command, timeout_secs).await;
    let status = if execution.success {
        MutantStatus::Survived
    } else {
        MutantStatus::Killed
    };

    match restore_file(&workspace.path, &job.target_file).await {
        Ok(()) => MutantResult {
            worker_id: workspace.id,
            job,
            status,
            command: Some(execution),
            error: None,
            workspace_usable: true,
        },
        Err(error) => MutantResult {
            worker_id: workspace.id,
            job,
            status: MutantStatus::Error,
            command: Some(execution),
            error: Some(format!("failed to restore worker workspace: {error}")),
            workspace_usable: false,
        },
    }
}

fn error_result(
    worker_id: usize,
    job: MutantJob,
    error: String,
    workspace_usable: bool,
) -> MutantResult {
    MutantResult {
        worker_id,
        job,
        status: MutantStatus::Error,
        command: None,
        error: Some(error),
        workspace_usable,
    }
}

async fn apply_diff(workspace: &Path, diff: &str) -> Result<()> {
    use std::io::Write;

    let mut patch = NamedTempFile::new()?;
    patch.write_all(diff.as_bytes())?;
    patch.flush()?;

    let output = TokioCommand::new("git")
        .current_dir(workspace)
        .args(["apply", "--whitespace=nowarn"])
        .arg(patch.path())
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("git apply failed: {e}")))?;

    if !output.status.success() {
        return Err(MutationError::Git(format!(
            "git apply error: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

async fn restore_file(workspace: &Path, target_file: &Path) -> Result<()> {
    let output = TokioCommand::new("git")
        .current_dir(workspace)
        .args(["restore", "--worktree", "--"])
        .arg(target_file)
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("git restore failed: {e}")))?;

    if !output.status.success() {
        return Err(MutationError::Git(format!(
            "git restore failed for {}: {}",
            target_file.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

async fn capture_command(workspace: &Path, command: &str, timeout_secs: u64) -> CommandExecution {
    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let mut child = TokioCommand::new(shell);
    child
        .current_dir(workspace)
        .arg(shell_arg)
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    match timeout(Duration::from_secs(timeout_secs), child.output()).await {
        Ok(Ok(output)) => CommandExecution {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            timed_out: false,
            spawn_error: None,
        },
        Ok(Err(error)) => CommandExecution {
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            spawn_error: Some(error.to_string()),
        },
        Err(_) => CommandExecution {
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
            spawn_error: None,
        },
    }
}

fn print_mutant_result(result: &MutantResult, total: usize) {
    let identity = match &result.job.identity {
        MutantIdentity::Database(id) => format!("mutant {id}"),
        MutantIdentity::Folder(name) => name.clone(),
    };
    let status = match result.status {
        MutantStatus::Killed => "KILLED ✅",
        MutantStatus::Survived => "NOT KILLED ❌",
        MutantStatus::Error => "ERROR",
    };
    let prefix = format!(
        "[worker {}][{}/{}][{}]",
        result.worker_id,
        result.job.sequence + 1,
        total,
        identity
    );
    println!("{prefix} {status}");

    if let Some(execution) = &result.command {
        print_command_execution(&prefix, &result.job.command, execution);
    }
    if let Some(error) = &result.error {
        eprintln!("{prefix} {error}");
    }
}

fn print_command_execution(prefix: &str, command: &str, execution: &CommandExecution) {
    println!("{prefix} Command: {command}");
    if execution.timed_out {
        println!("{prefix} Command timed out");
    } else if let Some(error) = &execution.spawn_error {
        println!("{prefix} Command execution failed: {error}");
    } else {
        println!("{prefix} Exit code: {}", execution.exit_code.unwrap_or(-1));
    }
    if !execution.stdout.is_empty() {
        println!("{prefix} STDOUT:\n{}", execution.stdout);
    }
    if !execution.stderr.is_empty() {
        println!("{prefix} STDERR:\n{}", execution.stderr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn worker_count_accepts_any_positive_value_and_caps_to_mutants() {
        assert_eq!(effective_worker_count(1, 9).unwrap(), 1);
        assert_eq!(effective_worker_count(3, 9).unwrap(), 3);
        assert_eq!(effective_worker_count(20, 9).unwrap(), 9);
        assert!(effective_worker_count(0, 9).is_err());
    }

    #[test]
    fn target_path_must_stay_inside_repository() {
        assert_eq!(
            validate_target_path("src/example.cpp").unwrap(),
            PathBuf::from("src/example.cpp")
        );
        assert!(validate_target_path("../example.cpp").is_err());
        assert!(validate_target_path("/tmp/example.cpp").is_err());
        assert!(validate_target_path("").is_err());
    }

    #[tokio::test]
    async fn arbitrary_worker_pool_processes_each_mutant_in_isolation() {
        let repository = initialized_repository();

        let base = workspace::head_commit(repository.path()).await.unwrap();
        let mut pool = WorktreePool::create(repository.path().to_path_buf(), &base, 4, false)
            .await
            .unwrap();
        let workspace_paths = pool
            .workspaces()
            .iter()
            .map(|workspace| workspace.path.clone())
            .collect::<Vec<_>>();

        let jobs = (0..9)
            .map(|sequence| MutantJob {
                sequence,
                identity: MutantIdentity::Folder(format!("mutant-{sequence}")),
                target_file: PathBuf::from("value.txt"),
                payload: MutantPayload::CompleteFile(if sequence % 2 == 0 {
                    "killed\n".to_string()
                } else {
                    "base\n".to_string()
                }),
                command: "! grep -q '^killed$' value.txt".to_string(),
            })
            .collect::<Vec<_>>();

        let run = execute_parallel_jobs(pool.workspaces(), jobs, 5, None, |_| Ok(()), |_| Ok(()))
            .await
            .unwrap();

        assert_eq!(run.results.len(), 9);
        assert_eq!(
            run.results
                .iter()
                .filter(|result| result.status == MutantStatus::Killed)
                .count(),
            5
        );
        assert_eq!(
            run.results
                .iter()
                .filter(|result| result.status == MutantStatus::Survived)
                .count(),
            4
        );
        let used_workers = run
            .results
            .iter()
            .map(|result| result.worker_id)
            .collect::<HashSet<_>>();
        assert_eq!(used_workers.len(), 4);
        assert_eq!(
            fs::read_to_string(repository.path().join("value.txt")).unwrap(),
            "base\n"
        );

        pool.cleanup().await.unwrap();
        assert!(workspace_paths.iter().all(|path| !path.exists()));
    }

    #[tokio::test]
    async fn threshold_stops_dispatching_new_jobs_but_finishes_in_flight_jobs() {
        let repository = initialized_repository();
        let base = workspace::head_commit(repository.path()).await.unwrap();
        let mut pool = WorktreePool::create(repository.path().to_path_buf(), &base, 2, false)
            .await
            .unwrap();
        let jobs = (0..8)
            .map(|sequence| MutantJob {
                sequence,
                identity: MutantIdentity::Folder(format!("mutant-{sequence}")),
                target_file: PathBuf::from("value.txt"),
                payload: MutantPayload::CompleteFile("survives\n".to_string()),
                command: "true".to_string(),
            })
            .collect::<Vec<_>>();

        let run = execute_parallel_jobs(
            pool.workspaces(),
            jobs,
            5,
            Some((0.0, 8)),
            |_| Ok(()),
            |_| Ok(()),
        )
        .await
        .unwrap();

        assert_eq!(run.results.len(), 2);
        assert_eq!(run.skipped, 6);
        assert!(run
            .results
            .iter()
            .all(|result| result.status == MutantStatus::Survived));
        pool.cleanup().await.unwrap();
    }

    fn initialized_repository() -> tempfile::TempDir {
        let repository = tempdir().unwrap();
        run_git(repository.path(), &["init", "-q"]);
        run_git(
            repository.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(repository.path(), &["config", "user.name", "Test"]);
        run_git(repository.path(), &["config", "commit.gpgsign", "false"]);
        fs::write(repository.path().join("value.txt"), "base\n").unwrap();
        run_git(repository.path(), &["add", "value.txt"]);
        run_git(repository.path(), &["commit", "-qm", "base"]);
        repository
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
}
