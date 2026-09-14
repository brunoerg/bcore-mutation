//! Language-level mutation operators and skip rules shared across C-family
//! projects. These contain no project-specific identifiers (no `LogPrintf`,
//! `BOOST_`, RPC types, etc.), so they are safe to reuse for any C or C++
//! codebase. Project modules compose these with their own additions.
//!
//! Operators are returned as `(pattern, replacement)` pairs and compiled by
//! [`super::build`]. Order is significant: boundary (off-by-one) mutations are
//! listed before direction flips because they are harder to kill.

/// Generic arithmetic, relational, boolean and control-flow operators.
pub(crate) fn regex_operators() -> Vec<(&'static str, &'static str)> {
    vec![
        (r"--(\b\w+\b)", r"++$1"),
        (r"(\b\w+\b)--", r"$1++"),
        ("continue", "break"),
        ("break", "continue"),
        ("true", "false"),
        ("false", "true"),
        // Designated-initializer / member-assignment value mutation: for any
        // `.field = expr,` or `.field = expr;` (struct init lists, plain
        // member assignment), force the assigned value. Catches cases like
        // `.m_preferred = state->fPreferredDownload,` -> `.m_preferred = true,`
        // that the literal-only true/false swap above can't reach because the
        // RHS isn't itself the literal `true`/`false`.
        (r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}true${3}"),
        (r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}false${3}"),
        // Integer type narrowing: `int64_t` -> `uint32_t`. Silently truncates
        // values above 2^32 and turns negatives into large positives, so a
        // mutant only dies when a test actually exercises the wide or signed
        // range. Word boundaries keep `uint64_t` untouched.
        (r"\bint64_t\b", "uint32_t"),
        (r" / ", " * "),
        // Boundary (off-by-one) mutations first — hardest to kill
        (r" >= ", " > "),
        (r" <= ", " < "),
        (r" > ", " >= "),
        (r" < ", " <= "),
        // Direction flips — easier to detect
        (r" >= ", " <= "),
        (r" <= ", " >= "),
        (r" > ", " < "),
        (r" < ", " > "),
        // Cross-boundary
        (r" > ", " <= "),
        (r" < ", " >= "),
        (r"&&", "||"),
        (r"\|\|", "&&"),
        (r" == ", " != "),
        (r" != ", " == "),
        (" - ", " + "),
        (r" \+ ", " - "),
        (r" \+ ", " * "),
        (r" \+ ", " / "),
        (r"\((-?\d+)\)", r"($1 - 1)"),
        (r"\((-?\d+)\)", r"($1 + 1)"),
        (r"\b(if|else\s+if|while)\s*\(([^()]*)\)", r"$1 (1==1)"),
        (r"\b(if|else\s+if|while)\s*\(([^()]*)\)", r"$1 (1==0)"),
        (
            r"^\s*[a-zA-Z_]\w*(?:::[a-zA-Z_]\w*)*(?:(?:->|\.)[a-zA-Z_]\w*)*\s*\([^;]*\)\s*;$",
            "",
        ),
        (r"^.*if\s*\(.*\)\s*continue;.*$", ""),
        (r"^.*if\s*\(.*\)\s*return;.*$", ""),
        (r"^.*if\s*\(.*\)\s*return.*;.*$", ""),
        // Nested early-return deletion: delete a standalone `return ...;`
        // that sits at least two indentation levels deep, i.e. inside an
        // `if`/loop body rather than at function scope. The enclosing loop or
        // function then carries on past the failure handling, which the
        // single-line `if (...) return ...;` deletions above do not cover when
        // the block spans several lines. Function-scope returns are left alone
        // since deleting them mostly yields fall-off-the-end noise.
        (r"^\s{8,}return\b[^;]*;\s*$", ""),
        (r"^(.*for\s*\(.*;.*;.*\)\s*\{.*)$", r"$1break;"),
        (r"^(.*while\s*\(.*\)\s*\{.*)$", r"$1break;"),
    ]
}

