use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::context::tool_dedup::get_global_tool_dedup;
use crate::tools::infra::undo_history;

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

        let content = std::fs::read_to_string(&args.path).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = content.split('\n').collect();
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

        let new_lines: Vec<&str> = if args.new_content.is_empty() {
            vec![]
        } else {
            args.new_content.split('\n').collect()
        };

        let mut result_lines: Vec<&str> = Vec::with_capacity(
            total_lines - args.delete_count + new_lines.len(),
        );

        result_lines.extend_from_slice(&lines[..args.start_line - 1]);
        result_lines.extend_from_slice(&new_lines);
        result_lines.extend_from_slice(&lines[args.start_line - 1 + args.delete_count..]);

        let new_file_content = result_lines.join("\n");

        // Record the change for undo before writing
        let _ = undo_history::record_change(
            &args.path,
            Some(content.clone()),
            Some(new_file_content.clone()),
            "file_update",
        );

        std::fs::write(&args.path, &new_file_content).map_err(|e| e.to_string())?;

        // Invalidate dedup cache for this path — file content has changed
        {
            let dedup = get_global_tool_dedup();
            let mut guard = dedup.lock().unwrap();
            guard.invalidate_path(&args.path);
        }

        // Build a diff showing the line-level change
        let diff = build_line_diff(args.start_line, args.delete_count, &args.new_content, &lines);

        // Run git diff to show what changed
        let git_diff = super::run_git_diff(&args.path).await;

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

    for line in new_content.split('\n') {
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
