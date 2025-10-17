use rusqlite::params;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use rusqlite::{Connection, Result};

use crate::git_changes::{get_commit_hash};


pub fn store_mutants(db_path: &PathBuf, run_id: i64) -> Result<()> {
    
    println!("SQLite option: Storing mutants on {}", db_path.display());
    let connection = Connection::open(db_path)?;




    //run_id
    println!("run_id: {}", run_id.to_string());
    //diff
    //patch_hash
    //command_to_test
    //file_path
    //operator

    /*
    connection.execute("

        INSERT INTO  mutants (run_id , diff, patch_hash, command_to_test, file_path, operator)
        VALUES (?1, ?2, ?3, ?4);
    ", params![run_id, commit_hash, pr_number, tool_version],)?;
    */
    //Filling mutants table



    // Fazer preenchimento da ultima tabela (mutants)
    // TODO fill tables with run
    // TODO test functionality
    // TODO script test

    Ok(())
}



pub fn store_run(db_path: &PathBuf, pr_number: Option<u32>) -> Result<i64> {

    println!("SQLite option: Storing current run on {}", db_path.display());
    let connection = Connection::open(db_path)?;

    let proj_query_row: (i32, String) = connection.query_row(
        "SELECT id, name FROM projects;",
        [],
        |row| Ok((row.get(0)?, row.get(1)?))
    )?;

    let project_id = proj_query_row.0;
    println!("id: {}", project_id);
    
    /*
    let commit_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
*/
  
    let commit_hash = match get_commit_hash() {
        Ok(hash) => hash,
        Err(_) => "unknown".to_string(),
    };

    println!("commit hash: {}", commit_hash);

    let tool_version = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    
    connection.execute("

        INSERT INTO  runs (project_id , commit_hash, pr_number, tool_version)
        VALUES (?1, ?2, ?3, ?4);
    ", params![project_id, commit_hash, pr_number, tool_version],)?;
    
    let run_id = connection.last_insert_rowid();

    Ok(run_id)
}

fn _check_initial_row(connection: &Connection) -> Result<()> {
    println!("SQLite option: Checking first row of projects...");

    let result = connection.query_row(
        "SELECT id, name, repository_url FROM projects WHERE id = 1;",
        [],
        |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    );

    match result {
        Ok((id, name, repo)) => {
            if id == 1 && name == "Bitcoin Core" && repo == "https://github.com/bitcoin/bitcoin" {
                println!("SQLite option: Project table corrected filled!");
            }
        },
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            println!("SQLite option: No matches found for projects table, filling initial row...");
            _fill_projects_table(&connection)?;
        },
        Err(e) => {
            eprintln!("SQLite option: FAILED to verify initial project: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

fn _fill_projects_table(connection: &Connection) -> Result<()> {
    connection.execute("
        ---First time initialization
        INSERT OR IGNORE INTO projects (id, name, repository_url)
        VALUES (1, 'Bitcoin Core', 'https://github.com/bitcoin/bitcoin');
    ", [])?;

    Ok(())
}

fn _check_schema(connection: &Connection) -> Result<()> {
    println!("SQLite option: Checking schema integrity...");

    let expected_tables = vec!["projects", "runs", "mutants"];
    for table in expected_tables {
        let exists: bool = connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1;",
            params![table],
            |row| row.get(0),
        )?;

        if !exists {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(format!("Missing table: {}", table)),
            ));
        }
    }

    // Verificação de colunas essenciais por tabela (incluindo colunas virtuais)
    let table_columns: Vec<(&str, Vec<&str>)> = vec![
        ("projects", vec!["id", "name", "repository_url"]),
        ("runs", vec!["id", "project_id", "commit_hash", "pr_number", "created_at", "tool_version"]),
        ("mutants", vec![
            "id", "run_id", "diff", "patch_hash", "status", "killed", 
            "command_to_test", "file_path", "operator"
        ]),
    ];

    for (table, columns) in table_columns {
        let mut stmt = connection.prepare(&format!("PRAGMA table_xinfo({});", table))?;
        let column_names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))? 
            .filter_map(Result::ok)
            .collect();

        for col in columns {
            if !column_names.contains(&col.to_string()) {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(1),
                    Some(format!("Missing column '{}' in table '{}'", col, table)),
                ));
            }
        }
    }

    println!("SQLite option: Schema verified successfully.");
    Ok(())
}

fn _createdb(connection: &Connection) -> Result<()> {
    println!("SQLite option:: New db detected initializing first fillment...");

    // DB tables creation
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
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            commit_hash     TEXT NOT NULL,
            pr_number       INTEGER,
            created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            tool_version    TEXT,
            FOREIGN KEY(project_id) REFERENCES projects(id)
        );

        CREATE INDEX IF NOT EXISTS idx_runs_project_created ON runs(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_runs_commit ON runs(commit_hash);

        CREATE TABLE IF NOT EXISTS mutants (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
            diff                TEXT NOT NULL,
            patch_hash          TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending','running','killed','survived','timeout','error','skipped','equivalent','unproductive')),
            killed INTEGER GENERATED ALWAYS AS (CASE WHEN status='killed' THEN 1 ELSE 0 END) VIRTUAL,
            command_to_test     TEXT,
            file_path           TEXT NOT NULL,
            operator            TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES runs(id)
        );

        CREATE INDEX IF NOT EXISTS idx_mutants_run_status ON mutants(run_id, status);
        CREATE INDEX IF NOT EXISTS idx_mutants_file ON mutants(file_path);
        CREATE INDEX IF NOT EXISTS idx_mutants_operator ON mutants(operator);
        CREATE INDEX IF NOT EXISTS idx_mutants_killed ON mutants(killed);

    ")?;

    println!("SQLite option: Ok batch");

    //Filling projects table
    _fill_projects_table(&connection)?;

    Ok(())
}

pub fn check_db(db_path: &PathBuf) -> Result<()> {
    
    println!("SQLite option: Checking if db exist...");
    let is_new_db = !db_path.exists();
    
    //Verify path integrity
    let exist_path = Path::new("db");
    if !exist_path.exists() {
        match fs::create_dir_all(exist_path) {
            Ok(_) => {},
            Err(e) => {
                eprintln!("FAIL creating new folder db: {}", e);
                std::process::exit(1);
            }
        }
    }

    let connection = Connection::open(db_path)?;
    
    if is_new_db {
        _createdb(&connection)?;

    } else {
        println!("SQLite option: Current db exists!");
        _check_schema(&connection)?;
        _check_initial_row(&connection)?;
    }

    Ok(())
}