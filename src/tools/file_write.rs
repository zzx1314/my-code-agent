use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::tool_dedup::get_global_tool_dedup;
use super::undo_history;

#[derive(Debug, thiserror::Error)]
pub enum FileWriteError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize, Serialize)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub create_dirs: bool,
}

#[derive(Deserialize, Serialize)]
pub struct FileWriteOutput {
    pub path: String,
    pub bytes_written: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FileWrite;

impl Tool for FileWrite {
    const NAME: &'static str = "file_write";
    type Error = FileWriteError;
    type Args = FileWriteArgs;
    type Output = FileWriteOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write content to a file on the local filesystem. \
                Creates the file if it doesn't exist, overwrites if it does. \
                Set create_dirs to true to create parent directories automatically."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to write (relative to project root or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    },
                    "create_dirs": {
                        "type": "boolean",
                        "description": "Whether to create parent directories if they don't exist. Default: false."
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.create_dirs
            && let Some(parent) = std::path::Path::new(&args.path).parent()
        {
            std::fs::create_dir_all(parent).map_err(FileWriteError::Io)?;
        }

        // Record the change for undo before writing
        let old_content = std::fs::read_to_string(&args.path).ok();
        let _ = undo_history::record_change(
            &args.path,
            old_content,
            Some(args.content.clone()),
            "file_write",
        );

        let bytes_written = args.content.len();
        std::fs::write(&args.path, &args.content).map_err(FileWriteError::Io)?;

        // Invalidate dedup cache for this path — file content has changed
        {
            let dedup = get_global_tool_dedup();
            let mut guard = dedup.lock().unwrap();
            guard.invalidate_path(&args.path);
        }

        Ok(FileWriteOutput {
            path: args.path,
            bytes_written,
        })
    }
}
