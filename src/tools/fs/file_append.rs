use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct FileAppendArgs {
    pub path: String,
    /// Content to append to the file
    pub content: String,
    #[serde(default)]
    pub create_dirs: bool,
}

#[derive(Debug, Serialize)]
pub struct FileAppendOutput {
    pub path: String,
    /// Number of bytes appended to the file
    pub bytes_appended: usize,
    /// Git diff showing what changed (None if file is untracked or not in a git repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FileAppend;

#[async_trait::async_trait]
impl Tool for FileAppend {
    fn name(&self) -> &str {
        "file_append"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Append content to the end of a file. If the file does not exist, it will be created. \
                 Use this tool to add content to files incrementally — especially useful for writing \
                 large files in multiple small chunks to avoid tool call argument truncation. \
                 See file_write for the recommended large-file workflow. \
                 The content is always added to the end of the file; use file_update for inserting \
                 content at specific line positions or replacing existing lines. \
                 **Important**: Set create_dirs to true when the target directory might not exist, \
                 otherwise the tool will fail with 'No such file or directory'."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to append to (relative to project root or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to append to the end of the file. Keep each chunk under ~2000 characters to avoid tool call truncation."
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
        let args: FileAppendArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // Read current content (None if file doesn't exist)
        let old_content = tokio::fs::read_to_string(&args.path).await.ok();

        // If file doesn't exist and create_dirs is set, create parent directories
        if old_content.is_none() && args.create_dirs {
            if let Some(parent) = std::path::Path::new(&args.path).parent() {
                let parent = parent.as_os_str();
                if !parent.is_empty() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        }

        // Compute new content (append to existing, or create new file)
        let new_content = match &old_content {
            Some(existing) => format!("{}{}", existing, args.content),
            None => args.content.clone(),
        };

        let bytes_appended = args.content.len();

        // Use the shared tracking utility: write + undo + dedup invalidation + git diff
        let (_, git_diff) = super::fs_write_with_tracking(
            &args.path,
            &new_content,
            "file_append",
            old_content,
        )
        .await?;

        serde_json::to_string(&FileAppendOutput {
            path: args.path,
            bytes_appended,
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}
