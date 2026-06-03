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
