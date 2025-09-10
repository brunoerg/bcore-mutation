# AST-Based Arid Node Detection

This document explains the implementation of Google's AST-based arid node detection algorithm in the Rust mutation testing tool.

## Overview

The AST-based filtering prevents generation of "unproductive mutants" by identifying **arid nodes** - statements that are typically not tested by unit tests and thus generate mutants that survive not because of test quality issues, but because they're not meant to be tested.

## Algorithm Implementation

Based on Google's research paper, the algorithm is:

```
arid(N) = expert(N)                    if simple(N)
        = ∀(arid(c)) = 1, ∀c ∈ N      otherwise (compound nodes)
```

Where:
- `N` is a node in the Abstract Syntax Tree
- `simple(N)` determines if a node is simple (no body) or compound (has body/children)
- `expert(N)` is manually curated knowledge about which simple nodes are arid

## Usage Examples

### Basic Usage

```bash
# Enable AST filtering (default for C++ files)
mutation-core mutate -f src/wallet/coinselection.cpp

# Disable AST filtering (generate all mutants)
mutation-core mutate -f src/wallet/coinselection.cpp --disable-ast-filtering

# Add custom expert rule
mutation-core mutate -f src/wallet/coinselection.cpp --add-expert-rule "mylog\s*\("
```

## What Gets Filtered Out

### 1. Memory Management Functions
```cpp
std::vector<COutput> coins;
coins.reserve(100);           // ← FILTERED (arid)
coins.resize(50);             // ← FILTERED (arid)
```

### 2. Logging and Debug Output
```cpp
LogPrintf("Processing %d coins\n", coins.size());  // ← FILTERED (arid)
std::cout << "Debug: " << value << std::endl;      // ← FILTERED (arid)
```

### 3. Performance Monitoring
```cpp
auto start_time = std::chrono::steady_clock::now(); // ← FILTERED (arid)
auto duration = end_time - start_time;              // ← FILTERED (arid)
```

### 4. Bitcoin Core Specific Patterns
```cpp
if (G_FUZZING) return;                              // ← FILTERED (arid)
strprintf("Amount: %s", FormatMoney(amount));       // ← FILTERED (arid)
```

### 5. What DOESN'T Get Filtered
```cpp
if (amount > target) {           // ← MUTATED (business logic)
    return false;                // ← MUTATED (business logic)
}

CAmount fee = CalculateFee();    // ← MUTATED (important calculation)
bool valid = ValidateInput();    // ← MUTATED (validation logic)
```

## Expert Knowledge Rules

The system includes 100+ pre-defined rules based on common patterns:

### Function Call Patterns
- `std::vector<.*>::reserve`
- `std::.*::resize`
- `LogPrintf\s*\(`
- `std::cout\s*<<`
- Memory allocation: `malloc`, `calloc`, `free`
- Threading: `std::thread`, `std::mutex`

### Variable Patterns
- Timing variables: `.*_time$`, `.*_duration$`
- Debug variables: `.*_debug$`, `.*_log$`
- Temporary variables: `temp_.*`, `tmp_.*`

### Statement Patterns
- Comments: `^\s*//`, `^\s*/\*`
- Preprocessor: `^\s*#`
- Namespace declarations: `^\s*namespace\s+`

## Custom Expert Rules

Add your own patterns for project-specific arid code:

```bash
# Filter out custom logging functions
mutation-core mutate -f src/file.cpp --add-expert-rule "MyLogger::"

# Filter out specific debugging code
mutation-core mutate -f src/file.cpp --add-expert-rule "DEBUG_PRINT\s*\("

# Filter out performance counters
mutation-core mutate -f src/file.cpp --add-expert-rule ".*_counter\+\+"
```

## Performance Impact

### Mutation Generation Speed
- **Faster**: Fewer lines to process means faster mutation generation
- **Reduced I/O**: Fewer mutant files to write

### Analysis Speed
- **Faster**: Fewer mutants to compile and test
- **Better Focus**: Time spent on meaningful mutants

## Integration with Existing Features

### Works with Coverage-Guided Testing
```bash
mutation-core mutate -f src/file.cpp -c coverage.info
# 1. Coverage filtering applied first
# 2. AST filtering applied to covered lines
# 3. Result: only covered, non-arid lines mutated
```

### Works with Line Ranges
```bash
mutation-core mutate -f src/file.cpp --range 100 200
# 1. Line range applied first
# 2. AST filtering applied to lines 100-200
# 3. Result: only non-arid lines in range mutated
```

### Works with Skip Lines
```bash
mutation-core mutate -f src/file.cpp --skip-lines skip.json
# 1. Skip lines applied first
# 2. AST filtering applied to non-skipped lines
# 3. Result: comprehensive filtering stack
```

## Technical Implementation

### AST Node Types
```rust
pub enum AstNodeType {
    // Simple nodes (no body)
    FunctionCall,      // func()
    Assignment,        // x = y
    Literal,          // 42, "string"

    // Compound nodes (have body)
    IfStatement,      // if (...) { ... }
    ForLoop,          // for (...) { ... }
    Block,            // { ... }
}
```

### Detection Algorithm
```rust
pub fn is_arid(&mut self, node: &AstNode) -> bool {
    if node.is_simple() {
        // Use expert knowledge for simple nodes
        self.expert.is_arid_simple_node(node)
    } else {
        // For compound nodes: ALL children must be arid
        node.children.iter().all(|child| self.is_arid(child))
    }
}
```

## Future Enhancements

1. **Full AST Parser**: Integrate with `clang` or `tree-sitter` for complete C++ parsing
4. **IDE Integration**: Real-time arid node highlighting

## References

- [Google's Mutation Testing Research Paper](https://research.google/pubs/pub46584/)
- [AST-based Program Analysis](https://en.wikipedia.org/wiki/Abstract_syntax_tree)
