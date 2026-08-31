//! secp256k1 operator set.
//!
//! secp256k1 is a pure C library with no Python functional tests and no Boost
//! unit tests - its tests are C programs (`tests.c`, `tests_exhaustive.c`,
//! `*_impl.h` test sections) driven by CTest. The operators here therefore
//! reuse the language-level [`super::common`] set and add C-specific guard
//! macros, annotations, cleanup calls, and diagnostics to the skip lists.

use super::{build, common, MutationOperator, OperatorSet};

pub struct Secp256k1;

const SECP256K1_SKIP_PREFIXES: &[&str] = &[
    // Invariant and public API guards.
    "VERIFY_CHECK",
    "VERIFY_BITS",
    "ARG_CHECK",
    "ARG_CHECK_VOID",
    "CHECK",
    "SECP256K1_ARG_NONNULL",
    "secp256k1_fe_verify",
    "secp256k1_ge_verify",
    "secp256k1_gej_verify",
    "secp256k1_scalar_verify",
    // Defensive output zeroing and testrand diagnostics.
    "memset",
    "printf",
    "fprintf",
];

const SECP256K1_SKIP_SUBSTRINGS: &[&str] = &[
    // VERIFY-style macro variants such as SECP256K1_FE_VERIFY(...).
    "VERIFY_CHECK",
    "VERIFY_BITS",
    "_VERIFY",
    "ARG_CHECK",
    // Callback/error reporting and verification annotations.
    "secp256k1_callback_call",
    "secp256k1_declassify",
    "SECP256K1_CHECKMEM_",
    // Secret/object cleanup functions.
    "secp256k1_memclear_explicit",
    "secp256k1_scalar_clear",
    "secp256k1_ge_clear",
    "secp256k1_gej_clear",
    "secp256k1_fe_clear",
    "secp256k1_sha256_clear",
    "secp256k1_hmac_sha256_clear",
    "secp256k1_rfc6979_hmac_sha256_clear",
    // Conditional zeroing on failure.
    "secp256k1_memczero",
    // Field normalization. These only canonicalize the internal limb
    // representation without changing the represented value, so deleting a
    // call almost always yields an equivalent (non-useful) mutant. Covers
    // secp256k1_fe_normalize{,_weak,_var,_to_zero,_to_zero_var}.
    "secp256k1_fe_normalize",
    // testrand /dev/urandom file I/O.
    "fopen(",
    "fread(",
    "fclose(",
];

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
        patterns.extend(SECP256K1_SKIP_PREFIXES);
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
        SECP256K1_SKIP_SUBSTRINGS.to_vec()
    }

    fn test_line_skip_prefixes(&self) -> Vec<&'static str> {
        let mut prefixes = vec!["assert", "CHECK", "run_", "test_"];
        prefixes.extend(SECP256K1_SKIP_PREFIXES);
        prefixes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skipped_by_global_secp256k1_lists(line: &str) -> bool {
        let ops = Secp256k1;
        let trimmed = line.trim_start();

        ops.do_not_mutate_patterns()
            .iter()
            .any(|pattern| trimmed.starts_with(pattern))
            || ops
                .skip_if_contain_patterns()
                .iter()
                .any(|pattern| line.contains(pattern))
    }

    #[test]
    fn skips_verify_and_api_guard_lines() {
        assert!(skipped_by_global_secp256k1_lists(
            "    VERIFY_CHECK(r != NULL);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    VERIFY_BITS_128(x, 64);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    SECP256K1_SCALAR_VERIFY (&s);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    ARG_CHECK(ctx != NULL);"
        ));
    }

    #[test]
    fn skips_annotations_cleanup_and_zeroing() {
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_callback_call(&ctx->error_callback, \"bad\");"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_declassify(ctx, &ret, sizeof(ret));"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    SECP256K1_CHECKMEM_CHECK(p, len);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_scalar_clear(&s);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_memczero(sig64, 64, !ret);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    memset(sig64, 0, 64);"
        ));
    }

    #[test]
    fn skips_field_normalization_calls() {
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_fe_normalize(&r->x);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_fe_normalize_weak(&r->x);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_fe_normalize_var(&r->x);"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    secp256k1_fe_normalize_to_zero_var(&t);"
        ));
    }

    #[test]
    fn skips_preprocessor_comments_and_testrand_diagnostics() {
        assert!(skipped_by_global_secp256k1_lists("# ifdef VERIFY"));
        assert!(skipped_by_global_secp256k1_lists(
            "    fprintf(stderr, \"random seed failure\\n\");"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    fp = fopen(\"/dev/urandom\", \"rb\");"
        ));
        assert!(skipped_by_global_secp256k1_lists(
            "    fread(seed, 1, sizeof(seed), fp);"
        ));
        assert!(skipped_by_global_secp256k1_lists("    // comment"));
        assert!(!skipped_by_global_secp256k1_lists("    ret = a + b;"));
    }
}
