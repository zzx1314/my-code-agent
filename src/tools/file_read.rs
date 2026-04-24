use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum FileReadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Default number of lines returned when `limit` is not specified.
/// Prevents reading excessively large files into context when the agent
/// doesn't need the entire content.
///
/// This is intentionally lower than the `@filepath` expansion limit (500 lines)
/// because `file_read` is agent-initiated (may read many files in a single turn)
/// while `@filepath` is user-initiated (one explicit attachment at a time).
pub const DEFAULT_READ_LIMIT: usize = 200;

#[derive(Deserialize, Serialize)]
pub struct FileReadArgs {
    pub path: String,
    #[serde(default = "default_offset")]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

fn default_offset() -> Option<usize> {
    None
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileReadOutput {
    pub path: String,
    pub content: String,
    /// Total lines in the file (not just the lines returned).
    pub lines: usize,
    /// Whether the output was truncated because the file exceeds the requested/default limit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FileRead;

impl Tool for FileRead {
    const NAME: &'static str = "file_read";
    type Error = FileReadError;
    type Args = FileReadArgs;
    type Output = FileReadOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the contents of a file from the local filesystem. \
                Returns the file content with line numbers. By default, only the first \
                200 lines are returned (set `limit` to read more, or use `offset` to skip ahead). \
                The output includes the total line count and whether it was truncated, \
                so you can paginate through large files efficiently."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to read (relative to the project root or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Number of lines to skip from the start (0-indexed). Output line numbers are 1-indexed. Default: 0."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read. Default: 200. Increase to read more of a large file."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content = std::fs::read_to_string(&args.path).map_err(FileReadError::Io)?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let offset = args.offset.unwrap_or(0);
        let limit = args.limit.unwrap_or(DEFAULT_READ_LIMIT);

        let start = offset.min(total_lines);
        let end = (start + limit).min(total_lines);
        let truncated = end < total_lines;

        let selected_lines: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6} | {}", start + i + 1, line))
            .collect();

        let mut content = selected_lines.join("\n");
        if truncated {
            let shown = end - start;
            content.push_str(&format!(
                "\n\n... (showing {} of {} total lines. Use offset={} to read more)",
                shown, total_lines, end
            ));
        }

        Ok(FileReadOutput {
            path: args.path,
            content,
            lines: total_lines,
            truncated,
        })
    }
}
