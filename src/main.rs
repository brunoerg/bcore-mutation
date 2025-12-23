use anyhow::Error;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

mod analyze;
mod ast_analysis;
mod coverage;
mod error;
mod git_changes;
mod mutation;
mod operators;
mod report;
mod sqlite;

use error::{MutationError, Result};

#[derive(Parser)]
#[command(name = "bcore-mutation")]
#[command(about = "Mutation testing tool designed for Bitcoin Core")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create mutants for a specific Bitcoin Core PR or file
    Mutate {
        /// Bitcoin Core's PR number (0 = current branch)
        #[arg(short, long, default_value = "0")]
        pr: u32,

        /// Only create mutants for unit and functional tests
        #[arg(short = 't', long)]
        test_only: bool,

        /// Path for the coverage file (*.info generated with cmake -P build/Coverage.cmake)
        #[arg(short, long)]
        cov: Option<PathBuf>,

        /// Path for the file with lines to skip when creating mutants
        #[arg(long)]
        skip_lines: Option<PathBuf>,

        /// File path to mutate
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Specify a range of lines from a file to be mutated
        #[arg(short, long, num_args = 2)]
        range: Option<Vec<usize>>,

        /// Create only one mutant per line
        #[arg(long)]
        one_mutant: bool,

        /// Apply only security-based mutations (usually to test fuzzing)
        #[arg(short, long)]
        only_security_mutations: bool,

        /// Disable AST-based arid node detection (generate more mutants)
        #[arg(long)]
        disable_ast_filtering: bool,

        /// Add custom expert rule for arid node detection
        #[arg(long, value_name = "PATTERN")]
        add_expert_rule: Option<String>,

        /// Optional path to SQLite database file (default: mutation.db)
        #[arg(long, value_name = "PATH")]
        sqlite: Option<Option<PathBuf>>,
    },
    /// Analyze mutants
    Analyze {
        /// Folder with the mutants
        #[arg(short, long)]
        folder: Option<PathBuf>,

        /// Timeout value per mutant in seconds
        #[arg(short, long, default_value = "1000")]
        timeout: u64,

        /// Number of jobs to be used to compile Bitcoin Core
        #[arg(short, long, default_value = "0")]
        jobs: u32,

        /// Command to test the mutants
        #[arg(short, long)]
        command: Option<String>,

        /// Maximum acceptable survival rate (0.3 = 30%)
        #[arg(long, default_value = "0.75")]
        survival_threshold: f64,

        /// Optional path to SQLite database file (default: mutation.db)
        #[arg(long, value_name = "PATH")]
        sqlite: Option<Option<PathBuf>>,

        /// Run ID stored in SQLite
        #[arg(long)]
        runid: Option<i64>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Mutate {
            pr,
            test_only,
            cov,
            skip_lines,
            file,
            range,
            one_mutant,
            only_security_mutations,
            disable_ast_filtering,
            add_expert_rule,
            sqlite,
        } => {
            let mut run_id: i64 = 0;

            let skip_lines_map = if let Some(path) = skip_lines {
                read_skip_lines(&path)?
            } else {
                HashMap::new()
            };

            let coverage = if let Some(cov_path) = cov {
                Some(coverage::parse_coverage_file(&cov_path)?)
            } else {
                None
            };

            let range_lines = if let Some(range_vec) = range {
                if range_vec.len() != 2 || range_vec[0] > range_vec[1] {
                    return Err(MutationError::InvalidInput("Invalid range".to_string()));
                }
                Some((range_vec[0], range_vec[1]))
            } else {
                None
            };
    
            let db_path = match sqlite {
                Some(Some(path)) => {
                    let mut full_path = PathBuf::from("db");
                    full_path.push(path);
                    Some(full_path)
                }
                Some(None) => Some(PathBuf::from("db/mutation.db")),
                None => None,
            };

            if pr != 0 && file.is_some() {
                return Err(MutationError::InvalidInput(
                    "You should only provide PR number or file".to_string(),
                ));
            }

            if coverage.is_some() && range_lines.is_some() {
                return Err(MutationError::InvalidInput(
                    "You should only provide coverage file or the range of lines to mutate"
                        .to_string(),
                ));
            }

            if let Some(ref expert_rule) = add_expert_rule {
                println!("Custom expert rule will be applied: {}", expert_rule);
            }

            if let Some(ref path) = db_path {
                sqlite::check_db(path).map_err(Error::from)?;
                run_id = sqlite::store_run(path, if pr == 0 { None } else { Some(pr) }).map_err(Error::from)?;
            }

            mutation::run_mutation(
                if pr == 0 { None } else { Some(pr) },
                file.clone(),
                one_mutant,
                only_security_mutations,
                range_lines,
                coverage,
                test_only,
                skip_lines_map,
                !disable_ast_filtering,
                add_expert_rule,
            )
            .await?;

            if let Some(ref path) = db_path {
                sqlite::store_mutants(
                    path,
                    run_id,
                    if pr == 0 { None } else { Some(pr) },
                    file,
                    range_lines).map_err(Error::from)?;
            }

        }
        Commands::Analyze {
            folder,
            timeout,
            jobs,
            command,
            survival_threshold,
            sqlite,
            runid,
        } => {

            let db_path = match sqlite {
                Some(Some(path)) => {
                    let mut full_path = PathBuf::from("db");
                    full_path.push(path);
                    Some(full_path)
                }
                Some(None) => Some(PathBuf::from("db/mutation.db")),
                None => None,
            };

            if runid.is_none() {
                return Err(MutationError::InvalidInput(
                    "--sqlite requires --runid".to_string(),
                ));
            }

            if runid.is_some() && db_path.is_none() {
                return Err(MutationError::InvalidInput(
                    "--runid requires --sqlite".to_string(),
                ));
            }

            analyze::run_analysis(folder, command, jobs, timeout, survival_threshold, db_path, runid).await?;
        }
    }

    Ok(())
}

fn read_skip_lines(path: &PathBuf) -> Result<HashMap<String, Vec<usize>>> {
    let content = std::fs::read_to_string(path)?;
    let map: HashMap<String, Vec<usize>> = serde_json::from_str(&content)?;
    Ok(map)
}
