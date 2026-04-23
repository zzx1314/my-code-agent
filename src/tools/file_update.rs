use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum FileUpdateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Old string not found in file: {path}")]
    NotFound { path: String },
    #[error("Old string found multiple times in file: {path} ({count} occurrences). Use `allow_multiple` to replace all.")]
    MultipleMatches { path: String, count: usize },
}

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

impl Tool for FileUpdate {
    const NAME: &'static str = "file_update";
    type Error = FileUpdateError;
    type Args = FileUpdateArgs;
    type Output = FileUpdateOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.old.is_empty() {
            return Err(FileUpdateError::NotFound { path: args.path });
        }

        let content = std::fs::read_to_string(&args.path).map_err(FileUpdateError::Io)?;

        let count = content.matches(&args.old).count();

        if count == 0 {
            return Err(FileUpdateError::NotFound { path: args.path });
        }

        if count > 1 && !args.allow_multiple {
            return Err(FileUpdateError::MultipleMatches {
                path: args.path,
                count,
            });
        }

        let new_content = content.replace(&args.old, &args.new);
        std::fs::write(&args.path, &new_content).map_err(FileUpdateError::Io)?;

        // Build a minimal diff showing the change
        let diff = build_diff(&args.old, &args.new, &content);

        Ok(FileUpdateOutput {
            path: args.path,
            replacements: count,
            diff,
        })
    }
}

/// Builds a minimal unified-diff-style string showing the replacement.
fn build_diff(old: &str, new: &str, content: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_diff() {
        let content = "fn hello() {\n    println!(\"hi\");\n}\n";
        let diff = build_diff("println!(\"hi\");", "println!(\"hello\");", content);
        assert!(diff.contains("@@ line 2 @@"));
        assert!(diff.contains("-println!(\"hi\");"));
        assert!(diff.contains("+println!(\"hello\");"));
    }

    #[test]
    fn test_build_diff_multiline() {
        let content = "fn foo() {\n    let x = 1;\n    let y = 2;\n}\n";
        let old = "    let x = 1;\n    let y = 2;";
        let new = "    let x = 3;\n    let y = 4;";
        let diff = build_diff(old, new, content);
        assert!(diff.contains("@@ line 2 @@"));
        assert!(diff.contains("-    let x = 1;"));
        assert!(diff.contains("+    let x = 3;"));
    }
}
