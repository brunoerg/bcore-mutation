# Mutation Core (Rust Version)

**A mutation testing tool for Bitcoin Core, rewritten in Rust**.

This is a complete rewrite of the original Python bcore-mutationtool, offering improved performance, better error handling, and enhanced concurrency.

"Mutation testing (or mutation analysis or program mutation) is used to design new software tests and evaluate the quality of existing software tests. Mutation testing involves modifying a program in small ways. Each mutated version is called a mutant and tests detect and reject mutants by causing the behaviour of the original version to differ from the mutant. This is called killing the mutant. Test suites are measured by the percentage of mutants that they kill." (Wikipedia)

## Features

- **High Performance**: Leverages Rust's performance and memory safety
- **Async/Await**: Non-blocking I/O for improved concurrency
- **Better Error Handling**: Comprehensive error types with detailed messages
- **Memory Safe**: No risk of buffer overflows or memory leaks
- **Cross-Platform**: Works on Linux, macOS, and Windows

All original features from the Python version:
- Generate mutants only for code touched in specific branches (useful for testing PRs)
- Security-based mutation operators for testing fuzzing scenarios
- Skip useless mutants (comments, LogPrintf statements, etc.)
- One mutant per line mode for faster analysis
- Support for functional and unit test mutation
- Coverage-guided mutation testing
- Specific mutation operators designed for Bitcoin Core

## Installation

### From Source

```bash
git clone <repository>
cd bcore-mutation
cargo build --release
cargo install --path .
```

## Usage

### Basic Usage

```bash
cd bitcoin
git checkout branch # if needed
bcore-mutationmutate
bcore-mutationanalyze # use -j=N to set number of compilation jobs
```

### Generate Mutants for Specific File

```bash
bcore-mutationmutate -f src/wallet/wallet.cpp
```

### Generate Mutants for Specific PR

```bash
bcore-mutationmutate -p 12345
```

### Create Skip Lines Configuration

Create a JSON file specifying lines to skip:

```json
{
  "src/wallet/wallet.cpp": [1, 2, 3],
  "src/validation.cpp": [10, 121, 8]
}
```

Use with:

```bash
bcore-mutationmutate -p 12345 --skip-lines skip.json
```

### Advanced Options

Create only one mutant per line (faster analysis):

```bash
bcore-mutationmutate -p 12345 --one-mutant
```

Create mutants only for tests:

```bash
bcore-mutationmutate -p 12345 --test-only
```

Use coverage file to create mutants only for covered code:

```bash
bcore-mutationmutate -f src/wallet/wallet.cpp -c total_coverage.info
```

Specify line range:

```bash
bcore-mutationmutate -f src/wallet/wallet.cpp --range 10 50
```

Security-only mutations (for fuzzing):

```bash
bcore-mutationmutate -f src/wallet/wallet.cpp --only-security-mutations
```

### Analysis

Analyze all mutation folders:

```bash
bcore-mutationanalyze
```

Analyze specific folder with custom command:

```bash
bcore-mutationanalyze -f muts-wallet-cpp -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

Set timeout and parallel jobs:

```bash
bcore-mutationanalyze -j 8 -t 300 --survival-threshold 0.3
```

## Performance Improvements

The Rust version offers several performance improvements over the Python version:

- **Parallel Processing**: Uses Rayon for CPU-intensive operations
- **Async I/O**: Non-blocking file operations and command execution
- **Memory Efficiency**: Lower memory usage and no GIL limitations
- **Faster Regex**: Compiled regex patterns with better performance
- **Zero-Copy Operations**: Efficient string handling where possible

## Command Line Interface

### Mutate Command

```
bcore-mutationmutate [OPTIONS]

Options:
  -p, --pr <PR>                           Bitcoin Core's PR number (0 = current branch) [default: 0]
  -t, --test-only                         Only create mutants for unit and functional tests
  -c, --cov <COV>                        Path for the coverage file (*.info)
      --skip-lines <SKIP_LINES>          Path for the file with lines to skip
  -f, --file <FILE>                      File path to mutate
  -r, --range <RANGE> <RANGE>            Specify a range of lines to be mutated
      --one-mutant                       Create only one mutant per line
  -s, --only-security-mutations          Apply only security-based mutations
```

### Analyze Command

```
bcore-mutationanalyze [OPTIONS]

Options:
  -f, --folder <FOLDER>                  Folder with the mutants
  -t, --timeout <TIMEOUT>                Timeout value per mutant in seconds [default: 1000]
  -j, --jobs <JOBS>                      Number of jobs for Bitcoin Core compilation [default: 0]
  -c, --command <COMMAND>                Command to test the mutants
      --survival-threshold <THRESHOLD>    Maximum acceptable survival rate [default: 0.75]
```

## Library Usage

The tool can also be used as a Rust library:

```rust
use mutation_core::prelude::*;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    // Generate mutants for a file
    run_mutation(
        None,                           // PR number
        Some("src/test.cpp".into()),    // file path
        false,                          // one_mutant
        false,                          // only_security_mutations
        None,                           // range_lines
        None,                           // coverage
        false,                          // test_only
        HashMap::new(),                 // skip_lines
    ).await?;

    // Analyze mutants
    run_analysis(
        Some("muts-test-cpp".into()),   // folder
        None,                           // command
        4,                              // jobs
        1000,                           // timeout
        0.75,                           // survival_threshold
    ).await?;

    Ok(())
}
```

## Error Handling

The Rust version provides comprehensive error handling with detailed error messages:

```rust
use mutation_core::MutationError;

match run_mutation(/* args */).await {
    Ok(_) => println!("Mutation completed successfully"),
    Err(MutationError::Git(msg)) => eprintln!("Git error: {}", msg),
    Err(MutationError::Io(err)) => eprintln!("I/O error: {}", err),
    Err(MutationError::InvalidInput(msg)) => eprintln!("Invalid input: {}", msg),
    Err(err) => eprintln!("Other error: {}", err),
}
```

## Testing

Run the test suite:

```bash
cargo test
```

Run tests with coverage:

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out html
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for your changes
4. Ensure all tests pass: `cargo test`
5. Run clippy: `cargo clippy -- -D warnings`
6. Format code: `cargo fmt`
7. Submit a pull request

## License

MIT License

### Migration Checklist

- [ ] Replace `pip install mutation-core` with `cargo install mutation-core`
- [ ] Update CI/CD scripts to use the new binary
- [ ] Review any custom scripts that parse output (format is mostly the same)
