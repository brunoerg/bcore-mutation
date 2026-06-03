use regex::Regex;

use crate::project::Project;

mod bitcoin_core;
mod common;
mod secp256k1;

pub use bitcoin_core::BitcoinCore;
pub use secp256k1::Secp256k1;

#[derive(Debug, Clone)]
pub struct MutationOperator {
    pub pattern: Regex,
    pub replacement: String,
}

impl MutationOperator {
    pub fn new(pattern: &str, replacement: &str) -> Result<Self, regex::Error> {
        Ok(MutationOperator {
            pattern: Regex::new(pattern)?,
            replacement: replacement.to_string(),
        })
    }
}

/// Compile a list of `(pattern, replacement)` pairs into mutation operators,
/// preserving order. Used by the per-project [`OperatorSet`] implementations.
pub(crate) fn build(pairs: Vec<(&str, &str)>) -> Result<Vec<MutationOperator>, regex::Error> {
    pairs
        .into_iter()
        .map(|(pattern, replacement)| MutationOperator::new(pattern, replacement))
        .collect()
}

/// The mutation operators and skip rules for a single project.
///
/// Each project (Bitcoin Core, secp256k1, …) provides its own operators and
/// "do not mutate" lists. Implementations typically compose the language-level
/// operators in [`common`] with project-specific additions.
pub trait OperatorSet {
    /// Operators applied to general (non-test) source files.
    fn regex_operators(&self) -> Result<Vec<MutationOperator>, regex::Error>;

    /// Operators applied when `--only-security-mutations` is set.
    fn security_operators(&self) -> Result<Vec<MutationOperator>, regex::Error>;

    /// Operators applied to test files (unit and, where applicable, functional).
    fn test_operators(&self) -> Result<Vec<MutationOperator>, regex::Error>;

    /// Line prefixes that disable mutation of a line entirely.
    fn do_not_mutate_patterns(&self) -> Vec<&'static str>;

    /// Substrings that disable mutation of a Python test line.
    fn do_not_mutate_py_patterns(&self) -> Vec<&'static str>;

    /// Substrings that disable mutation of a (C/C++) unit-test line.
    fn do_not_mutate_unit_patterns(&self) -> Vec<&'static str>;

    /// Substrings that disable mutation of any line when contained.
    fn skip_if_contain_patterns(&self) -> Vec<&'static str>;

    /// Prefixes that mark a test line as not worth mutating (asserts, helpers).
    /// Consumed by the default [`OperatorSet::should_mutate_test_line`].
    fn test_line_skip_prefixes(&self) -> Vec<&'static str>;

    /// Whether a test line should be mutated by [`OperatorSet::test_operators`].
    ///
    /// Default behaviour: skip lines starting with any
    /// [`OperatorSet::test_line_skip_prefixes`], then only mutate lines that
    /// look like a standalone function call.
    fn should_mutate_test_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        for pattern in self.test_line_skip_prefixes() {
            if trimmed.starts_with(pattern) {
                return false;
            }
        }

        // Only mutate if it looks like a function call.
        let function_call_pattern =
            Regex::new(r"^\s*(?:\w+(?:\.|->|::))*(\w+)\s*\([^)]*\)\s*;?\s*$").unwrap();
        function_call_pattern.is_match(line)
    }
}

/// Return the [`OperatorSet`] for the given project.
pub fn for_project(project: Project) -> Box<dyn OperatorSet> {
    match project {
        Project::BitcoinCore => Box::new(BitcoinCore),
        Project::Secp256k1 => Box::new(Secp256k1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generic_call_deletion_op() -> MutationOperator {
        MutationOperator::new(
            r"^\s*[a-zA-Z_]\w*(?:::[a-zA-Z_]\w*)*(?:(?:->|\.)[a-zA-Z_]\w*)*\s*\([^;]*\)\s*;$",
            "",
        )
        .unwrap()
    }

    #[test]
    fn test_generic_call_deletion_matches_free_function() {
        let op = generic_call_deletion_op();
        assert!(op.pattern.is_match("    Foo(arg1, arg2);"));
        assert!(op.pattern.is_match("DoSomething();"));
    }

    #[test]
    fn test_generic_call_deletion_matches_dot_member_call() {
        let op = generic_call_deletion_op();
        assert!(op.pattern.is_match("    obj.Method(arg);"));
        assert!(op.pattern.is_match("obj.Method();"));
    }

    #[test]
    fn test_generic_call_deletion_matches_arrow_member_call() {
        let op = generic_call_deletion_op();
        assert!(op.pattern.is_match("    ptr->Method(arg);"));
        assert!(op.pattern.is_match("ptr->Method();"));
    }

    #[test]
    fn test_generic_call_deletion_matches_namespaced_call() {
        let op = generic_call_deletion_op();
        assert!(op.pattern.is_match("    Namespace::Function(arg);"));
        assert!(op.pattern.is_match("ns::Foo();"));
    }

    #[test]
    fn test_generic_call_deletion_ignores_control_flow_and_keywords() {
        let op = generic_call_deletion_op();
        assert!(!op.pattern.is_match("    if (condition) {"));
        assert!(!op.pattern.is_match("    while (x > 0) {"));
        assert!(!op.pattern.is_match("    for (int i = 0; i < n; i++) {"));
        assert!(!op.pattern.is_match("    switch (value) {"));
        assert!(!op.pattern.is_match("    return Foo();"));
        assert!(!op.pattern.is_match("    delete ptr;"));
        assert!(!op
            .pattern
            .is_match("    throw std::runtime_error(\"err\");"));
    }

    #[test]
    fn test_for_project_returns_distinct_sets() {
        // secp256k1 has no Python functional tests, so its Python skip list is empty;
        // Bitcoin Core's is not. This is a cheap proxy for "the sets differ".
        let btc = for_project(Project::BitcoinCore);
        let secp = for_project(Project::Secp256k1);
        assert!(!btc.do_not_mutate_py_patterns().is_empty());
        assert!(secp.do_not_mutate_py_patterns().is_empty());
    }
}
