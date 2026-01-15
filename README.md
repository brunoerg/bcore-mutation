# bcore-mutation

**A mutation testing tool for Bitcoin Core, rewritten in Rust**.

This is a complete rewrite of the original Python mutation-core tool, offering improved performance, better error handling, and enhanced concurrency.

"Mutation testing (or mutation analysis or program mutation) is used to design new software tests and evaluate the quality of existing software tests. Mutation testing involves modifying a program in small ways. Each mutated version is called a mutant and tests detect and reject mutants by causing the behaviour of the original version to differ from the mutant. This is called killing the mutant. Test suites are measured by the percentage of mutants that they kill." (Wikipedia)


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
bcore-mutation mutate
bcore-mutation analyze # use -j=N to set number of jobs
```

### Generate Mutants for Specific File

```bash
bcore-mutation mutate -f src/wallet/wallet.cpp
```

### Generate Mutants for Specific PR

```bash
bcore-mutation mutate -p 12345
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
bcore-mutation mutate -p 12345 --skip-lines skip.json
```

### Advanced Options

Create only one mutant per line (faster analysis):

```bash
bcore-mutation mutate -p 12345 --one-mutant
```

Create mutants only for tests:

```bash
bcore-mutation mutate -p 12345 --test-only
```

Use coverage file to create mutants only for covered code:

```bash
bcore-mutation mutate -f src/wallet/wallet.cpp -c total_coverage.info
```

Specify line range:

```bash
bcore-mutation mutate -f src/wallet/wallet.cpp --range 10 50
```

Security-only mutations (for fuzzing):

```bash
bcore-mutation mutate -f src/wallet/wallet.cpp --only-security-mutations
```

### Analysis

Analyze all mutation folders:

```bash
bcore-mutation analyze
```

Analyze specific folder with custom command:

```bash
bcore-mutation analyze -f muts-wallet-cpp -c "cmake --build build && ./build/test/functional/wallet_test.py"
```

Set timeout and parallel jobs:

```bash
bcore-mutation analyze -j 8 -t 300 --survival-threshold 0.3
```

### Storage

Performed during the mutants generation (`mutation` command)

Store generated mutants in the `db` folder (create if does not exists).
Default folder: `mutation.db`: 

```bash
bcore-mutation mutate <options> --sqlite <db_name> 
```

### Examples:

For a specific file, using the default database(`mutation.db`):

```bash
bcore-mutation mutate -f src/wallet/wallet.cpp --sqlite 
```

For a specific PR with custom database(`results.db`):

```bash
bcore-mutation mutate -p 12345 --sqlite results.db
```

### Update Storage

Performed during the mutant analysis (`analyze` command)

Perform full analysis for a specific run id (obligatory):

```bash
bcore-mutation analyze --sqlite --runid <run id number>
```

Perform analysis for a specific file:

```bash
bcore-mutation analyze -f <file name> --sqlite --run_id <run id number>
```

Perform analysis for a specific file with custom command to test:

```bash
bcore-mutation analyze -f <file name> --sqlite --run_id <run id number> -c <command to test>
```

### Examples:

For general analysis, on run id 10:

```bash
bcore-mutation analyze --sqlite --run_id 10
```

Analysis on the muts-pr-wallet-1-150 folder generated on run id 1:

```bash
bcore-mutation analyze -f muts-pr-wallet-1-150 --sqlite --run_id 1
```

Perform analysis for muts-pr-wallet-1-150 folder of run id 2 with custom command `cmake --build build`:

```bash
bcore-mutation analyze -f muts-pr-wallet-1-150 --sqlite --run_id 2 -c "cmake --build build"

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
