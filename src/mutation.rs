use crate::ast_analysis::{filter_mutatable_lines, AridNodeDetector};
use crate::error::{MutationError, Result};
use crate::git_changes::{get_changed_files, get_lines_touched};
use crate::operators::{
    get_do_not_mutate_patterns, get_do_not_mutate_py_patterns, get_do_not_mutate_unit_patterns,
    get_regex_operators, get_security_operators, get_skip_if_contain_patterns, get_test_operators,
    should_mutate_test_line,
};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct FileToMutate {
    pub file_path: String,
    pub lines_touched: Vec<usize>,
    pub is_unit_test: bool,
}

pub async fn run_mutation(
    pr_number: Option<u32>,
    file: Option<PathBuf>,
    one_mutant: bool,
    only_security_mutations: bool,
    range_lines: Option<(usize, usize)>,
    coverage: Option<HashMap<String, Vec<usize>>>,
    test_only: bool,
    skip_lines: HashMap<String, Vec<usize>>,
    enable_ast_filtering: bool,
    custom_expert_rule: Option<String>,
) -> Result<()> {
    if let Some(file_path) = file {
        let file_str = file_path.to_string_lossy().to_string();
        let is_unit_test = file_str.contains("test") && !file_str.contains(".py");

        mutate_file(
            &file_str,
            None,
            None,
            one_mutant,
            only_security_mutations,
            range_lines,
            &coverage,
            is_unit_test,
            &skip_lines,
            enable_ast_filtering,
            custom_expert_rule,
        )
        .await?;
        return Ok(());
    }

    let files_changed = get_changed_files(pr_number).await?;
    let mut files_to_mutate = Vec::new();

    for file_changed in files_changed {
        // Skip certain file types
        if file_changed.contains("doc")
            || file_changed.contains("fuzz")
            || file_changed.contains("bench")
            || file_changed.contains("util")
            || file_changed.contains("sanitizer_supressions")
            || file_changed.ends_with(".txt")
        {
            continue;
        }

        let lines_touched = get_lines_touched(&file_changed).await?;
        let is_unit_test = file_changed.contains("test")
            && !file_changed.contains(".py")
            && !file_changed.contains("util");

        if test_only && !(is_unit_test || file_changed.contains(".py")) {
            continue;
        }

        files_to_mutate.push(FileToMutate {
            file_path: file_changed,
            lines_touched,
            is_unit_test,
        });
    }

    for file_info in files_to_mutate {
        mutate_file(
            &file_info.file_path,
            Some(file_info.lines_touched),
            pr_number,
            one_mutant,
            only_security_mutations,
            range_lines,
            &coverage,
            file_info.is_unit_test,
            &skip_lines,
            enable_ast_filtering,
            custom_expert_rule.clone(),
        )
        .await?;
    }

    Ok(())
}

