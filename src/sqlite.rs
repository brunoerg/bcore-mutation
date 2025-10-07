use rusqlite::{Connection, Result};

pub fn store_mutants() -> Result<()> {
    let connection = Connection::open("db/mutants.db")?;
    
    connection.execute(
        "
        PRAGMA foreign_keys = ON;
        "
    , [])?;

    println!("exec 1");
    connection.execute(
        "
        -- Projects
        CREATE TABLE IF NOT EXISTS projects (
        id              INTEGER PRIMARY KEY,
        name            TEXT NOT NULL,
        repository_url  TEXT,
        UNIQUE(name),
        UNIQUE(repository_url)
        );
        "
    , [])?;

    println!("exec 2");


    //TOCONTINUE Creating tables
    connection.execute(
        "
        -- Projects
        CREATE TABLE IF NOT EXISTS runs (
        id              INTEGER PRIMARY KEY
        );
        "
    , [])?;

    println!("exec 3");


    Ok(())
}
