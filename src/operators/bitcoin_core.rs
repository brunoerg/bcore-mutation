//! Bitcoin Core operator set.
//!
//! These lists are kept verbatim from the original `operators.rs` so the
//! refactor introduces no behavioural change for Bitcoin Core. The operator
//! order is intentionally tuned (boundary mutations first, etc.). Over time the
//! generic entries here could be migrated onto [`super::common`], but only with
//! care to preserve ordering.

use super::{build, MutationOperator, OperatorSet};

pub struct BitcoinCore;

impl OperatorSet for BitcoinCore {
    fn regex_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
        let operators = vec![
            (r"--(\b\w+\b)", r"++$1"),
            (r"(\b\w+\b)--", r"$1++"),
            //(r"CAmount\s+(\w+)\s*=\s*([0-9]+)", r"CAmount $1 = $2 + 1"),
            //(r"CAmount\s+(\w+)\s*=\s*([0-9]+)", r"CAmount $1 = $2 - 1"),
            ("Misbehaving", "//Misbehaving"),
            ("continue", "break"),
            ("break", "continue"),
            ("std::all_of", "std::any_of"),
            ("std::any_of", "std::all_of"),
            ("std::min", "std::max"),
            ("std::max", "std::min"),
            ("std::begin", "std::end"),
            ("std::end", "std::begin"),
            // Block-time source swap: `GetBlockTime()` -> `GetMedianTimePast()`.
            // Both live on CBlockIndex and return int64_t, but MTP lags the
            // header timestamp, so locktime/timeout checks that pick the
            // wrong clock only get caught by tests around that boundary.
            (r"\bGetBlockTime\(\)", "GetMedianTimePast()"),
            ("true", "false"),
            ("false", "true"),
            // Designated-initializer / member-assignment value mutation: for
            // any `.field = expr,` or `.field = expr;` (struct init lists,
            // plain member assignment), force the assigned value. Catches
            // cases like `.m_preferred = state->fPreferredDownload,` ->
            // `.m_preferred = true,` that the literal-only true/false swap
            // above can't reach because the RHS isn't itself the literal
            // `true`/`false`.
            (r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}true${3}"),
            (r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}false${3}"),
            // Integer type narrowing: `int64_t` -> `uint32_t`. Silently
            // truncates values above 2^32 and turns negatives into large
            // positives, so a mutant only dies when a test actually exercises
            // the wide or signed range. Word boundaries keep `uint64_t`
            // untouched. Mirrors the entry in `common`.
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
            (r".*\berase\(.+", ""),
            (
                r"^\s*[a-zA-Z_]\w*(?:::[a-zA-Z_]\w*)*(?:(?:->|\.)[a-zA-Z_]\w*)*\s*\([^;]*\)\s*;$",
                "",
            ),
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

        build(operators)
    }

    fn security_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
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

