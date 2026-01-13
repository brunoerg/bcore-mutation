use regex::Regex;
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

pub fn get_regex_operators() -> Result<Vec<MutationOperator>, regex::Error> {
    let operators = vec![
        (r"--(\b\w+\b)", r"++$1"),
        (r"(\b\w+\b)--", r"$1++"),
        //(r"CAmount\s+(\w+)\s*=\s*([0-9]+)", r"CAmount $1 = $2 + 1"),
        //(r"CAmount\s+(\w+)\s*=\s*([0-9]+)", r"CAmount $1 = $2 - 1"),
        ("continue", "break"),
        ("break", "continue"),
        ("std::all_of", "std::any_of"),
        ("std::any_of", "std::all_of"),
        ("std::min", "std::max"),
        ("std::max", "std::min"),
        ("std::begin", "std::end"),
        ("std::end", "std::begin"),
        ("true", "false"),
        ("false", "true"),
        (r" / ", " * "),
        (r" > ", " < "),
        (r" > ", " >= "),
        (r" > ", " <= "),
        (r" < ", " > "),
        (r" < ", " <= "),
        (r" < ", " >= "),
        (r" >= ", " <= "),
        (r" >= ", " > "),
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
        (r".*\berase\(.+", ""),
        (r"^.*if\s*\(.*\)\s*continue;.*$", ""),
        (r"^.*if\s*\(.*\)\s*return;.*$", ""),
        (r"^.*if\s*\(.*\)\s*return.*;.*$", ""),
        (r"^(.*for\s*\(.*;.*;.*\)\s*\{.*)$", r"$1break;"),
        (r"^(.*while\s*\(.*\)\s*\{.*)$", r"$1break;"),
        /* Seems they're unproductive
        (
            r"\b(int64_t|uint64_t|int32_t|uint32_t)\s+(\w+)\s*=\s*(.*?);$",
            r"$1 $2 = ($3) + 1;",
        ),
        (
            r"\b(int64_t|uint64_t|int32_t|uint32_t)\s+(\w+)\s*=\s*(.*?);$",
            r"$1 $2 = ($3) - 1;",
        ),
        (
            r"static\s+const\s+size_t\s+(\w+)\s*=\s*([^;]+);",
            r"static const size_t $1 = $2 - 1;",
        ),
        (
            r"static\s+const\s+size_t\s+(\w+)\s*=\s*([^;]+);",
            r"static const size_t $1 = $2 + 1;",
        ),
        //(r"NodeClock::now\(\)", r"NodeClock::now() - 1"),
        //(r"NodeClock::now\(\)", r"NodeClock::now() + 1"),*/
    ];

    operators
        .into_iter()
        .map(|(pattern, replacement)| MutationOperator::new(pattern, replacement))
        .collect()
}

pub fn get_security_operators() -> Result<Vec<MutationOperator>, regex::Error> {
    let operators = vec![
        ("==", "="),
        (r" - ", " + "),
        (r"\s\+\s", "-"),
        (
            r"std::array<\s*([\w:]+)\s*,\s*(\d+)\s*>",
            r"std::array<$1, $2 - 2>",
        ),
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
        (
            r"GetSelectionAmount\(\)",
            r"GetSelectionAmount() + std::numeric_limits<CAmount>::max() - 1",
        ),
        (r"resetBlock\(\);", ""),
        (
            r"\w+(\.|->)GetMedianTimePast\(\)",
            "std::numeric_limits<int64_t>::max()",
        ),
        ("break", ""),
    ];

    operators
        .into_iter()
        .map(|(pattern, replacement)| MutationOperator::new(pattern, replacement))
        .collect()
}

pub fn get_test_operators() -> Result<Vec<MutationOperator>, regex::Error> {
    // Instead of using negative lookahead, we'll use a simpler approach
    // This will match function calls but we'll filter out assert functions in the application logic
    let operators = vec![
        (r"^\s*(?:\w+(?:\.|->|::))*(\w+)\s*\([^)]*\)\s*;?\s*$", ""), // Function calls (will be filtered by skip logic)
    ];

    operators
        .into_iter()
        .map(|(pattern, replacement)| MutationOperator::new(pattern, replacement))
        .collect()
}

pub fn get_do_not_mutate_patterns() -> Vec<&'static str> {
    vec![
        "/",
        "//",
        "#",
        "*",
        "assert",
        "self.log",
        "Assume",
        "CHECK_NONFATAL",
        "/*",
        "LogPrintf",
        "LogPrint",
        "LogDebug",
        "strprintf",
        "G_FUZZING",
    ]
}

pub fn get_do_not_mutate_py_patterns() -> Vec<&'static str> {
    vec![
        "wait_for",
        "wait_until",
        "check_",
        "for",
        "expected_error",
        "def",
        "send_and_ping",
        "test_",
        "rehash",
        "start_",
        "solve()",
        "restart_",
        "stop_",
        "connect_",
        "sync_",
        "class",
        "return",
        "generate(",
        "continue",
        "sleep",
        "break",
        "getcontext().prec",
        "if",
        "else",
        "assert",
    ]
}

pub fn get_do_not_mutate_unit_patterns() -> Vec<&'static str> {
    vec![
        "while",
        "for",
        "if",
        "test_",
        "_test",
        "reset",
        "class",
        "return",
        "continue",
        "break",
        "else",
        "reserve",
        "resize",
        "static",
        "void",
        "BOOST_",
        "LOCK(",
        "LOCK2(",
        "Test",
        "Assert",
        "EXCLUSIVE_LOCKS_REQUIRED",
        "catch",
    ]
}

pub fn get_skip_if_contain_patterns() -> Vec<&'static str> {
    vec!["EnableFuzzDeterminism", "nLostUnk", "RPCArg::Type::"]
}

// Helper function to check if a line should be mutated by test operators
// This replaces the negative lookahead functionality
pub fn should_mutate_test_line(line: &str) -> bool {
    let line_trimmed = line.trim();

    // Don't mutate lines that start with assert or other test-specific patterns
    let skip_patterns = vec![
        "assert",
        "BOOST_",
        "EXPECT_",
        "ASSERT_",
        "CHECK_",
        "REQUIRE_",
        "wait_for",
        "wait_until",
        "send_and_ping",
    ];

    for pattern in skip_patterns {
        if line_trimmed.starts_with(pattern) {
            return false;
        }
    }

    // Only mutate if it looks like a function call
    let function_call_pattern =
        Regex::new(r"^\s*(?:\w+(?:\.|->|::))*(\w+)\s*\([^)]*\)\s*;?\s*$").unwrap();
    function_call_pattern.is_match(line)
}
