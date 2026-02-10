//! # Mutation Core
//!
//! A mutation testing tool for Bitcoin Core written in Rust.
//!
//! This library provides functionality to:
//! - Generate mutants for Bitcoin Core source code
//! - Analyze mutants by running tests against them
//! - Generate detailed reports of surviving mutants
//! - AST-based arid node detection to filter unproductive mutants
//!
//! ## Example
//!
//! ```rust,no_run
//! use bcore_mutation::mutation;
//! use std::collections::HashMap;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Generate mutants for a specific file with AST filtering
//!     mutation::run_mutation(
//!         None,                    // PR number
//!         Some("src/test.cpp".into()), // file path
//!         false,                   // one_mutant
//!         false,                   // only_security_mutations
//!         None,                    // range_lines
//!         None,                    // coverage
//!         false,                   // test_only
//!         HashMap::new(),          // skip_lines
//!         true,                    // enable_ast_filtering
//!         None,                    // custom_expert_rule
//!     ).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod sqlite;
pub mod analyze;
pub mod ast_analysis;
pub mod coverage;
pub mod error;
pub mod git_changes;
pub mod mutation;
pub mod operators;
pub mod report;

pub use error::{MutationError, Result};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::analyze::run_analysis;
    pub use crate::ast_analysis::{AridNodeDetector, AstNode, AstNodeType};
    pub use crate::coverage::parse_coverage_file;
    pub use crate::error::{MutationError, Result};
    pub use crate::mutation::run_mutation;
}
