use thiserror::Error;

#[derive(Error, Debug)]
pub enum MutationError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Git command failed: {0}")]
    Git(String),

    #[error("Command execution failed: {0}")]
    Command(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Coverage parsing error: {0}")]
    Coverage(String),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("Walkdir error: {0}")]
    Walkdir(#[from] walkdir::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Db path error")]
    MissingDbPath,

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, MutationError>;
