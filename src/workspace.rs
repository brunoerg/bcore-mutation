//! Isolated Git worktrees used by parallel mutant analysis.

use crate::error::{MutationError, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tokio::process::Command as TokioCommand;

#[derive(Clone, Debug)]
pub struct WorkerWorkspace {
    pub id: usize,
    pub path: PathBuf,
}

/// Owns the temporary Git worktrees created for one parallel analysis run.
///
/// Normal cleanup is asynchronous and should be performed with [`cleanup`].
/// `Drop` is a best-effort fallback for early returns and panics.
pub struct WorktreePool {
    repository_root: PathBuf,
    temporary_root: Option<TempDir>,
    workspaces: Vec<WorkerWorkspace>,
    keep: bool,
    cleaned: bool,
}

impl WorktreePool {
    pub async fn create(
        repository_root: PathBuf,
        base_commit: &str,
        worker_count: usize,
        keep: bool,
    ) -> Result<Self> {
        if worker_count == 0 {
            return Err(MutationError::InvalidInput(
                "parallel worker count must be at least 1".to_string(),
            ));
        }

        verify_commit(&repository_root, base_commit).await?;

        let temporary_root = tempfile::Builder::new()
            .prefix("bcore-mutation-workers-")
            .tempdir()?;
        let mut pool = Self {
            repository_root,
            temporary_root: Some(temporary_root),
            workspaces: Vec::with_capacity(worker_count),
            keep,
            cleaned: false,
        };

        for id in 0..worker_count {
            let path = pool
                .temporary_root
                .as_ref()
                .expect("temporary root exists while creating worktrees")
                .path()
                .join(format!("worker-{id}"));

            let add_worktree = TokioCommand::new("git")
                .current_dir(&pool.repository_root)
                .args(["worktree", "add", "--detach"])
                .arg(&path)
                .arg(base_commit)
                .output();
            let output = tokio::select! {
                output = add_worktree => output.map_err(|e| {
                    MutationError::Git(format!("failed to create worker {id} worktree: {e}"))
                })?,
                signal = tokio::signal::ctrl_c() => {
                    let signal_note = signal
                        .err()
                        .map(|error| format!(": {error}"))
                        .unwrap_or_default();
                    let cleanup_error = pool.cleanup().await.err();
                    let cleanup_note = cleanup_error
                        .map(|e| format!("; cleanup also failed: {e}"))
                        .unwrap_or_default();
                    return Err(MutationError::Command(format!(
                        "parallel analysis cancelled while creating worker {id} worktree{signal_note}{cleanup_note}"
                    )));
                }
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let cleanup_error = pool.cleanup().await.err();
                let cleanup_note = cleanup_error
                    .map(|e| format!("; cleanup also failed: {e}"))
                    .unwrap_or_default();
                return Err(MutationError::Git(format!(
                    "git worktree add failed for worker {id}: {}{}",
                    stderr.trim(),
                    cleanup_note
                )));
            }

            pool.workspaces.push(WorkerWorkspace { id, path });
        }

        Ok(pool)
    }

    pub fn workspaces(&self) -> &[WorkerWorkspace] {
        &self.workspaces
    }

    /// Mirror repository sibling paths referenced by commands as `../name`.
    ///
    /// Parallel workers live under a temporary root, so a command that worked
    /// from the original checkout with `../qa-assets` would otherwise point at
    /// the wrong parent directory. Symlinking referenced siblings into the
    /// temporary root preserves that common Bitcoin Core layout without copying
    /// large corpora.
    pub fn link_referenced_siblings<'a>(
        &self,
        commands: impl IntoIterator<Item = &'a str>,
    ) -> Result<()> {
        let Some(temporary_root) = &self.temporary_root else {
            return Ok(());
        };
        let Some(repository_parent) = self.repository_root.parent() else {
            return Ok(());
        };

        for name in referenced_sibling_names(commands) {
            let source = repository_parent.join(&name);
            if !source.exists() {
                continue;
            }

            let destination = temporary_root.path().join(&name);
            if destination.exists() {
                continue;
            }

            symlink_path(&source, &destination)?;
            println!(
                "Parallel workers linked ../{} to {}",
                name,
                source.display()
            );
        }

        Ok(())
    }

    pub async fn cleanup(&mut self) -> Result<()> {
        if self.cleaned {
            return Ok(());
        }

        if self.keep {
            if let Some(root) = self.temporary_root.take() {
                let path = root.keep();
                println!("Parallel worker worktrees kept at {}", path.display());
            }
            self.cleaned = true;
            return Ok(());
        }

        let mut first_error = None;
        for workspace in self.workspaces.iter().rev() {
            let output = TokioCommand::new("git")
                .current_dir(&self.repository_root)
                .args(["worktree", "remove", "--force"])
                .arg(&workspace.path)
                .output()
                .await;

            match output {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    if first_error.is_none() {
                        first_error = Some(MutationError::Git(format!(
                            "failed to remove worker {} worktree: {}",
                            workspace.id,
                            String::from_utf8_lossy(&output.stderr).trim()
                        )));
                    }
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(MutationError::Git(format!(
                            "failed to remove worker {} worktree: {e}",
                            workspace.id
                        )));
                    }
                }
            }
        }

        self.workspaces.clear();
        self.temporary_root.take();
        self.cleaned = true;

        let _ = TokioCommand::new("git")
            .current_dir(&self.repository_root)
            .args(["worktree", "prune"])
            .output()
            .await;

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for WorktreePool {
    fn drop(&mut self) {
        if self.cleaned || self.keep {
            return;
        }

        for workspace in self.workspaces.iter().rev() {
            let _ = Command::new("git")
                .current_dir(&self.repository_root)
                .args(["worktree", "remove", "--force"])
                .arg(&workspace.path)
                .output();
        }

        let _ = Command::new("git")
            .current_dir(&self.repository_root)
            .args(["worktree", "prune"])
            .output();
    }
}

