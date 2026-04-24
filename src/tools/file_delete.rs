use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::safety::{confirm_action, is_dangerous_deletion, is_dangerous_snippet_deletion};

#[derive(Debug, thiserror::Error)]
pub enum FileDeleteError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Path not found: {path}")]
    NotFound { path: String },
    #[error("Path is not a file or directory: {path}")]
    InvalidType { path: String },
    #[error("Snippet not found in file: {path}")]
    SnippetNotFound { path: String },
    #[error("Snippet found multiple times in file: {path} ({count} occurrences). Use `allow_multiple` to delete all.")]
    SnippetMultipleMatches { path: String, count: usize },
    #[error("Cannot use snippet mode on a directory: {path}")]
    SnippetOnDirectory { path: String },
    #[error("Action cancelled by user: {path}")]
    Cancelled { path: String },
}

#[derive(Deserialize, Serialize)]
pub struct FileDeleteArgs {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    /// If provided, delete this text snippet from the file instead of deleting the entire file.
    #[serde(default)]
    pub snippet: Option<String>,
    /// If true, delete all occurrences of `snippet` in the file. Default: false (fails if snippet appears multiple times).
    #[serde(default)]
    pub allow_multiple: bool,
    /// If true, skip the safety confirmation prompt for dangerous deletions.
    /// Should only be set by the user, never by the agent.
    #[serde(default)]
    pub auto_approve: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileDeleteOutput {
    pub path: String,
    /// "file", "directory", or "snippet"
    pub deleted_type: String,
    /// Number of snippet occurrences removed (only for snippet mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<usize>,
    /// Diff showing what was removed (only for snippet mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FileDelete;

impl Tool for FileDelete {
    const NAME: &'static str = "file_delete";
    type Error = FileDeleteError;
    type Args = FileDeleteArgs;
    type Output = FileDeleteOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Delete a file, directory, or a specific text snippet from a file. \
                When `snippet` is provided, the tool removes that exact text from the file \
                (like file_update with an empty replacement, but clearer intent). \
                Without `snippet`, deletes the entire file or directory. \
                For directories, set `recursive` to true to delete all contents. \
                Use with caution — deletions cannot be undone. \
                Always confirm with the user before deleting unless explicitly asked."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file or directory (relative to project root or absolute)"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "If true, delete directories and all their contents recursively. Required for non-empty directories. Default: false."
                    },
                    "snippet": {
                        "type": "string",
                        "description": "If provided, delete this exact text from the file instead of deleting the entire file. Must match exactly including whitespace and indentation."
                    },
                    "allow_multiple": {
                        "type": "boolean",
                        "description": "If true, delete all occurrences of `snippet` in the file. Default: false (fails if snippet appears multiple times)."
                    },
                    "auto_approve": {
                        "type": "boolean",
                        "description": "If true, skip the safety confirmation for dangerous deletions. Only set this if you are confident the deletion is safe. Default: false."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // ── Snippet deletion mode ──
        if let Some(snippet) = args.snippet {
            if snippet.is_empty() {
                return Err(FileDeleteError::SnippetNotFound { path: args.path });
            }

            let path = std::path::Path::new(&args.path);

            // Existence/type checks first — no point confirming deletion of a non-existent file
            if !path.exists() {
                return Err(FileDeleteError::NotFound { path: args.path });
            }
            if path.is_dir() {
                return Err(FileDeleteError::SnippetOnDirectory { path: args.path });
            }

            // Safety check — skip if auto_approve is set
            if !args.auto_approve && let Some(reason) = is_dangerous_snippet_deletion(&args.path) {
                let approved = confirm_action(
                    reason,
                    &format!("snippet deletion from: {}", args.path),
                )
                .await;
                if !approved {
                    return Err(FileDeleteError::Cancelled { path: args.path });
                }
            }

            let content = std::fs::read_to_string(path).map_err(FileDeleteError::Io)?;

            let count = content.matches(&snippet).count();

            if count == 0 {
                return Err(FileDeleteError::SnippetNotFound { path: args.path });
            }
            if count > 1 && !args.allow_multiple {
                return Err(FileDeleteError::SnippetMultipleMatches {
                    path: args.path,
                    count,
                });
            }

            let new_content = content.replace(&snippet, "");
            std::fs::write(path, &new_content).map_err(FileDeleteError::Io)?;

            let diff = super::build_diff(&snippet, "", &content);

            return Ok(FileDeleteOutput {
                path: args.path,
                deleted_type: "snippet".to_string(),
                deletions: Some(count),
                diff: Some(diff),
            });
        }

        // ── Whole file/directory deletion mode ──
        let path = std::path::Path::new(&args.path);

        // Existence check first — no point confirming deletion of a non-existent path
        if !path.exists() {
            return Err(FileDeleteError::NotFound { path: args.path });
        }

        // Safety check — skip if auto_approve is set
        if !args.auto_approve && let Some(reason) = is_dangerous_deletion(&args.path, args.recursive) {
            let detail = if args.recursive {
                format!("recursive deletion of: {}", args.path)
            } else {
                format!("deleting: {}", args.path)
            };
            let approved = confirm_action(reason, &detail).await;
            if !approved {
                return Err(FileDeleteError::Cancelled { path: args.path });
            }
        }

        let deleted_type = if path.is_dir() {
            if args.recursive {
                std::fs::remove_dir_all(path).map_err(FileDeleteError::Io)?;
                "directory".to_string()
            } else {
                std::fs::remove_dir(path).map_err(FileDeleteError::Io)?;
                "directory".to_string()
            }
        } else if path.is_file() {
            std::fs::remove_file(path).map_err(FileDeleteError::Io)?;
            "file".to_string()
        } else {
            return Err(FileDeleteError::InvalidType { path: args.path });
        };

        Ok(FileDeleteOutput {
            path: args.path,
            deleted_type,
            deletions: None,
            diff: None,
        })
    }
}
