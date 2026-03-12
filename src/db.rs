use crate::error::{MutationError, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::Path;

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
  id              INTEGER PRIMARY KEY,
  name            TEXT NOT NULL,
  repository_url  TEXT,
  UNIQUE(name),
  UNIQUE(repository_url)
);

CREATE TABLE IF NOT EXISTS runs (
  id              INTEGER PRIMARY KEY,
  project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  commit_hash     TEXT NOT NULL,
  pr_number       INTEGER,
  created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  tool_version    TEXT,
  config_json     TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_project_created ON runs(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_commit ON runs(commit_hash);

CREATE TABLE IF NOT EXISTS mutants (
  id              INTEGER PRIMARY KEY,
  run_id          INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  diff            TEXT NOT NULL,
  patch_hash      TEXT NOT NULL,
  status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','running','killed','survived',
                                      'timeout','error','skipped','equivalent','unproductive')),
  killed          INTEGER GENERATED ALWAYS AS (CASE WHEN status='killed' THEN 1 ELSE 0 END) VIRTUAL,
  command_to_test TEXT,
  file_path       TEXT,
  operator        TEXT,
  UNIQUE(run_id, patch_hash)
);

CREATE INDEX IF NOT EXISTS idx_mutants_run_status ON mutants(run_id, status);
CREATE INDEX IF NOT EXISTS idx_mutants_file ON mutants(file_path);
CREATE INDEX IF NOT EXISTS idx_mutants_operator ON mutants(operator);
CREATE INDEX IF NOT EXISTS idx_mutants_killed ON mutants(killed);
";

/// Data collected during mutation for a single generated mutant.
pub struct MutantData {
    pub diff: String,
    pub patch_hash: String,
    pub file_path: String,
    pub operator: String,
}

/// A mutant row read back from the database.
pub struct MutantRow {
    pub id: i64,
    pub diff: String,
    pub file_path: Option<String>,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at `path` and enable foreign keys.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Database { conn })
    }

    /// Create tables and indexes if they do not yet exist, and apply any
    /// additive migrations needed for older databases.
    pub fn ensure_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        // Migration: add config_json to runs if the column is missing.
        // ALTER TABLE ADD COLUMN fails with "duplicate column name" when the
        // column already exists; silence that specific error so the function
        // is idempotent on databases created before this column was added.
        if let Err(e) = self
            .conn
            .execute_batch("ALTER TABLE runs ADD COLUMN config_json TEXT;")
        {
            if !e.to_string().contains("duplicate column name") {
                return Err(e.into());
            }
        }
        Ok(())
    }

    /// Insert the Bitcoin Core project row if not already present.
    pub fn seed_projects(&self) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO projects (name, repository_url) VALUES (?1, ?2)",
            params!["Bitcoin Core", "https://github.com/bitcoin/bitcoin"],
        )?;
        Ok(())
    }

    /// Return the id of the Bitcoin Core project row.
    pub fn get_bitcoin_core_project_id(&self) -> Result<i64> {
        let id = self.conn.query_row(
            "SELECT id FROM projects WHERE name = 'Bitcoin Core'",
            [],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Create a new run row and return its id.
    pub fn create_run(
        &self,
        project_id: i64,
        commit_hash: &str,
        tool_version: &str,
        pr_number: Option<u32>,
        config_json: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO runs (project_id, commit_hash, tool_version, pr_number, config_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, commit_hash, tool_version, pr_number, config_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Batch-insert mutants under `run_id` using a single transaction.
    /// Duplicates (same run_id + patch_hash) are silently ignored.
    pub fn insert_mutant_batch(&mut self, run_id: i64, mutants: &[MutantData]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO mutants
                   (run_id, diff, patch_hash, status, file_path, operator)
                 VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
            )?;
            for m in mutants {
                stmt.execute(params![
                    run_id,
                    m.diff,
                    m.patch_hash,
                    m.file_path,
                    m.operator
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Return mutants belonging to `run_id`, optionally filtered by `file_path`.
    /// When `survivors_only` is true, only mutants with status `'survived'` are returned.
    pub fn get_mutants_for_run(
        &self,
        run_id: i64,
        file_path: Option<&str>,
        survivors_only: bool,
    ) -> Result<Vec<MutantRow>> {
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(MutantRow {
                id: row.get(0)?,
                diff: row.get(1)?,
                file_path: row.get(2)?,
            })
        };

        let rows: Vec<MutantRow> = match (file_path, survivors_only) {
            (Some(fp), false) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, diff, file_path FROM mutants WHERE run_id = ?1 AND file_path = ?2",
                )?;
                let rows = stmt.query_map(params![run_id, fp], map_row)?
                    .collect::<rusqlite::Result<_>>()?;
                rows
            }
            (Some(fp), true) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, diff, file_path FROM mutants \
                     WHERE run_id = ?1 AND file_path = ?2 AND status = 'survived'",
                )?;
                let rows = stmt.query_map(params![run_id, fp], map_row)?
                    .collect::<rusqlite::Result<_>>()?;
                rows
            }
            (None, false) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, diff, file_path FROM mutants WHERE run_id = ?1",
                )?;
                let rows = stmt.query_map(params![run_id], map_row)?
                    .collect::<rusqlite::Result<_>>()?;
                rows
            }
            (None, true) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, diff, file_path FROM mutants \
                     WHERE run_id = ?1 AND status = 'survived'",
                )?;
                let rows = stmt.query_map(params![run_id], map_row)?
                    .collect::<rusqlite::Result<_>>()?;
                rows
            }
        };

        Ok(rows)
    }

    /// Update the status and command_to_test for a single mutant.
    pub fn update_mutant_status(&self, id: i64, status: &str, command: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE mutants SET status = ?1, command_to_test = ?2 WHERE id = ?3",
            params![status, command, id],
        )?;
        Ok(())
    }
}

/// Compute the SHA-256 hex digest of `diff`.
pub fn compute_patch_hash(diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate a proper unified diff by running `git diff --no-index` between the
/// original file on disk and a temp file containing `mutated_content`.
/// The resulting patch includes context lines and is suitable for `git apply`.
pub async fn generate_diff(file_path: &str, mutated_content: &str) -> Result<String> {
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::process::Command;

    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(mutated_content.as_bytes())?;
    tmp.flush()?;

    let tmp_path = tmp.path().to_string_lossy().to_string();

    // `git diff --no-index` exits with 1 when differences exist — that is expected.
    let output = Command::new("git")
        .args(["diff", "--no-index", "--", file_path, &tmp_path])
        .output()
        .await
        .map_err(|e| MutationError::Git(format!("git diff failed to spawn: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if stdout.is_empty() {
        return Err(MutationError::Git(format!(
            "git diff produced no output for {}",
            file_path
        )));
    }

    // Fix the temp-file path back to the real file path in the diff headers.
    // `git diff --no-index` shows the second argument's path in `+++ b/` and
    // `diff --git … b/…`; replace those with `file_path`.
    let fixed = stdout
        .lines()
        .map(|line| {
            if line.starts_with("+++ ") {
                format!("+++ b/{}", file_path)
            } else if line.starts_with("diff --git ") {
                format!("diff --git a/{} b/{}", file_path, file_path)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Preserve trailing newline present in git diff output.
    let fixed = if stdout.ends_with('\n') {
        fixed + "\n"
    } else {
        fixed
    };

    Ok(fixed)
}
