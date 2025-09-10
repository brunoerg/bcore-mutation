use crate::error::{MutationError, Result};
use regex::Regex;
use std::process::Command;
use std::str;

pub async fn run_git_command(args: &[&str]) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| MutationError::Git(format!("Failed to execute git command: {}", e)))?;

    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
        return Err(MutationError::Git(format!(
            "Git command failed: {}",
            stderr
        )));
    }

    let stdout = str::from_utf8(&output.stdout)
        .map_err(|e| MutationError::Git(format!("Invalid UTF-8 in git output: {}", e)))?;

    Ok(stdout.lines().map(|s| s.to_string()).collect())
}

pub async fn get_changed_files(pr_number: Option<u32>) -> Result<Vec<String>> {
    if let Some(pr) = pr_number {
        // Fetch the PR
        let fetch_args = &["fetch", "upstream", &format!("pull/{}/head:pr/{}", pr, pr)];
        match run_git_command(fetch_args).await {
            Ok(_) => {
                println!("Checking out...");
                let checkout_args = &["checkout", &format!("pr/{}", pr)];
                run_git_command(checkout_args).await?;
            }
            Err(_) => {
                println!("Fetching and updating branch...");
                let rebase_args = &["rebase", &format!("pr/{}", pr)];
                run_git_command(rebase_args).await?;
            }
        }
    }

    let diff_args = &["diff", "--name-only", "upstream/master...HEAD"];
    run_git_command(diff_args).await
}

pub async fn get_lines_touched(file_path: &str) -> Result<Vec<usize>> {
    let diff_args = &[
        "diff",
        "--unified=0",
        "upstream/master...HEAD",
        "--",
        file_path,
    ];
    let diff_output = run_git_command(diff_args).await?;

    let mut lines = Vec::new();
    let line_range_regex = Regex::new(r"@@.*\+(\d+)(?:,(\d+))?.*@@")?;

    for line in diff_output {
        if line.starts_with("@@") {
            if let Some(captures) = line_range_regex.captures(&line) {
                let start_line: usize = captures[1]
                    .parse()
                    .map_err(|_| MutationError::Git("Invalid line number in diff".to_string()))?;

                let num_lines = if let Some(count_match) = captures.get(2) {
                    count_match
                        .as_str()
                        .parse::<usize>()
                        .map_err(|_| MutationError::Git("Invalid line count in diff".to_string()))?
                } else {
                    1
                };

                lines.extend(start_line..start_line + num_lines);
            }
        }
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_lines_touched_parsing() {
        // This would require a git repository setup, so we'll test the regex parsing logic
        let line_range_regex = Regex::new(r"@@.*\+(\d+)(?:,(\d+))?.*@@").unwrap();

        // Test single line change
        let single_line = "@@ -10,0 +11 @@ some context";
        if let Some(captures) = line_range_regex.captures(single_line) {
            let start_line: usize = captures[1].parse().unwrap();
            let num_lines = if let Some(count_match) = captures.get(2) {
                count_match.as_str().parse::<usize>().unwrap()
            } else {
                1
            };
            assert_eq!(start_line, 11);
            assert_eq!(num_lines, 1);
        }

        // Test multiple line change
        let multi_line = "@@ -10,3 +11,5 @@ some context";
        if let Some(captures) = line_range_regex.captures(multi_line) {
            let start_line: usize = captures[1].parse().unwrap();
            let num_lines = captures.get(2).unwrap().as_str().parse::<usize>().unwrap();
            assert_eq!(start_line, 11);
            assert_eq!(num_lines, 5);
        }
    }
}
