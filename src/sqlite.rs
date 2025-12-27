use sha2::{Sha256, Digest};
use std::error::Error;
use std::process::Command;
use rusqlite::params;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use rusqlite::{Connection, Result, Params};

use crate::git_changes::{get_commit_hash};
use crate::error::{MutationError};

fn update_mutants_table<P>(connection: &Connection, sql: &str, params: P) -> Result<(), MutationError>
where
    P: Params,
{
    connection.execute(sql, params)?;

    Ok(())
}

pub fn update_command_to_test_mutant(
    command: &str,
    fullpath: &PathBuf,
    db_path: PathBuf,
    run_id: i64,
    ) -> Result<(), MutationError>{ 

    let connection = Connection::open(db_path.clone())?;
    let fullpath = fullpath.strip_prefix("./").unwrap_or(fullpath);

    let sql_command = "UPDATE mutants
        SET command_to_test = ?
        WHERE run_id = ? AND 
        file_name = ?";

    let params = params![command, run_id, fullpath.to_str()];
    update_mutants_table(&connection, sql_command, params)?;
    Ok(())
}

pub fn update_status_mutant(killed: bool,
    fullpath: &PathBuf,
    db_path: Option<PathBuf>,
    run_id: i64,
) -> Result<(), MutationError>{

    let db_path = db_path.ok_or(MutationError::MissingDbPath)?;
    let connection = Connection::open(db_path.clone())?;
    let fullpath = fullpath.strip_prefix("./").unwrap_or(fullpath);

    let sql_command = 
    "UPDATE mutants
        SET status = ?
        WHERE run_id = ? AND 
        file_name = ?";
        
    //status
    if killed {
        println!("SQLite option: Updating mutant {} on {} status changed to killed",
            fullpath.display(),
            db_path.clone().display());

        let params = params!["killed", run_id, fullpath.to_str()];
        update_mutants_table(&connection, sql_command, params)?;

    } else if !killed {
        println!("SQLite option: Updating mutant {} on {} status changed to survived",
            fullpath.display(),
            db_path.clone().display());

        let params = params!["survived", run_id, fullpath.to_str()];
        update_mutants_table(&connection, sql_command, params)?;

    };
    Ok(())
}

fn get_hash_from_diff(diff: &str) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(diff.as_bytes());
    let result = hasher.finalize();
    let hash_hex = format!("{:x}", result);
    Ok(hash_hex)
}

fn get_file_diff(mainfile: Option<PathBuf>, comparefile: PathBuf) -> Result<String, Box<dyn Error>> {
    let mainfile = mainfile.ok_or("Missing source file to compare with mutant in get_file_diff proccess")?;

    let output = Command::new("diff")
        .arg(&mainfile)
        .arg(&comparefile)
        .output()?;

    println!("Executing diff from files {:?} and  {:?} for storage", mainfile, comparefile);

    if output.status.success() {
        Ok(String::from("Compare files are equal!"))
    } else {
        let diff_result = str::from_utf8(&output.stdout)?;
        Ok(diff_result.to_string())
    }
}

fn get_files_from_folder(filepath: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    println!("filepath get_files_from_folder: {:?}", filepath);

    if !filepath.is_dir() {
        return Err(format!("Current path is not a folder: {:?}", filepath).into());
    }

    let entries = fs::read_dir(filepath)?
        .filter_map(|entry| {
            match entry {
                Ok(e) => {
                    let path = e.path();
                    if path.is_file() {
                        // Remove "original_file.txt" from vec
                        if let Some(name) = path.file_name() {
                            if name != "original_file.txt" {
                                return Some(path);
                            }
                        }
                    }
                    None
                }
                Err(_) => None,
            }
        })
        .collect();

    Ok(entries)
}

fn check_mutation_folder(
    file_to_mutate: &str,
    pr_number: Option<u32>,
    range_lines: Option<(usize, usize)>,
) -> Result<PathBuf> {
    let file_extension = if file_to_mutate.ends_with(".h") {
        ".h"
    } else if file_to_mutate.ends_with(".py") {
        ".py"
    } else {
        ".cpp"
    };

    let file_name = Path::new(file_to_mutate)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| MutationError::InvalidInput("Invalid file path".to_string()))
        .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

    let ext = file_extension.trim_start_matches('.');
    let folder = if let Some(pr) = pr_number {
        format!("muts-pr-{}-{}-{}", pr, file_name, ext)
    } else if let Some(range) = range_lines {
        format!("muts-pr-{}-{}-{}", file_name, range.0, range.1)
    } else {
        format!("muts-{}-{}", file_name, ext)
    };

    Ok(PathBuf::from(folder))
}