pub async fn repository_root(start: &Path) -> Result<PathBuf> {
    let output = TokioCommand::new("git")
        .current_dir(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("failed to locate Git repository: {e}")))?;

    if !output.status.success() {
        return Err(MutationError::Git(format!(
            "not inside a Git repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

pub async fn head_commit(repository_root: &Path) -> Result<String> {
    let output = TokioCommand::new("git")
        .current_dir(repository_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("failed to resolve HEAD: {e}")))?;

    if !output.status.success() {
        return Err(MutationError::Git(format!(
            "failed to resolve HEAD: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn verify_commit(repository_root: &Path, commit: &str) -> Result<()> {
    let object = format!("{commit}^{{commit}}");
    let output = TokioCommand::new("git")
        .current_dir(repository_root)
        .args(["cat-file", "-e", &object])
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("failed to verify commit {commit}: {e}")))?;

    if !output.status.success() {
        return Err(MutationError::Git(format!(
            "commit {commit} is not available in the current repository"
        )));
    }

    Ok(())
}

fn referenced_sibling_names<'a>(commands: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for command in commands {
        let mut search_start = 0usize;
        while let Some(relative_index) = command[search_start..].find("../") {
            let index = search_start + relative_index;
            let after_parent = &command[index + 3..];
            let preceded_by_path_component = command[..index]
                .chars()
                .next_back()
                .is_some_and(|character| character == '.' || character == '/');
            if preceded_by_path_component {
                search_start = index + 3;
                continue;
            }
            let name = after_parent
                .split(|character: char| {
                    character == '/'
                        || character.is_whitespace()
                        || matches!(
                            character,
                            '"' | '\'' | '`' | ';' | '|' | '&' | '(' | ')' | '<' | '>' | '\\'
                        )
                })
                .next()
                .unwrap_or_default();

            if is_valid_sibling_name(name) {
                names.insert(name.to_string());
            }

            search_start = index + 3 + name.len();
        }
    }
    names
}

fn is_valid_sibling_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".."
}

#[cfg(unix)]
fn symlink_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, destination)
    } else {
        std::os::windows::fs::symlink_file(source, destination)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::fs::create_dir_all(destination)
    } else {
        std::fs::copy(source, destination).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_references_are_extracted_from_shell_commands() {
        let names = referenced_sibling_names([
            "FUZZ=x ./build/bin/fuzz ../qa-assets/fuzz_corpora/x",
            "FOO='../depends' ./script --path ../qa-assets",
            "echo ../../outside ../.hidden ../two-words",
        ]);

        assert!(names.contains("qa-assets"));
        assert!(names.contains("depends"));
        assert!(names.contains(".hidden"));
        assert!(names.contains("two-words"));
        assert!(!names.contains(".."));
        assert_eq!(names.len(), 4);
    }
}
