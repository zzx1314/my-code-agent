use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum FileReadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize, Serialize)]
pub struct FileReadArgs {
    pub path: String,
    #[serde(default = "default_offset")]
    pub offset: Option<usize>,
    #[serde(default = "default_limit")]
    pub limit: Option<usize>,
}

fn default_offset() -> Option<usize> {
    None
}

fn default_limit() -> Option<usize> {
    None
}

#[derive(Deserialize, Serialize)]
pub struct FileReadOutput {
    pub path: String,
    pub content: String,
    pub lines: usize,
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
                Returns the file content with line numbers. Use 'offset' and 'limit' \
                to read specific portions of large files."
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
                        "description": "Maximum number of lines to read. Default: read entire file."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content = std::fs::read_to_string(&args.path).map_err(|e| FileReadError::Io(e))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let offset = args.offset.unwrap_or(0);
        let limit = args.limit.unwrap_or(total_lines);

        let start = offset.min(total_lines);
        let end = (start + limit).min(total_lines);

        let selected_lines: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6} | {}", start + i + 1, line))
            .collect();

        Ok(FileReadOutput {
            path: args.path,
            content: selected_lines.join("\n"),
            lines: total_lines,
        })
    }
}
