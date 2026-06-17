//! Git-based change detection for review orchestrator.
//!
//! Detects changed files by running `git diff` directly (more reliable than
//! parsing tool outputs) and generates unified diffs for the review LLM.

use crate::core::types::review::*;

/// Detect changed files by running `git diff` directly.
///
/// This is more reliable than parsing tool outputs, as it always reflects
/// the actual working tree state.
///
/// Always diffs against HEAD so the review LLM sees the complete picture of
/// all uncommitted changes. Designed after codebuff's approach: the review
/// agent receives the full diff and relies on its system prompt to focus on
/// the most relevant changes.
///
/// Also detects untracked files (e.g., from `mv` via shell tool) so the review
/// LLM has complete context — without this, a moved file would appear only as
/// "Deleted" with no corresponding "Added" entry, causing false positives.
pub async fn detect_changed_files_from_git() -> Vec<ChangedFile> {
    let mut files = Vec::new();

    // Step 1: Get tracked changes via git diff (always against HEAD)
    let has_tracked_changes = match tokio::process::Command::new("git")
        .args(["diff", "--no-color"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            let diff_text = String::from_utf8_lossy(&o.stdout);
            if !diff_text.trim().is_empty() {
                files = parse_git_diff(&diff_text);
                true
            } else {
                false
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!("detect_changed_files_from_git: git diff failed: {}", stderr);
            false
        }
        Err(e) => {
            tracing::warn!(
                "detect_changed_files_from_git: failed to run git diff: {}",
                e
            );
            false
        }
    };

    // Step 2: Find untracked files (e.g., created by `mv` or `cp` via shell tool)
    // `git ls-files --others --exclude-standard` lists untracked files not in .gitignore
    let untracked_files = match tokio::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            let output = String::from_utf8_lossy(&o.stdout);
            output
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!(
                "detect_changed_files_from_git: git ls-files failed: {}",
                stderr
            );
            Vec::new()
        }
        Err(e) => {
            tracing::warn!(
                "detect_changed_files_from_git: failed to run git ls-files: {}",
                e
            );
            Vec::new()
        }
    };

    // Build ChangedFile entries for untracked files
    for path in &untracked_files {
        // Don't add duplicates if the file already appears in tracked changes
        if files.iter().any(|f| f.path == *path) {
            continue;
        }
        // Read the file content
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => continue, // skip files that can't be read
        };
        let line_count = content.lines().count();
        // Generate a unified diff for the added file
        let mut diff = format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
        );
        diff.push_str(&format!("@@ -0,0 +1,{} @@\n", line_count.max(1)));
        for line in content.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        // Ensure trailing newline
        if !content.ends_with('\n') {
            diff.push('+');
            diff.push('\n');
        }
        files.push(ChangedFile {
            path: path.clone(),
            change_type: ChangeType::Added,
            lines_added: line_count,
            lines_removed: 0,
            diff,
        });
    }

    if files.is_empty() {
        tracing::info!("detect_changed_files_from_git: no changes");
    } else {
        tracing::info!(
            count = files.len(),
            tracked = has_tracked_changes,
            untracked = untracked_files.len(),
            "detect_changed_files_from_git: found changes"
        );
    }

    files
}

/// Parse unified diff output from `git diff` into per-file `ChangedFile` entries.
pub fn parse_git_diff(diff_text: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut current_diff = String::new();
    let mut current_path: Option<String> = None;
    let mut current_change_type = ChangeType::Modified;

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            // Save previous file if any
            if let Some(path) = current_path.take() {
                let (added, removed) = count_diff_lines(&current_diff);
                files.push(ChangedFile {
                    path,
                    change_type: current_change_type.clone(),
                    lines_added: added,
                    lines_removed: removed,
                    diff: current_diff.clone(),
                });
                current_diff.clear();
            }
            // Extract file path from "diff --git a/path b/path"
            let path = line
                .strip_prefix("diff --git a/")
                .and_then(|s| s.split(" b/").next())
                .unwrap_or("")
                .to_string();
            current_path = Some(path);
            current_change_type = ChangeType::Modified;
            current_diff.push_str(line);
            current_diff.push('\n');
        } else if line.starts_with("new file mode") {
            current_change_type = ChangeType::Added;
            current_diff.push_str(line);
            current_diff.push('\n');
        } else if line.starts_with("deleted file mode") {
            current_change_type = ChangeType::Deleted;
            current_diff.push_str(line);
            current_diff.push('\n');
        } else if current_path.is_some() {
            current_diff.push_str(line);
            current_diff.push('\n');
        }
    }

    // Save last file
    if let Some(path) = current_path {
        let (added, removed) = count_diff_lines(&current_diff);
        files.push(ChangedFile {
            path,
            change_type: current_change_type,
            lines_added: added,
            lines_removed: removed,
            diff: current_diff,
        });
    }

    files
}

/// Count added/removed lines from a unified diff string.
fn count_diff_lines(diff: &str) -> (usize, usize) {
    let added = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .count();
    let removed = diff
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .count();
    (added, removed)
}