pub fn store_mutants(db_path: &PathBuf, run_id: i64, pr_number: Option<u32>, origin_file: Option<PathBuf>, range_lines: Option<(usize, usize)>) -> Result<()> {
    println!("SQLite option: Storing mutants on {}", db_path.display());
    let connection = Connection::open(db_path)?;
    let operator: String = "None".to_string();

    if let Some(file_path) = origin_file.clone() {
        let file_str = file_path.to_string_lossy().to_string();
        let mutation_folder = check_mutation_folder(&file_str, pr_number, range_lines)?;

        let files = get_files_from_folder(&mutation_folder).unwrap_or_default();
        
        for file in &files{
            let diff = get_file_diff(origin_file.clone(), file.into()).unwrap_or_default();
            let patch_hash = get_hash_from_diff(&diff).unwrap_or_default();
            
            let file_path = origin_file.clone().unwrap_or_default().to_string_lossy().into_owned();
            let filename = file.to_str();

            connection.execute("
                INSERT INTO  mutants (run_id , diff, patch_hash, file_path, operator, file_name)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6);
            ", params![run_id, diff, patch_hash, file_path, operator, filename],)?;
        }
    };
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
  
    let commit_hash = match get_commit_hash() {
        Ok(hash) => hash,
        Err(_) => "unknown".to_string(),
    };

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

    let table_columns: Vec<(&str, Vec<&str>)> = vec![
        ("projects", vec!["id", "name", "repository_url"]),
        ("runs", vec!["id", "project_id", "commit_hash", "pr_number", "created_at", "tool_version"]),
        ("mutants", vec![
            "id", "run_id", "diff", "patch_hash", "status", "killed", 
            "command_to_test", "file_path", "operator", "file_name"
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
            file_name           TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES runs(id)
        );

        CREATE INDEX IF NOT EXISTS idx_mutants_run_status ON mutants(run_id, status);
        CREATE INDEX IF NOT EXISTS idx_mutants_file ON mutants(file_path);
        CREATE INDEX IF NOT EXISTS idx_mutants_operator ON mutants(operator);
        CREATE INDEX IF NOT EXISTS idx_mutants_killed ON mutants(killed);

    ")?;

    println!("SQLite option: Ok batch");

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
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;
    use std::fs::{self, File};
    use tempfile::TempPath;
    use tempfile::NamedTempFile;

    fn setup_db() -> (Connection,TempPath) {
        let temp_db = NamedTempFile::new().unwrap();
        let db_path = temp_db.into_temp_path();
        let connection = Connection::open(&db_path).unwrap();

        (connection, db_path)
    }

    #[test]
    #[allow(unused)]
    fn test_db_creation_and_seed() {

        let (connection, db_path) = setup_db();

        println!("connection: {:?} \n path: {:?}", connection, db_path);
        let db_creation_verify = _createdb(&connection);
        assert!(db_creation_verify.is_ok());

        let schema_verify = _check_schema(&connection);
        assert!(schema_verify.is_ok());

        let initial_row_verify = _check_initial_row(&connection);
        assert!(initial_row_verify.is_ok());
    }

    #[test]
    #[allow(unused)]
    fn test_store_run_creates_row() {
        let (connection, db_path) = setup_db();
        _createdb(&connection).unwrap();

        let run_id = store_run(&db_path.to_path_buf(), None).unwrap();
        assert!(run_id > 0, "store_run must return a valid run_id");

        let count: i64 = connection.query_row(
            "SELECT count(*) FROM runs WHERE id=?1",
            [run_id],
            |row| row.get(0)
        ).unwrap();
        assert_eq!(count, 1, "Must exist exactly 1 run");
    }

    #[test]
    #[allow(unused)]
    fn test_store_mutants_inserts_rows() {
        let (connection, db_path) = setup_db();

        let dir = tempdir().unwrap();
        let origin_file = dir.path().join("origin.rs");
        File::create(&origin_file).unwrap();

        let mutation_folder = dir.path().join("muts-origin-rs");
        fs::create_dir_all(&mutation_folder).unwrap();

        let mutant_file = mutation_folder.join("mutant1.rs");
        File::create(&mutant_file).unwrap();

        let run_id = 1;

        let result = store_mutants(&db_path.to_path_buf(), run_id, None, Some(origin_file.clone()), None);
        assert!(result.is_ok());
    }

    #[test]
    #[allow(unused)]
    fn test_update_status_mutant() {
        let (connection, db_path) = setup_db();
        _createdb(&connection).unwrap();

        let dir = tempdir().unwrap();
        let origin_file = dir.path().join("origin.rs");
        let file_path = &origin_file;

        let operator: String = "None".to_string();
        let run_id = 1;

        let origin_file = origin_file.to_str();

        //Seed tables
        connection.execute("
            INSERT INTO runs (id, project_id, commit_hash)
            VALUES (?1, ?2, ?3);
        ", params![1, 1, "hash"]).unwrap();

        connection.execute("
            INSERT INTO  mutants (run_id , diff, patch_hash, file_path, operator, file_name)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6);
        ", params![run_id, "killed diff", "", origin_file, operator, origin_file],).unwrap();

        connection.execute("
            INSERT INTO  mutants (run_id , diff, patch_hash, file_path, operator, file_name)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6);
        ", params![run_id, "survived diff", "", origin_file, operator, origin_file],).unwrap();

        let count: i64 = connection.query_row(
            "SELECT count(*) FROM mutants;",
            [],
            |row| row.get(0)
        ).unwrap();
        println!("count: {:?}", count);
        assert_eq!(count, 2, "Must exist exactly 2 mutants");

        //Test for status killed
        let result = update_status_mutant(true, &file_path, Some(db_path.to_path_buf()), 1);
        assert!(result.is_ok());

        let proj_query_row: (i32, String, String) = connection.query_row(
            "SELECT id, status, diff FROM mutants WHERE run_id=?1 AND id=?2;",
            [1, 1],
            |row| Ok((row.get(0)?, row.get(1)?,row.get(2)?))
        ).unwrap();

        assert!(proj_query_row.0 == 1 && proj_query_row.1 == "killed" && proj_query_row.2 == "killed diff", "Status should've been updated to killed");

        //Test for status survived
        let result = update_status_mutant(false, &file_path, Some(db_path.to_path_buf()), 1);
        assert!(result.is_ok());

        let proj_query_row: (i32, String, String) = connection.query_row(
            "SELECT id, status, diff FROM mutants WHERE run_id=?1 AND id=?2;",
            [1, 2],
            |row| Ok((row.get(0)?, row.get(1)?,row.get(2)?))
        ).unwrap();

        assert!(proj_query_row.0 == 2 && proj_query_row.1 == "survived" && proj_query_row.2 == "survived diff", "Status should've been updated to survived");
    }

    #[test]
    #[allow(unused)]
    fn test_update_command_mutant() {
        let (connection, db_path) = setup_db();
        _createdb(&connection).unwrap();

        let dir = tempdir().unwrap();
        let origin_file = dir.path().join("origin.rs");
        let file_path = &origin_file;

        let operator: String = "None".to_string();
        let run_id = 1;

        let origin_file = origin_file.to_str();

        //Seed tables
        connection.execute("
            INSERT INTO runs (id, project_id, commit_hash)
            VALUES (?1, ?2, ?3);
        ", params![1, 1, "hash"]).unwrap();

        connection.execute("
            INSERT INTO  mutants (run_id , diff, patch_hash, file_path, operator, file_name)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6);
        ", params![run_id, "command diff", "", origin_file, operator, origin_file],).unwrap();

        let count: i64 = connection.query_row(
            "SELECT count(*) FROM mutants;",
            [],
            |row| row.get(0)
        ).unwrap();
        println!("count: {:?}", count);
        assert_eq!(count, 1, "Must exist exactly 1 mutant");

        let result = update_command_to_test_mutant("command", file_path, db_path.to_path_buf(), run_id);
        assert!(result.is_ok());

        let proj_query_row: (i32, String, String) = connection.query_row(
            "SELECT id, diff, command_to_test FROM mutants WHERE run_id=?1 AND id=?2;",
            [1, 1],
            |row| Ok((row.get(0)?, row.get(1)?,row.get(2)?))
        ).unwrap();

        assert!(proj_query_row.0 == 1 && proj_query_row.1 == "command diff" && proj_query_row.2 == "command", "Command should've been updated to command");
    }
}