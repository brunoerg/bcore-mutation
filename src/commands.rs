//! Project-specific build and test commands used by folder-based analysis.
//!
//! When the user does not pass `--command`, `analyze` has to build the project
//! and derive a test command for the mutated file itself. Those commands differ
//! per project (Bitcoin Core uses CMake + `test_bitcoin` + the functional
//! `test_runner.py`; secp256k1 uses CMake + CTest), so each project provides a
//! [`ProjectCommands`] implementation, selected by [`for_project`].
//!
//! This mirrors the [`crate::operators`] module: a trait per concern, one
//! implementation per project, and a single `for_project` selector.

use crate::error::{MutationError, Result};
use crate::project::Project;
use std::path::Path;

/// Build and test commands for a single project.
pub trait ProjectCommands {
    /// Full clean build, run once before analyzing a folder when the user did
    /// not supply an explicit `--command`.
    fn build_command(&self) -> String;

    /// Timeout, in seconds, allowed for [`ProjectCommands::build_command`].
    fn build_timeout_secs(&self) -> u64 {
        3600 // 1 hour
    }

    /// Incremental-build-and-test command for the mutated `target_file_path`.
    /// `jobs` is the parallelism passed to the compiler (0 = system default).
    fn test_command(&self, target_file_path: &str, jobs: u32) -> Result<String>;
}

/// Return the [`ProjectCommands`] for the given project.
pub fn for_project(project: Project) -> Box<dyn ProjectCommands> {
    match project {
        Project::BitcoinCore => Box::new(BitcoinCore),
        Project::Secp256k1 => Box::new(Secp256k1),
    }
}

/// Append ` -j{jobs}` to `build` when `jobs > 0`.
fn with_jobs(mut build: String, jobs: u32) -> String {
    if jobs > 0 {
        build.push_str(&format!(" -j{}", jobs));
    }
    build
}

pub struct BitcoinCore;

impl ProjectCommands for BitcoinCore {
    fn build_command(&self) -> String {
        "rm -rf build && cmake -B build -DENABLE_IPC=OFF && cmake --build build -j $(nproc)"
            .to_string()
    }

    fn test_command(&self, target_file_path: &str, jobs: u32) -> Result<String> {
        let build_command = with_jobs("cmake --build build".to_string(), jobs);

        let command = if target_file_path.contains("functional") {
            format!("./build/{}", target_file_path)
        } else if target_file_path.contains("test") {
            let filename_with_extension = Path::new(target_file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| MutationError::InvalidInput("Invalid file path".to_string()))?;

            let test_to_run = filename_with_extension
                .rsplit('.')
                .nth(1)
                .ok_or_else(|| {
                    MutationError::InvalidInput("Cannot extract test name".to_string())
                })?;

            format!(
                "{} && ./build/bin/test_bitcoin --run_test={}",
                build_command, test_to_run
            )
        } else {
            format!(
                "{} && ctest --output-on-failure --stop-on-failure -C Release && CI_FAILFAST_TEST_LEAVE_DANGLING=1 ./build/test/functional/test_runner.py -F",
                build_command
            )
        };

        Ok(command)
    }
}

pub struct Secp256k1;

impl ProjectCommands for Secp256k1 {
    fn build_command(&self) -> String {
        // secp256k1 builds with CMake (no IPC option). Tests are enabled by
        // default; this is a starting point and may need extra feature flags
        // (e.g. -DSECP256K1_BUILD_EXHAUSTIVE_TESTS) as the tool is exercised.
        "rm -rf build && cmake -B build && cmake --build build -j $(nproc)".to_string()
    }

    fn test_command(&self, _target_file_path: &str, jobs: u32) -> Result<String> {
        // secp256k1 does not map source files to per-file test binaries the way
        // Bitcoin Core does, so we rebuild incrementally and run the whole CTest
        // suite regardless of which file was mutated.
        let build_command = with_jobs("cmake --build build".to_string(), jobs);
        Ok(format!(
            "{} && ctest --test-dir build --output-on-failure",
            build_command
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitcoin_core_test_command() {
        let cmds = BitcoinCore;

        // Functional test
        let cmd = cmds
            .test_command("test/functional/test_example.py", 4)
            .unwrap();
        assert_eq!(cmd, "./build/test/functional/test_example.py");

        // Unit test
        let cmd = cmds.test_command("src/test/test_example.cpp", 0).unwrap();
        assert_eq!(
            cmd,
            "cmake --build build && ./build/bin/test_bitcoin --run_test=test_example"
        );

        // General case
        let cmd = cmds.test_command("src/wallet/wallet.cpp", 2).unwrap();
        assert!(cmd.contains("cmake --build build -j2"));
        assert!(cmd.contains("ctest"));
        assert!(cmd.contains("test_runner.py"));
    }

    #[test]
    fn test_secp256k1_test_command_runs_ctest() {
        let cmds = Secp256k1;
        let cmd = cmds.test_command("src/field_impl.h", 3).unwrap();
        assert!(cmd.contains("cmake --build build -j3"));
        assert!(cmd.contains("ctest --test-dir build"));
        // No Bitcoin Core specifics leak in.
        assert!(!cmd.contains("test_bitcoin"));
        assert!(!cmd.contains("test_runner.py"));
    }

    #[test]
    fn test_build_commands_differ() {
        assert!(BitcoinCore.build_command().contains("-DENABLE_IPC=OFF"));
        assert!(!Secp256k1.build_command().contains("-DENABLE_IPC=OFF"));
    }
}
