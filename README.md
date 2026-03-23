# bcore-mutation

**A mutation testing tool for Bitcoin Core**

> "Mutation testing involves modifying a program in small ways. Each mutated version is called a mutant, and tests detect and reject mutants by causing the behaviour of the original version to differ from the mutant. This is called _killing_ the mutant. Test suites are measured by the percentage of mutants that they kill." — Wikipedia

## Features

- Generate mutants only for code touched in a specific PR or branch
- Security-based mutation operators for testing fuzzing scenarios
- Skip useless mutants (comments, `LogPrintf` statements, etc.)
- One-mutant-per-line mode for faster analysis
- Coverage-guided mutation (only mutate covered lines)
- AST-based arid node filtering to reduce noise
- Persist results and resume analysis with a SQLite database

---

## Installation

### From source

```bash
git clone <repository>
cd bcore-mutation
cargo build --release
cargo install --path .
```

### From crates.io

```bash
cargo install bcore-mutation
```

---

## Workflow

1. **Mutate** — generate mutants and store them in a SQLite database.
2. **Analyze** — run your test command against each mutant and report survivors.

---

## `mutate` command

Generates mutants for the target code and optionally persists them to a SQLite database.

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--sqlite [PATH]` | | `mutation.db` | Persist mutants to a SQLite database. Accepts an optional custom path. |
| `--file PATH` | `-f` | | File to mutate. Mutually exclusive with `--pr`. |
| `--pr NUMBER` | `-p` | `0` (current branch) | Bitcoin Core PR number to mutate. Mutually exclusive with `--file`. |
| `--range START END` | `-r` | | Restrict mutation to a line range within the target file. Cannot be combined with `--cov`. |
| `--cov PATH` | `-c` | | Path to a coverage file (`*.info` generated with `cmake -P build/Coverage.cmake`). Only lines covered by tests will be mutated. Cannot be combined with `--range`. |
| `--skip-lines PATH` | | | Path to a JSON file listing lines to skip per file (see format below). |
| `--one-mutant` | | | Create only one mutant per line (prioritises harder-to-kill operators). Useful for large files. |
| `--test-only` | `-t` | | Only create mutants inside unit and functional test files. |
| `--only-security-mutations` | `-s` | | Apply only security-focused mutation operators. Useful when evaluating fuzzing coverage. |
| `--disable-ast-filtering` | | | Disable AST-based arid node detection. Generates more mutants, including potentially redundant ones. |
| `--add-expert-rule PATTERN` | | | Add a custom pattern for arid node detection (see AST filtering below). |

### Examples

**Mutate a specific file:**
```bash
bcore-mutation mutate --sqlite -f src/wallet/wallet.cpp
```

**Mutate all files changed in a PR:**
```bash
bcore-mutation mutate --sqlite -p 12345
```

**Restrict to a line range:**
```bash
bcore-mutation mutate --sqlite -f src/wallet/wallet.cpp --range 10 50
```

**Use a coverage file (only mutate covered lines):**
```bash
bcore-mutation mutate --sqlite -f src/wallet/wallet.cpp -c total_coverage.info
```

**One mutant per line (faster analysis):**
```bash
bcore-mutation mutate --sqlite -p 12345 --one-mutant
```

**Mutate only test files:**
```bash
bcore-mutation mutate --sqlite -p 12345 --test-only
```

**Security-only mutations (for fuzzing):**
```bash
bcore-mutation mutate --sqlite -f src/wallet/wallet.cpp --only-security-mutations
```

**Skip specific lines:**
```bash
bcore-mutation mutate --sqlite -p 12345 --skip-lines skip.json
```

### Skip lines file format

Create a JSON file that maps file paths to line numbers to skip:

```json
{
  "src/wallet/wallet.cpp": [1, 2, 3],
  "src/validation.cpp": [10, 121, 8]
}
```

---

## `analyze` command

Applies each mutant to the source tree, runs the test command, and reports whether the mutant was killed or survived.

When `--sqlite` is used, the `mutate` command prints a `run_id` that you pass to `analyze` with `--run-id`.

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--sqlite [PATH]` | | `mutation.db` | SQLite database to read mutants from. Requires `--run-id`. Accepts an optional custom path. |
| `--run-id ID` | | | Run ID returned by the `mutate` command. Requires `--sqlite`. |
| `--command CMD` | `-c` | | Shell command used to test each mutant (e.g. a build + test invocation). Required when using `--run-id`. |
| `--file-path PATH` | | | Only analyze mutants that belong to this file. Requires `--run-id`. |
| `--folder PATH` | `-f` | | Folder containing mutants (alternative to `--sqlite` / `--run-id`). |
| `--timeout SECONDS` | `-t` | `300` | Timeout in seconds for each mutant's test run. |
| `--jobs N` | `-j` | `0` | Number of parallel jobs passed to the compiler (e.g. `make -j N`). `0` uses the system default. |
| `--survival-threshold RATE` | | `0.75` | Maximum acceptable mutant survival rate (e.g. `0.3` = 30%). The run exits with an error if the threshold is exceeded. |
| `--surviving` | | | Only analyze mutants that survived a previous run. Requires `--run-id`. |

### Examples

**Basic analysis:**
```bash
bcore-mutation analyze --sqlite --run-id=1 -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

**Per-file commands (useful when a PR spans multiple modules):**
```bash
bcore-mutation analyze --sqlite --run-id=1 --file-path="src/wallet/coinselection.cpp" \
  -c "cmake --build build && ./build/test/functional/wallet_test.py"

bcore-mutation analyze --sqlite --run-id=1 --file-path="src/net_processing.cpp" \
  -c "cmake --build build && ./build/test/functional/p2p_test.py"
```

**Retry only survivors from a previous run:**
```bash
bcore-mutation analyze --sqlite --run-id=1 --surviving \
  -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

**Set a custom timeout and job count:**
```bash
bcore-mutation analyze --sqlite --run-id=1 -t 120 -j 8 \
  -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

**Set a survival rate threshold:**
```bash
bcore-mutation analyze --sqlite --run-id=1 --survival-threshold=0.2 \
  -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

---

## Testing

```bash
cargo test
```

## Contributing

1. Fork the repository.
2. Create a feature branch.
3. Add tests for your changes.
4. Ensure all tests pass: `cargo test`
5. Run the linter: `cargo clippy -- -D warnings`
6. Format code: `cargo fmt`
7. Submit a pull request.

## License

MIT License
