use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct ProposeStrReplaceArgs {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub allow_multiple: bool,
}

#[derive(Debug, Serialize)]
pub struct ProposeStrReplaceOutput {
    pub path: String,
    pub diff: String,
    pub lines_added: usize,
    pub lines_removed: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ProposeStrReplace;

#[async_trait::async_trait]
impl Tool for ProposeStrReplace {
    fn name(&self) -> &str {
        "propose_str_replace"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Preview a string replacement without writing to disk. Returns the diff \
                 that would result from the change. Use this to verify changes before \
                 applying them. Works identically to str_replace but does NOT modify the file."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Exact string to find (must match exactly)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "String to replace old_string with"
                    },
                    "allow_multiple": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default: false)",
                        "default": false
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: ProposeStrReplaceArgs =
            serde_json::from_value(args).map_err(|e| e.to_string())?;

        let content =
            tokio::fs::read_to_string(&args.path).await.map_err(|e| e.to_string())?;

        let count = content.matches(&args.old_string).count();

        if count == 0 {
            return Err(format!("old_string not found in {}", args.path));
        }

        if count > 1 && !args.allow_multiple {
            return Err(format!(
                "Found {} occurrences of old_string in {}. \
                 Set allow_multiple=true to replace all occurrences.",
                count, args.path
            ));
        }

        let new_content = if args.allow_multiple {
            content.replace(&args.old_string, &args.new_string)
        } else {
            content.replacen(&args.old_string, &args.new_string, 1)
        };

        let diff = generate_diff(&content, &new_content, &args.path);

        let old_lines = content.lines().count();
        let new_lines = new_content.lines().count();
        let lines_added = if new_lines > old_lines { new_lines - old_lines } else { 0 };
        let lines_removed = if old_lines > new_lines { old_lines - new_lines } else { 0 };

        serde_json::to_string(&ProposeStrReplaceOutput {
            path: args.path,
            diff,
            lines_added,
            lines_removed,
        })
        .map_err(|e| e.to_string())
    }
}

/// Generate a minimal unified-diff string between old and new content.
///
/// Output format:
/// ```text
/// --- a/path
/// +++ b/path
/// @@ -start,count +start,count @@
///  context
/// -removed line
/// +added line
///  context
/// ```
fn generate_diff(old_content: &str, new_content: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let old_len = old_lines.len();
    let new_len = new_lines.len();

    let start = old_lines
        .iter()
        .zip(new_lines.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(old_len.min(new_len));

    let suffix_match = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let old_end = old_len - suffix_match;
    let new_end = new_len - suffix_match;

    if start >= old_end && start >= new_end {
        let line = content_line_at(old_content, 0).min(old_len.saturating_sub(1));
        return minimal_hdiff(old_content, new_content, path, line);
    }

    const CTX: usize = 3;
    let hunk_start = start.saturating_sub(CTX);
    let hunk_old_end = (old_end + CTX).min(old_len).max(hunk_start);
    let hunk_new_end = (new_end + CTX).min(new_len).max(hunk_start);

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{path}\n"));
    diff.push_str(&format!("+++ b/{path}\n"));
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        hunk_start + 1,
        hunk_old_end - hunk_start,
        hunk_start + 1,
        hunk_new_end - hunk_start,
    ));

    for i in hunk_start..start {
        diff.push_str(&format!(" {}\n", old_lines[i]));
    }
    for i in start..old_end {
        diff.push_str(&format!("-{}\n", old_lines[i]));
    }
    for i in start..new_end {
        diff.push_str(&format!("+{}\n", new_lines[i]));
    }
    for i in old_end..hunk_old_end {
        diff.push_str(&format!(" {}\n", old_lines[i]));
    }

    diff
}

fn content_line_at(content: &str, byte_offset: usize) -> usize {
    content[..byte_offset.min(content.len())].lines().count().saturating_sub(1)
}

fn minimal_hdiff(old_content: &str, new_content: &str, path: &str, line: usize) -> String {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();
    let line = line.min(old_lines.len().saturating_sub(1)).min(new_lines.len().saturating_sub(1));

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{path}\n"));
    diff.push_str(&format!("+++ b/{path}\n"));
    diff.push_str(&format!("@@ -{},1 +{},1 @@\n", line + 1, line + 1));
    if line < old_lines.len() {
        diff.push_str(&format!("-{}\n", old_lines[line]));
    }
    if line < new_lines.len() {
        diff.push_str(&format!("+{}\n", new_lines[line]));
    }
    diff
}
