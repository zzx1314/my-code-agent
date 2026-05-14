use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    /// Git diff showing what changed (None if file is untracked or not in a git repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FileWrite;

#[async_trait::async_trait]
impl Tool for FileWrite {
    fn name(&self) -> &str {
        "file_write"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
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

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileWriteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        if args.create_dirs
            && let Some(parent) = std::path::Path::new(&args.path).parent()
        {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }

        // Use the shared tracking utility: write + undo + dedup invalidation + git diff
        let (bytes_written, git_diff) = super::fs_write_with_tracking(
            &args.path,
            &args.content,
            "file_write",
            None,
        )
        .await?;

        serde_json::to_string(&FileWriteOutput {
            path: args.path,
            bytes_written,
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}
