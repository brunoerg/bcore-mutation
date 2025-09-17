use crate::error::{MutationError, Result};
use chrono::{DateTime, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct MutantInfo {
    pub id: usize,
    pub commit: String,
    pub diff: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReportData {
    pub filename: String,
    pub mutation_score: f64,
    pub date: String,
    pub diffs: HashMap<String, Vec<MutantInfo>>,
}

pub async fn generate_report(
    not_killed_mutants: &[String],
    folder: &str,
    original_file: &str,
    score: f64,
) -> Result<()> {
    // Skip creating a report file if mutation score is 100%
    if not_killed_mutants.is_empty() {
        return Ok(());
    }

    let now: DateTime<Local> = Local::now();
    let mut original_file_path = original_file.to_string();

    // Adjust path for test files
    if original_file_path.contains("test/") && !original_file_path.contains(".cpp") {
        if let Some(start_index) = original_file_path.find("test/") {
            original_file_path = original_file_path[start_index..].to_string();
        }
    }

    // Restore original file
    restore_original_file(&original_file_path).await?;

    println!("Surviving mutants:");

    let mut diffs = Vec::new();

    // Collect diffs for all surviving mutants
    for filename in not_killed_mutants {
        let modified_file = Path::new(folder).join(filename);
        let diff_output =
            get_git_diff(&original_file_path, modified_file.to_str().unwrap()).await?;

        println!("{}", diff_output);
        println!("--------------");

        diffs.push(diff_output);
    }

    // Parse diffs and create report
    let parsed_diffs = parse_diffs_to_json(&diffs).await?;

    let report_data = ReportData {
        filename: original_file_path.clone(),
        mutation_score: score,
        date: now.format("%d/%m/%Y %H:%M:%S").to_string(),
        diffs: parsed_diffs,
    };

    // Save report
    save_report(report_data, &original_file_path).await?;

    Ok(())
}

async fn restore_original_file(file_path: &str) -> Result<()> {
    let output = Command::new("git")
        .args(&["checkout", "--", file_path])
        .output()
        .map_err(|e| MutationError::Git(format!("Failed to restore file: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MutationError::Git(format!(
            "Git checkout failed: {}",
            stderr
        )));
    }

    Ok(())
}

async fn get_git_diff(original_file: &str, modified_file: &str) -> Result<String> {
    let output = Command::new("git")
        .args(&["diff", "--no-index", original_file, modified_file])
        .output()
        .map_err(|e| MutationError::Git(format!("Failed to get git diff: {}", e)))?;

    // git diff --no-index returns exit code 1 when files differ, which is expected
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.to_string())
}

async fn parse_diffs_to_json(diffs_list: &[String]) -> Result<HashMap<String, Vec<MutantInfo>>> {
    let mut result = HashMap::new();
    let line_regex = Regex::new(r"@@ -(\d+),")?;
    let commit = get_git_hash().await?;

    for diff in diffs_list {
        if let Some(captures) = line_regex.captures(diff) {
            let line_num = captures[1].parse::<usize>().map_err(|_| {
                MutationError::InvalidInput("Invalid line number in diff".to_string())
            })?;
            let line_key = (line_num + 3).to_string();

            let entry = result.entry(line_key).or_insert_with(Vec::new);

            // Find the start of the actual diff content (after @@)
            let diff_content = if let Some(pos) = diff.find("@@") {
                &diff[pos..]
            } else {
                diff
            };

            entry.push(MutantInfo {
                id: entry.len() + 1,
                commit: commit.clone(),
                diff: diff_content.to_string(),
                status: "alive".to_string(),
            });
        }
    }

    Ok(result)
}

async fn get_git_hash() -> Result<String> {
    let output = Command::new("git")
        .args(&["log", "--pretty=format:%h", "-n", "1"])
        .output()
        .map_err(|e| MutationError::Git(format!("Failed to get git hash: {}", e)))?;

    if output.status.success() {
        let hash = String::from_utf8_lossy(&output.stdout);
        Ok(hash.trim().to_string())
    } else {
        Ok("unknown".to_string())
    }
}

async fn save_report(report_data: ReportData, _original_file_path: &str) -> Result<()> {
    let json_file = "diff_not_killed.json";

    let final_data = if Path::new(json_file).exists() {
        // Load existing data and append
        let existing_content = fs::read_to_string(json_file)?;
        let mut existing_data: serde_json::Value = serde_json::from_str(&existing_content)?;

        match existing_data {
            serde_json::Value::Array(ref mut arr) => {
                arr.push(serde_json::to_value(report_data)?);
                existing_data
            }
            _ => {
                // Convert single object to array and append
                serde_json::json!([existing_data, report_data])
            }
        }
    } else {
        // Create new array with this report
        serde_json::json!([report_data])
    };

    let json_content = serde_json::to_string_pretty(&final_data)?;
    fs::write(json_file, json_content)?;

    println!("Report saved to {}", json_file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_diffs_to_json() {
        let diffs = vec![
            "@@ -10,3 +11,5 @@ some context\n-old line\n+new line".to_string(),
            "@@ -20,1 +21,1 @@ other context\n-another old\n+another new".to_string(),
        ];

        let result = parse_diffs_to_json(&diffs).await.unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.contains_key("13")); // 10 + 3
        assert!(result.contains_key("23")); // 20 + 3

        let first_entry = &result["13"][0];
        assert_eq!(first_entry.id, 1);
        assert_eq!(first_entry.status, "alive");
        assert!(first_entry.diff.contains("@@"));
    }

    #[test]
    fn test_report_data_serialization() {
        let mut diffs = HashMap::new();
        diffs.insert(
            "10".to_string(),
            vec![MutantInfo {
                id: 1,
                commit: "abc123".to_string(),
                diff: "@@ test diff".to_string(),
                status: "alive".to_string(),
            }],
        );

        let report = ReportData {
            filename: "test.cpp".to_string(),
            mutation_score: 0.85,
            date: "01/01/2024 12:00:00".to_string(),
            diffs,
        };

        let json = serde_json::to_string(&report).unwrap();
        let deserialized: ReportData = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.filename, "test.cpp");
        assert_eq!(deserialized.mutation_score, 0.85);
        assert_eq!(deserialized.diffs.len(), 1);
    }
}
