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
    let mut used_remote = "upstream"; // Track which remote we successfully used

    if let Some(pr) = pr_number {
        // Try to fetch the PR from upstream first
        let fetch_upstream_args = &["fetch", "upstream", &format!("pull/{}/head:pr/{}", pr, pr)];
        match run_git_command(fetch_upstream_args).await {
            Ok(_) => {
                println!("Successfully fetched from upstream");
                println!("Checking out...");
                let checkout_args = &["checkout", &format!("pr/{}", pr)];
                run_git_command(checkout_args).await?;
            }
            Err(upstream_err) => {
                println!("Failed to fetch from upstream: {:?}", upstream_err);
                println!("Trying to fetch from origin...");

                // Try to fetch from origin as fallback
                let fetch_origin_args =
                    &["fetch", "origin", &format!("pull/{}/head:pr/{}", pr, pr)];
                match run_git_command(fetch_origin_args).await {
                    Ok(_) => {
                        println!("Successfully fetched from origin");
                        used_remote = "origin";
                        println!("Checking out...");
                        let checkout_args = &["checkout", &format!("pr/{}", pr)];
                        run_git_command(checkout_args).await?;
                    }
                    Err(origin_err) => {
                        println!("Failed to fetch from origin: {:?}", origin_err);
                        println!("Attempting to rebase existing pr/{} branch...", pr);
                        let rebase_args = &["rebase", &format!("pr/{}", pr)];
                        run_git_command(rebase_args).await?;
                        // In rebase case, we don't know which remote was used originally
                        // Try upstream first, fall back to origin if it fails
                    }
                }
            }
        }
    }

    // Try diff with the appropriate remote
    let diff_args = &[
        "diff",
        "--name-only",
        "--diff-filter=d",
        &format!("{}/master...HEAD", used_remote),
    ];
    match run_git_command(diff_args).await {
        Ok(result) => Ok(result),
        Err(_) if used_remote == "upstream" => {
            // If upstream diff failed, try origin
            println!("Diff with upstream/master failed, trying origin/master...");
            let diff_args_origin = &["diff", "--name-only", "origin/master...HEAD"];
            run_git_command(diff_args_origin).await
        }
        Err(e) => Err(e),
    }
}

pub async fn get_lines_touched(file_path: &str) -> Result<Vec<usize>> {
    // Try upstream first
    let diff_args_upstream = &[
        "diff",
        "--unified=0",
        "upstream/master...HEAD",
        "--",
        file_path,
    ];

    let diff_output = match run_git_command(diff_args_upstream).await {
        Ok(output) => output,
        Err(_) => {
            // Fall back to origin if upstream fails
            println!("Diff with upstream/master failed, trying origin/master...");
            let diff_args_origin = &[
                "diff",
                "--unified=0",
                "origin/master...HEAD",
                "--",
                file_path,
            ];
            run_git_command(diff_args_origin).await?
        }
    };

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

pub fn get_commit_hash() -> Result<String> {

    let commit_hash = Command::new("git")
    .args(["rev-parse", "HEAD"])
    .output()
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .unwrap_or_else(|_| "unknown".to_string());

    Ok(commit_hash)
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
