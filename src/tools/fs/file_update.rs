use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct FileUpdateArgs {
    pub path: String,
    /// 1-indexed line number where the edit starts
    pub start_line: usize,
    /// Number of lines to delete from start_line (0 = insert without deleting)
    pub delete_count: usize,
    /// The content to insert at start_line, replacing any deleted lines
    pub new_content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileUpdateOutput {
    pub path: String,
    /// Number of lines deleted
    pub replacements: usize,
    pub diff: String,
    /// Git diff showing what changed (None if file is untracked or not in a git repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FileUpdate;

#[async_trait::async_trait]
impl Tool for FileUpdate {
    fn name(&self) -> &str {
        "file_update"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Edit an existing file by specifying a line range to replace. \
                Read the file first with file_read to see line numbers, then use file_update \
                to replace lines at a specific position. \
                Set delete_count=0 to insert new lines without deleting anything. \
                Set new_content=\"\" to delete lines without inserting anything."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to update (relative to project root or absolute)"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "1-indexed line number where the edit starts"
                    },
                    "delete_count": {
                        "type": "integer",
                        "description": "Number of lines to delete from start_line (0 = insert without deleting)"
                    },
                    "new_content": {
                        "type": "string",
                        "description": "The content to insert at start_line, replacing any deleted lines. Use empty string to just delete."
                    }
                },
                "required": ["path", "start_line", "delete_count", "new_content"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileUpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let content = tokio::fs::read_to_string(&args.path).await.map_err(|e| e.to_string())?;
        let has_trailing_newline = content.ends_with('\n');
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if args.start_line == 0 {
            return Err("start_line must be >= 1 (1-indexed)".to_string());
        }
        if args.start_line > total_lines + 1 {
            return Err(format!(
                "start_line {} is beyond file length ({} lines). File: {}",
                args.start_line, total_lines, args.path
            ));
        }

        if args.start_line - 1 + args.delete_count > total_lines {
            return Err(format!(
                "start_line {} + delete_count {} exceeds file length ({} lines). File: {}",
                args.start_line, args.delete_count, total_lines, args.path
            ));
        }

        // Trim trailing newlines from new_content to avoid empty-string elements
        // from split('\n') when the model includes a trailing newline.
        // This matches the behavior of str::lines() which doesn't return
        // a trailing empty string.
        let new_content_trimmed = args.new_content.trim_end_matches('\n');
        let new_lines: Vec<&str> = if new_content_trimmed.is_empty() {
            vec![]
        } else {
            new_content_trimmed.split('\n').collect()
        };
        // Detect potential bracket/line duplication:
        // If new_content's last line matches the first preserved line after the
        // deletion range, the model likely included an extra copy of that line.
        // e.g. replacing line 2 with "    baz();\n}" but delete_count=1 preserves
        // the original "}" on line 3, producing double "}}".
        let mut duplication_warning: Option<String> = None;
        if let Some(last_new) = new_lines.last() {
            let preserved_start = args.start_line - 1 + args.delete_count;
            if preserved_start < total_lines && !last_new.is_empty() {
                let first_preserved = lines[preserved_start];
                if *last_new == first_preserved {
                    duplication_warning = Some(format!(
                        "WARNING: new_content's last line {:?} matches line {} ({:?}) \
                         which would also be preserved. This may create a duplicate. \
                         Consider increasing delete_count to {} or removing \"{}\" from new_content.",
                        last_new,
                        preserved_start + 1,
                        first_preserved,
                        args.delete_count + 1,
                        last_new,
                    ));
                }
            }
        }



        let mut result_lines: Vec<&str> = Vec::with_capacity(
            total_lines - args.delete_count + new_lines.len(),
        );

        result_lines.extend_from_slice(&lines[..args.start_line - 1]);
        result_lines.extend_from_slice(&new_lines);
        result_lines.extend_from_slice(&lines[args.start_line - 1 + args.delete_count..]);

        let mut new_file_content = result_lines.join("\n");
        if has_trailing_newline {
            new_file_content.push('\n');
        }

        // Use the shared tracking utility: write + undo + dedup invalidation + git diff.
        // Pass `Some(content.clone())` so it doesn't re-read the file we just read.
        // Clone because `lines` borrows from `content` and is used below for the diff.
        let (_, git_diff) = super::fs_write_with_tracking(
            &args.path,
            &new_file_content,
            "file_update",
            Some(content.clone()),
        )
        .await?;

        // Build a diff showing the line-level change
        let mut diff = build_line_diff(args.start_line, args.delete_count, &args.new_content, &lines);
        if let Some(warning) = duplication_warning {
            diff.push_str(&format!("\n{}\n", warning));
        }

        serde_json::to_string(&FileUpdateOutput {
            path: args.path,
            replacements: args.delete_count,
            diff,
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}

/// Build a human-readable diff showing what lines were removed and added.
pub fn build_line_diff(
    start_line: usize,
    delete_count: usize,
    new_content: &str,
    original_lines: &[&str],
) -> String {
    let mut diff = String::new();
    diff.push_str(&format!("@@ line {} @@\n", start_line));

    for i in 0..delete_count {
        let idx = start_line - 1 + i;
        if idx < original_lines.len() {
            diff.push_str(&format!("-{}\n", original_lines[idx]));
        }
    }

    // Trim trailing newlines to keep diff output consistent
    let trimmed = new_content.trim_end_matches('\n');
    for line in trimmed.split('\n') {
        diff.push_str(&format!("+{}\n", line));
    }

    diff
}

/// Builds a minimal unified-diff-style string showing a string-level replacement.
/// Used by file_delete for snippet deletion diffs.
pub fn build_diff(old: &str, new: &str, content: &str) -> String {
    let before_lines: Vec<&str> = content.lines().collect();
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Find the starting line number by checking full-line-boundary matches first
    let mut line_num = None;
    if !old_lines.is_empty() {
        for (i, window) in before_lines.windows(old_lines.len().max(1)).enumerate() {
            if window.join("\n") == old {
                line_num = Some(i + 1);
                break;
            }
        }
    }

    // Fallback: find the first line containing the old string as a substring
    let line_num = line_num.unwrap_or_else(|| {
        before_lines
            .iter()
            .position(|line| line.contains(old))
            .map(|i| i + 1)
            .unwrap_or(1)
    });

    let mut diff = String::new();
    diff.push_str(&format!("@@ line {} @@\n", line_num));
    for line in &old_lines {
        diff.push_str(&format!("-{}\n", line));
    }
    for line in &new_lines {
        diff.push_str(&format!("+{}\n", line));
    }

    diff
}
