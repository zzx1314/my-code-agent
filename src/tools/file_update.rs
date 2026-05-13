use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::context::tool_dedup::get_global_tool_dedup;
use super::undo_history;

#[derive(Deserialize, Serialize)]
pub struct FileUpdateArgs {
    pub path: String,
    pub old: String,
    pub new: String,
    #[serde(default)]
    pub allow_multiple: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileUpdateOutput {
    pub path: String,
    pub replacements: usize,
    pub diff: String,
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
            description: "Make a targeted edit to an existing file by finding and replacing text. \
                Prefer this over file_write for modifying existing files — it is safer and more precise. \
                The file is read, the `old` string is located, replaced with `new`, and written back. \
                Fails if `old` is not found, or if found multiple times without `allow_multiple`."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to update (relative to project root or absolute)"
                    },
                    "old": {
                        "type": "string",
                        "description": "The exact text to find in the file. Must match exactly including whitespace and indentation."
                    },
                    "new": {
                        "type": "string",
                        "description": "The text to replace `old` with. Use an empty string to delete the matched text."
                    },
                    "allow_multiple": {
                        "type": "boolean",
                        "description": "If true, replace all occurrences of `old` with `new`. Default: false (fails if multiple matches)."
                    }
                },
                "required": ["path", "old", "new"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileUpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        if args.old.is_empty() {
            return Err(format!("Old string not found in file: {}", args.path));
        }

        let content = std::fs::read_to_string(&args.path).map_err(|e| e.to_string())?;

        let count = content.matches(&args.old).count();

        if count == 0 {
            return Err(format!("Old string not found in file: {}", args.path));
        }

        if count > 1 && !args.allow_multiple {
            return Err(format!(
                "Old string found multiple times in file: {} ({} occurrences). Use `allow_multiple` to replace all.",
                args.path, count
            ));
        }

        let new_content = content.replace(&args.old, &args.new);

        // Record the change for undo before writing
        let _ = undo_history::record_change(
            &args.path,
            Some(content.clone()),
            Some(new_content.clone()),
            "file_update",
        );

        std::fs::write(&args.path, &new_content).map_err(|e| e.to_string())?;

        // Invalidate dedup cache for this path — file content has changed
        {
            let dedup = get_global_tool_dedup();
            let mut guard = dedup.lock().unwrap();
            guard.invalidate_path(&args.path);
        }

        // Build a minimal diff showing the change
        let diff = build_diff(&args.old, &args.new, &content);

        serde_json::to_string(&FileUpdateOutput {
            path: args.path,
            replacements: count,
            diff,
        })
        .map_err(|e| e.to_string())
    }
}

/// Builds a minimal unified-diff-style string showing the replacement.
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
