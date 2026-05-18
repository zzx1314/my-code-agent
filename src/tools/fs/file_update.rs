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

        // Trim leading and trailing newlines from new_content to avoid empty-string
        // elements from split('\n') when the model includes newlines from code block
        // fences or trailing formatting. This matches the behavior of str::lines()
        // which doesn't return a trailing empty string.
        // Common causes:
        //   - Leading \\n: LLM code block extraction (```\ncode)
        //   - Trailing \\n: LLM code block formatting (code\n```)
        let new_content_trimmed = args.new_content.trim_matches('\n');
        let new_lines: Vec<&str> = if new_content_trimmed.is_empty() {
            vec![]
        } else {
            new_content_trimmed.split('\n').collect()
        };
        let new_lines = deduplicate_closing_brackets(new_lines, args.start_line, args.delete_count, &lines, total_lines);

        // LLMs often include the preceding line in new_content (from code block
        // extraction or surrounding context), producing duplicate lines above the
        // edit point. Detect and remove the first line of new_content if it matches
        // the line immediately before the edit point.
        let new_lines = if args.start_line > 1 {
            let prev_line = lines[args.start_line - 2];
            if new_lines.first().map_or(false, |first| *first == prev_line) {
                new_lines[1..].to_vec()
            } else {
                new_lines
            }
        } else {
            new_lines
        };

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
        let diff = build_line_diff(args.start_line, args.delete_count, &args.new_content, &lines);

        serde_json::to_string(&FileUpdateOutput {
            path: args.path,
            replacements: args.delete_count,
            diff,
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}

/// Remove a trailing closing-bracket line from `new_lines` when it would duplicate
/// the first preserved line after the deletion range. LLMs often include closing
/// brackets that already exist in the original file, producing `}}` or `])`.
///
/// Only deduplicates when the bracket TYPE matches (e.g. `}` with `}` or `};`),
/// NOT across different types (e.g. `}` with `)`) — that would cause bracket loss.
fn deduplicate_closing_brackets<'a>(
    mut new_lines: Vec<&'a str>,
    start_line: usize,
    delete_count: usize,
    original_lines: &[&'a str],
    total_lines: usize,
) -> Vec<&'a str> {
    let Some(last_new) = new_lines.last() else { return new_lines };
    if last_new.trim().is_empty() {
        return new_lines;
    }
    let preserved_start = start_line - 1 + delete_count;
    if preserved_start >= total_lines {
        return new_lines;
    }
    let first_preserved = original_lines[preserved_start];

    // Extract the bracket character from a line if it's a closing bracket.
    // Treats `}`, `},`, `};` all as `}`, etc.
    let bracket_kind = |s: &str| -> Option<char> {
        match s.trim() {
            "}" | "}," | "};" => Some('}'),
            ")" | ");" => Some(')'),
            "]" | "]," => Some(']'),
            _ => None,
        }
    };

    if let (Some(b1), Some(b2)) = (bracket_kind(last_new), bracket_kind(first_preserved)) {
        if b1 == b2 {
            new_lines.pop();
        }
    }
    new_lines
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
