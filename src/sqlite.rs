use std::path::PathBuf;
use rusqlite::{Connection, Result};

pub fn store_mutants(db_path: &PathBuf) -> Result<()> {
    let connection = Connection::open(db_path)?;
    
    connection.execute_batch("
        PRAGMA foreign_keys = ON;
        
        -- Projects
        CREATE TABLE IF NOT EXISTS projects (
        id              INTEGER PRIMARY KEY,
        name            TEXT NOT NULL,
        repository_url  TEXT,
        UNIQUE(name),
        UNIQUE(repository_url)
        );

        --Runs
        CREATE TABLE IF NOT EXISTS runs (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id      INTEGER NOT NULL,
            commit_hash     TEXT NOT NULL,
            pr_number       INTEGER,
            created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(project_id) REFERENCES projects(id)
        );

        CREATE INDEX IF NOT EXISTS idx_runs_project_created ON runs(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_runs_commit ON runs(commit_hash);

        CREATE TABLE IF NOT EXISTS mutants (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id              INTEGER NOT NULL,
            diff                TEXT NOT NULL,
            patch_hash          TEXT NOT NULL,
            status              TEXT NOT NULL DEFAULT 'pending',
            command_to_test     TEXT,
            file_path           TEXT NOT NULL,
            operator            TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES runs(id)
        );

    ")?;


    println!("ok batch");

    //fillment test on tables
    connection.execute("
        INSERT INTO projects(id, name, repository_url)
        VALUES(1, 'teste local', 'https://github.com/JGsouzaa/bcore-mutation');
    ", [])?;

    println!("insert into projects ok");


    connection.execute("
        INSERT INTO runs(project_id, commit_hash, pr_number)
        VALUES(1, 'a1b2c3d4e5f6g7h8i9j0', 40);

    ", [])?;

    println!("insert into runs ok");

    //PRAGMA index_info('idx_runs_project_created');
    //PRAGMA index_info('idx_runs_commit');

    // TODO first-time initialization

    Ok(())
}