pub async fn mutate_file(
    file_to_mutate: &str,
    touched_lines: Option<Vec<usize>>,
    pr_number: Option<u32>,
    one_mutant: bool,
    only_security_mutations: bool,
    range_lines: Option<(usize, usize)>,
    coverage: &Option<HashMap<String, Vec<usize>>>,
    is_unit_test: bool,
    skip_lines: &HashMap<String, Vec<usize>>,
    enable_ast_filtering: bool,
    custom_expert_rule: Option<String>,
) -> Result<()> {
    println!("\n\nGenerating mutants for {}...", file_to_mutate);

    let source_code = fs::read_to_string(file_to_mutate)?;
    let lines: Vec<&str> = source_code.lines().collect();
    println!("File has {} lines", lines.len());

    // Initialize AST-based arid node detection for C++ files
    let mut arid_detector = if enable_ast_filtering
        && (file_to_mutate.ends_with(".cpp") || file_to_mutate.ends_with(".h"))
    {
        let mut detector = AridNodeDetector::new()?;

        // Add custom expert rule if provided
        if let Some(rule) = custom_expert_rule {
            detector.add_expert_rule(&rule, "Custom user rule")?;
        }

        Some(detector)
    } else {
        if !enable_ast_filtering {
            println!("AST filtering disabled - generating all possible mutants");
        }
        None
    };

    // Filter out arid lines using AST analysis (for C++ files)
    let ast_filtered_lines = if let Some(ref mut detector) = arid_detector {
        let string_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        let mutatable_line_numbers = filter_mutatable_lines(&string_lines, detector);
        println!(
            "AST analysis filtered to {} mutatable lines (from {})",
            mutatable_line_numbers.len(),
            lines.len()
        );

        // Show some examples of filtered out lines
        let filtered_out_count = lines.len() - mutatable_line_numbers.len();
        if filtered_out_count > 0 {
            println!(
                "Filtered out {} arid lines (logging, reserve calls, etc.)",
                filtered_out_count
            );
        }

        Some(mutatable_line_numbers)
    } else {
        None
    };

    // Select operators based on file type and options
    let operators = if only_security_mutations {
        println!("Using security operators");
        get_security_operators()?
    } else if file_to_mutate.contains(".py") || is_unit_test {
        println!("Using test operators (Python or unit test file)");
        get_test_operators()?
    } else {
        println!("Using regex operators");
        get_regex_operators()?
    };

    println!("Loaded {} operators", operators.len());

    let skip_lines_for_file = skip_lines.get(file_to_mutate);
    let mut touched_lines = touched_lines.unwrap_or_else(|| (1..=lines.len()).collect());

    // Apply AST filtering if available
    if let Some(ast_lines) = ast_filtered_lines {
        // Intersect touched_lines with AST-filtered lines
        touched_lines.retain(|line_num| ast_lines.contains(line_num));
        println!(
            "After AST filtering: {} lines to process",
            touched_lines.len()
        );
    }

    // Get coverage data for this file
    let lines_with_test_coverage = if let Some(cov) = coverage {
        cov.iter()
            .find(|(path, _)| file_to_mutate.contains(path.as_str()))
            .map(|(_, lines)| lines.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    if !lines_with_test_coverage.is_empty() {
        println!(
            "Using coverage data with {} covered lines",
            lines_with_test_coverage.len()
        );
    }

    let mut mutant_count = 0;

    if one_mutant {
        println!("One mutant mode enabled");
    }

    for line_num in touched_lines {
        let line_idx = line_num.saturating_sub(1);

        // Check coverage if provided
        if !lines_with_test_coverage.is_empty() && !lines_with_test_coverage.contains(&line_num) {
            continue;
        }

        // Check range if provided
        if let Some((start, end)) = range_lines {
            if line_idx < start || line_idx > end {
                continue;
            }
        }

        // Check skip lines (skip_lines uses 1-indexed line numbers)
        if let Some(skip) = skip_lines_for_file {
            if skip.contains(&line_num) {
                continue;
            }
        }

        if line_idx >= lines.len() {
            continue;
        }

        let line_before_mutation = lines[line_idx];

        // Check if line should be skipped (traditional approach)
        if should_skip_line(line_before_mutation, file_to_mutate, is_unit_test)? {
            continue;
        }

        let mut line_had_match = false;

        for operator in &operators {
            // Special handling for test operators
            if file_to_mutate.contains(".py") || is_unit_test {
                if !should_mutate_test_line(line_before_mutation) {
                    continue;
                }
            }

            if operator.pattern.is_match(line_before_mutation) {
                line_had_match = true;
                let line_mutated = operator
                    .pattern
                    .replace(line_before_mutation, &operator.replacement);

                // Create mutated file content
                let mut mutated_lines = lines.clone();
                mutated_lines[line_idx] = &line_mutated;
                let mutated_content = mutated_lines.join("\n");

                mutant_count = write_mutation(
                    file_to_mutate,
                    &mutated_content,
                    mutant_count,
                    pr_number,
                    range_lines,
                )?;

                if one_mutant {
                    break; // Break only from operator loop, continue to next line
                }
            }
        }

        // Debug output for lines that didn't match any patterns
        if !line_had_match && !line_before_mutation.trim().is_empty() {
            println!(
                "Line {} '{}' didn't match any patterns",
                line_num,
                line_before_mutation.trim()
            );
        }

        // Note: Removed the early break that was stopping line processing
        // Now each line gets processed independently
    }

    // Print AST analysis statistics
    if let Some(detector) = arid_detector {
        let stats = detector.get_stats();
        println!("AST Analysis Stats: {:?}", stats);
    }

    println!("Generated {} mutants...", mutant_count);
    Ok(())
}

fn should_skip_line(line: &str, file_path: &str, is_unit_test: bool) -> Result<bool> {
    let trimmed = line.trim_start();

    // Check basic patterns to skip
    for pattern in get_do_not_mutate_patterns() {
        if trimmed.starts_with(pattern) {
            return Ok(true);
        }
    }

    // Check skip if contain patterns
    for pattern in get_skip_if_contain_patterns() {
        if line.contains(pattern) {
            return Ok(true);
        }
    }

    // Language-specific checks
    if file_path.contains(".py") || is_unit_test {
        let patterns = if is_unit_test {
            get_do_not_mutate_unit_patterns()
        } else {
            get_do_not_mutate_py_patterns()
        };

        for pattern in patterns {
            if line.contains(pattern) {
                return Ok(true);
            }
        }

        // Check for assignment patterns
        let assignment_regex = if is_unit_test {
            Regex::new(
                r"\b(?:[a-zA-Z_][a-zA-Z0-9_:<>*&\s]+)\s+[a-zA-Z_][a-zA-Z0-9_]*(?:\[[^\]]*\])?(?:\.(?:[a-zA-Z_][a-zA-Z0-9_]*)|\->(?:[a-zA-Z_][a-zA-Z0-9_]*))*(?:\s*=\s*[^;]+|\s*\{[^;]+\})\s*",
            )?
        } else {
            Regex::new(r"^\s*([a-zA-Z_]\w*)\s*=\s*(.+)$")?
        };

        if assignment_regex.is_match(line) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn write_mutation(
    file_to_mutate: &str,
    mutated_content: &str,
    mutant_index: usize,
    pr_number: Option<u32>,
    range_lines: Option<(usize, usize)>,
) -> Result<usize> {
    let file_extension = if file_to_mutate.ends_with(".h") {
        ".h"
    } else if file_to_mutate.ends_with(".py") {
        ".py"
    } else {
        ".cpp"
    };

    let file_name = Path::new(file_to_mutate)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| MutationError::InvalidInput("Invalid file path".to_string()))?;

    let ext = file_extension.trim_start_matches('.');
    let folder = if let Some(pr) = pr_number {
        format!("muts-pr-{}-{}-{}", pr, file_name, ext)
    } else if let Some(range) = range_lines {
        format!("muts-pr-{}-{}-{}", file_name, range.0, range.1)
    } else {
        format!("muts-{}-{}", file_name, ext)
    };

    create_mutation_folder(&folder, file_to_mutate)?;

    let mutator_file = format!(
        "{}/{}.mutant.{}{}",
        folder, file_name, mutant_index, file_extension
    );
    fs::write(mutator_file, mutated_content)?;

    Ok(mutant_index + 1)
}

fn create_mutation_folder(folder_name: &str, file_to_mutate: &str) -> Result<()> {
    let folder_path = Path::new(folder_name);

    if !folder_path.exists() {
        fs::create_dir_all(folder_path)?;

        let original_file_path = folder_path.join("original_file.txt");
        fs::write(original_file_path, file_to_mutate)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_should_skip_line() {
        // Test basic skip patterns
        assert!(should_skip_line("// This is a comment", "test.cpp", false).unwrap());
        assert!(should_skip_line("assert(condition);", "test.cpp", false).unwrap());
        assert!(should_skip_line("LogPrintf(\"test\");", "test.cpp", false).unwrap());
        assert!(should_skip_line("LogDebug(\"test\");", "test.cpp", false).unwrap());

        // Test normal lines that shouldn't be skipped
        assert!(!should_skip_line("int x = 5;", "test.cpp", false).unwrap());
        assert!(!should_skip_line("return value;", "test.cpp", false).unwrap());
    }

    #[test]
    fn test_create_mutation_folder() {
        let temp_dir = tempdir().unwrap();
        let folder_path = temp_dir.path().join("test_muts");
        let folder_name = folder_path.to_str().unwrap();

        create_mutation_folder(folder_name, "test/file.cpp").unwrap();

        assert!(folder_path.exists());
        assert!(folder_path.join("original_file.txt").exists());

        let content = fs::read_to_string(folder_path.join("original_file.txt")).unwrap();
        assert_eq!(content, "test/file.cpp");
    }

    #[test]
    fn test_write_mutation() {
        let temp_dir = tempdir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let result = write_mutation("test.cpp", "mutated content", 0, None, None).unwrap();
        assert_eq!(result, 1);

        let folder_path = Path::new("muts-test-cpp");
        assert!(folder_path.exists());
        assert!(folder_path.join("test.mutant.0.cpp").exists());

        let content = fs::read_to_string(folder_path.join("test.mutant.0.cpp")).unwrap();
        assert_eq!(content, "mutated content");
    }
}