/// Generic security/fuzzing-oriented operators (no project-specific symbols).
pub(crate) fn security_operators() -> Vec<(&'static str, &'static str)> {
    vec![
        ("==", "="),
        (r" - ", " + "),
        (r"\s\+\s", "-"),
        (
            r"\b((?:int16_t|uint16_t|int32_t|uint32_t|int64_t|uint64_t|int)\s*[\(\{])([^\)\}]*)[\)\}]",
            "$2",
        ),
        (r"ignore\((\s*(\d+)\s*)\)", r"ignore($2 + 100)"),
        (r"(\w+)\[(\w+)\]", r"$1[$2 + 5]"),
        (
            r"^\s*(?:\(void\)\s*)?[a-zA-Z_][\w:]*\s*\([\w\s,]*\)\s*;\s*$",
            "",
        ),
        (r"if\s*\(\s*(.*?)\s*\|\|\s*(.*?)\s*\)", r"if($2||$1)"),
    ]
}

/// Generic test operator: delete a standalone function call.
pub(crate) fn test_operators() -> Vec<(&'static str, &'static str)> {
    vec![(r"^\s*(?:\w+(?:\.|->|::))*(\w+)\s*\([^)]*\)\s*;?\s*$", "")]
}

/// Comment and assertion prefixes that should never be mutated, regardless of
/// project. Project modules extend this with their own guard macros.
pub(crate) fn do_not_mutate_patterns() -> Vec<&'static str> {
    vec!["/", "//", "#", "*", "/*", "assert"]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested_return_deletion_op() -> regex::Regex {
        let (pattern, replacement) = regex_operators()
            .into_iter()
            .find(|(pattern, _)| pattern.contains("return\\b"))
            .expect("nested return deletion operator present");
        assert_eq!(replacement, "");
        regex::Regex::new(pattern).unwrap()
    }

    #[test]
    fn nested_return_deletion_matches_returns_inside_blocks() {
        let re = nested_return_deletion_op();
        assert!(re.is_match("            return 0;"));
        assert!(re.is_match("        return secp256k1_fe_is_odd(&y);"));
        assert!(re.is_match("\t\t\t\t\t\t\t\treturn false;"));
    }

    #[test]
    fn nested_return_deletion_ignores_function_scope_and_lookalikes() {
        let re = nested_return_deletion_op();
        assert!(!re.is_match("    return ret;"));
        assert!(!re.is_match("return 0;"));
        assert!(!re.is_match("        if (!ret) return 0;"));
        assert!(!re.is_match("        returned = 1;"));
        assert!(!re.is_match("        return_code = f();"));
    }

    fn int64_narrowing_op() -> (regex::Regex, &'static str) {
        let (pattern, replacement) = regex_operators()
            .into_iter()
            .find(|(pattern, _)| pattern.contains("int64_t"))
            .expect("int64_t narrowing operator present");
        (regex::Regex::new(pattern).unwrap(), replacement)
    }

    #[test]
    fn int64_narrowing_rewrites_declarations_and_casts() {
        let (re, rep) = int64_narrowing_op();
        assert_eq!(
            re.replace("    int64_t nTime = GetTime();", rep),
            "    uint32_t nTime = GetTime();"
        );
        assert_eq!(
            re.replace("    return static_cast<int64_t>(x);", rep),
            "    return static_cast<uint32_t>(x);"
        );
        assert_eq!(
            re.replace("std::numeric_limits<int64_t>::max()", rep),
            "std::numeric_limits<uint32_t>::max()"
        );
    }

    #[test]
    fn int64_narrowing_ignores_other_integer_types() {
        let (re, _) = int64_narrowing_op();
        assert!(!re.is_match("    uint64_t x = 0;"));
        assert!(!re.is_match("    int32_t x = 0;"));
        assert!(!re.is_match("    int64_tt x = 0;"));
        assert!(!re.is_match("    my_int64_t x = 0;"));
    }
}