        build(operators)
    }

    fn test_operators(&self) -> Result<Vec<MutationOperator>, regex::Error> {
        // Instead of using negative lookahead, we'll use a simpler approach
        // This will match function calls but we'll filter out assert functions in the application logic
        let operators = vec![
            (r"^\s*(?:\w+(?:\.|->|::))*(\w+)\s*\([^)]*\)\s*;?\s*$", ""), // Function calls (will be filtered by skip logic)
        ];

        build(operators)
    }

    fn do_not_mutate_patterns(&self) -> Vec<&'static str> {
        vec![
            "/",
            "//",
            "#",
            "*",
            "assert",
            "Assert",
            "LOCK",
            "self.log",
            "Assume",
            "CHECK_NONFATAL",
            "/*",
            "LogPrintf",
            "LogPrint",
            "LogDebug",
            "LogInfo",
            "strprintf",
            "G_FUZZING",
            // no-op for FindAndDelete
            "if (nFound > 0)",
        ]
    }

    fn do_not_mutate_py_patterns(&self) -> Vec<&'static str> {
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

    fn do_not_mutate_unit_patterns(&self) -> Vec<&'static str> {
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

    fn skip_if_contain_patterns(&self) -> Vec<&'static str> {
        vec!["EnableFuzzDeterminism", "nLostUnk", "RPCArg::Type::"]
    }

    fn test_line_skip_prefixes(&self) -> Vec<&'static str> {
        vec![
            "assert",
            "BOOST_",
            "EXPECT_",
            "ASSERT_",
            "CHECK_",
            "REQUIRE_",
            "wait_for",
            "wait_until",
            "send_and_ping",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member_assign_to_true_op() -> MutationOperator {
        MutationOperator::new(r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}true${3}").unwrap()
    }

    fn member_assign_to_false_op() -> MutationOperator {
        MutationOperator::new(r"(\.\w+\s*=\s*)([^,;=][^,;]*?)(\s*[,;])", r"${1}false${3}")
            .unwrap()
    }

    #[test]
    fn test_designated_initializer_value_is_mutated() {
        let op = member_assign_to_true_op();
        let line = "    .m_preferred = state->fPreferredDownload,";
        assert!(op.pattern.is_match(line));
        let mutated = op.pattern.replace(line, &op.replacement);
        assert_eq!(mutated, "    .m_preferred = true,");
    }

    #[test]
    fn test_designated_initializer_value_is_mutated_to_false() {
        let op = member_assign_to_false_op();
        let line = "    .m_preferred = state->fPreferredDownload,";
        let mutated = op.pattern.replace(line, &op.replacement);
        assert_eq!(mutated, "    .m_preferred = false,");
    }

    #[test]
    fn test_plain_member_assignment_is_mutated() {
        let op = member_assign_to_true_op();
        let line = "peer.m_wtxid_relay = ComputeWtxidRelay();";
        assert!(op.pattern.is_match(line));
        let mutated = op.pattern.replace(line, &op.replacement);
        assert_eq!(mutated, "peer.m_wtxid_relay = true;");
    }

    #[test]
    fn test_member_assignment_pattern_ignores_comparisons() {
        let op = member_assign_to_true_op();
        assert!(!op.pattern.is_match("if (state.m_count == 5) {"));
        assert!(!op.pattern.is_match("if (a.count >= b.count) {"));
        // Would previously match, treating `==` as `=` and corrupting the
        // comparison into an assignment (`state.m_count =true;`).
        assert!(!op.pattern.is_match("bool ok = state.m_count == 5;"));
    }

    fn find_op(needle: &str) -> MutationOperator {
        BitcoinCore
            .regex_operators()
            .unwrap()
            .into_iter()
            .find(|op| op.pattern.as_str().contains(needle))
            .unwrap_or_else(|| panic!("operator containing {needle:?} present"))
    }

    #[test]
    fn test_get_block_time_becomes_median_time_past() {
        let op = find_op("GetBlockTime");
        let line = "    if (pindex->GetBlockTime() < nLockTime) return false;";
        assert!(op.pattern.is_match(line));
        assert_eq!(
            op.pattern.replace(line, &op.replacement),
            "    if (pindex->GetMedianTimePast() < nLockTime) return false;"
        );
        assert_eq!(
            op.pattern
                .replace("int64_t t = tip.GetBlockTime();", &op.replacement),
            "int64_t t = tip.GetMedianTimePast();"
        );
    }

    #[test]
    fn test_get_block_time_swap_ignores_lookalikes() {
        let op = find_op("GetBlockTime");
        assert!(!op.pattern.is_match("    pindex->GetBlockTimeMax();"));
        assert!(!op.pattern.is_match("    block.SetBlockTime(now);"));
        assert!(!op.pattern.is_match("    int64_t GetBlockTime(int height);"));
    }

    #[test]
    fn test_int64_narrowing_is_present_in_bitcoin_core_set() {
        let op = find_op("int64_t");
        assert_eq!(
            op.pattern
                .replace("    int64_t nTime = GetTime();", &op.replacement),
            "    uint32_t nTime = GetTime();"
        );
        assert!(!op.pattern.is_match("    uint64_t x = 0;"));
    }
}
