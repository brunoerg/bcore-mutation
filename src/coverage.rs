use crate::error::{MutationError, Result};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn parse_coverage_file(coverage_file_path: &Path) -> Result<HashMap<String, Vec<usize>>> {
    let content = fs::read_to_string(coverage_file_path)?;
    let mut coverage_data: HashMap<String, Vec<usize>> = HashMap::new();
    let mut current_file: Option<String> = None;

    // Regular expressions for parsing lines
    let file_pattern = Regex::new(r"^SF:(.+)$")?; // Source file
    let line_pattern = Regex::new(r"^DA:(\d+),(\d+)$")?; // Line coverage

    for line in content.lines() {
        let line = line.trim();

        // Check for source file
        if let Some(captures) = file_pattern.captures(line) {
            let full_path = &captures[1];

            // Extract from "src/" onwards
            let relative_path = if let Some(pos) = full_path.find("src/") {
                &full_path[pos..]
            } else {
                full_path // fallback to full path if "src/" not found
            };

            current_file = Some(relative_path.to_string());
            coverage_data.insert(relative_path.to_string(), Vec::new());
            continue;
        }

        // Check for line coverage (DA:line_number,hits)
        if let Some(captures) = line_pattern.captures(line) {
            if let Some(ref file) = current_file {
                let line_number: usize = captures[1]
                    .parse()
                    .map_err(|_| MutationError::Coverage("Invalid line number".to_string()))?;
                let hits: usize = captures[2]
                    .parse()
                    .map_err(|_| MutationError::Coverage("Invalid hit count".to_string()))?;

                if hits > 0 {
                    if let Some(lines) = coverage_data.get_mut(file) {
                        if !lines.contains(&line_number) {
                            lines.push(line_number);
                        }
                    }
                }
            }
        }
    }

    Ok(coverage_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_coverage_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "SF:/path/to/file1.cpp").unwrap();
        writeln!(temp_file, "DA:1,5").unwrap();
        writeln!(temp_file, "DA:2,0").unwrap();
        writeln!(temp_file, "DA:3,10").unwrap();
        writeln!(temp_file, "SF:/path/to/file2.cpp").unwrap();
        writeln!(temp_file, "DA:10,1").unwrap();
        writeln!(temp_file, "DA:11,0").unwrap();

        let result = parse_coverage_file(temp_file.path()).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result["/path/to/file1.cpp"], vec![1, 3]);
        assert_eq!(result["/path/to/file2.cpp"], vec![10]);
    }
}
