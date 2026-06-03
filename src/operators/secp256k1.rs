//! secp256k1 operator set.
//!
//! secp256k1 is a pure C library with no Python functional tests and no Boost
//! unit tests — its tests are C programs (`tests.c`, `tests_exhaustive.c`,
//! `*_impl.h` test sections) driven by CTest. The operators here therefore
//! reuse the language-level [`super::common`] set and add C-specific guard
//! macros (`VERIFY_CHECK`, `ARG_CHECK`, `CHECK`, …) to the skip lists.
//!
//! These project-specific entries are a starting point and are expected to be
//! refined as the tool is exercised against secp256k1.

use super::{build, common, MutationOperator, OperatorSet};

pub struct Secp256k1;

impl OperatorSet for Secp256k1 {
    fn regex_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
        build(common::regex_operators())
    }

    fn security_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
        build(common::security_operators())
    }

    fn test_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
        build(common::test_operators())
    }

    fn do_not_mutate_patterns(&self) -> Vec<&'static str> {
        let mut patterns = common::do_not_mutate_patterns();
        // secp256k1 invariant/argument guards: mutating these produces
        // unproductive or always-aborting mutants.
        patterns.extend([
            "VERIFY_CHECK",
            "VERIFY_SETUP",
            "ARG_CHECK",
            "ARG_CHECK_VOID",
            "CHECK",
            "secp256k1_fe_verify",
            "secp256k1_ge_verify",
            "secp256k1_gej_verify",
            "secp256k1_scalar_verify",
        ]);
        patterns
    }

    fn do_not_mutate_py_patterns(&self) -> Vec<&'static str> {
        // secp256k1 has no Python functional test suite.
        Vec::new()
    }

    fn do_not_mutate_unit_patterns(&self) -> Vec<&'static str> {
        vec![
            "while",
            "for",
            "if",
            "else",
            "return",
            "continue",
            "break",
            "static",
            "void",
            // secp256k1 test harness helpers
            "CHECK",
            "VERIFY_CHECK",
            "run_",
            "test_",
            "secp256k1_",
        ]
    }

    fn skip_if_contain_patterns(&self) -> Vec<&'static str> {
        vec!["VERIFY_CHECK", "ARG_CHECK"]
    }

    fn test_line_skip_prefixes(&self) -> Vec<&'static str> {
        vec!["assert", "CHECK", "VERIFY_CHECK", "run_", "test_"]
    }
}
