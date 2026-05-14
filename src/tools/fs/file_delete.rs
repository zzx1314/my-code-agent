use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tools::exec::confirmation::ConfirmationHandle;
use crate::tools::exec::safety::{confirm_action, is_dangerous_deletion, is_dangerous_snippet_deletion};

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
    /// Git diff showing what changed (None if file is untracked or not in a git repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileDelete {
    confirmation_handle: ConfirmationHandle,
}

impl FileDelete {
    pub fn new(confirmation_handle: ConfirmationHandle) -> Self {
        Self {
            confirmation_handle,
        }
    }
}

impl Default for FileDelete {
    fn default() -> Self {
        Self {
            confirmation_handle: ConfirmationHandle::disabled(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for FileDelete {
    fn name(&self) -> &str {
        "file_delete"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
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

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileDeleteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // ── Snippet deletion mode ──
        if let Some(snippet) = args.snippet {
            if snippet.is_empty() {
                return Err(format!("Snippet not found in file: {}", args.path));
            }

            let path = std::path::Path::new(&args.path);

            // Existence/type checks first — no point confirming deletion of a non-existent file
            if !path.exists() {
                return Err(format!("Path not found: {}", args.path));
            }
            if path.is_dir() {
                return Err(format!("Cannot use snippet mode on a directory: {}", args.path));
            }

            // Safety check — skip if auto_approve is set
            if !args.auto_approve
                && let Some(reason) = is_dangerous_snippet_deletion(&args.path)
            {
                let approved = confirm_action(
                    &self.confirmation_handle,
                    reason,
                    &format!("snippet deletion from: {}", args.path),
                )
                .await;
                if !approved {
                    return Err(format!("Action cancelled by user: {}", args.path));
                }
            }

            let content = tokio::fs::read_to_string(path).await.map_err(|e| e.to_string())?;

            let count = content.matches(&snippet).count();

            if count == 0 {
                return Err(format!("Snippet not found in file: {}", args.path));
            }
            if count > 1 && !args.allow_multiple {
                return Err(format!(
                    "Snippet found multiple times in file: {} ({} occurrences). Use `allow_multiple` to delete all.",
                    args.path, count
                ));
            }

            let new_content = content.replace(&snippet, "");

            // Use the shared tracking utility: write + undo + dedup invalidation + git diff.
            // Pass `Some(content)` (the old content) so it doesn't re-read the file.
            let (_, git_diff) = super::fs_write_with_tracking(
                &args.path,
                &new_content,
                "file_delete (snippet)",
                Some(content.clone()),
            )
            .await?;

            let diff = super::build_diff(&snippet, "", &content);

            return serde_json::to_string(&FileDeleteOutput {
                path: args.path,
                deleted_type: "snippet".to_string(),
                deletions: Some(count),
                diff: Some(diff),
                git_diff,
            })
            .map_err(|e| e.to_string());
        }

        // ── Whole file/directory deletion mode ──
        let path = std::path::Path::new(&args.path);

        // Existence check first — no point confirming deletion of a non-existent path
        if !path.exists() {
            return Err(format!("Path not found: {}", args.path));
        }

        // Safety check — skip if auto_approve is set
        if !args.auto_approve
            && let Some(reason) = is_dangerous_deletion(&args.path, args.recursive)
        {
            let detail = if args.recursive {
                format!("recursive deletion of: {}", args.path)
            } else {
                format!("deleting: {}", args.path)
            };
            let approved = confirm_action(&self.confirmation_handle, reason, &detail).await;
            if !approved {
                return Err(format!("Action cancelled by user: {}", args.path));
            }
        }

        let (deleted_type, git_diff) = if path.is_dir() {
            if args.recursive {
                let git_diff = super::run_git_diff(&args.path).await;
                tokio::fs::remove_dir_all(path).await.map_err(|e| e.to_string())?;
                super::invalidate_dedup_cache(&args.path);
                ("directory".to_string(), git_diff)
            } else {
                tokio::fs::remove_dir(path).await.map_err(|e| e.to_string())?;
                super::invalidate_dedup_cache(&args.path);
                ("directory".to_string(), None)
            }
        } else if path.is_file() {
            // Use the shared deletion tracking utility: git diff + undo + delete + dedup invalidation
            let git_diff = super::fs_delete_with_tracking(&args.path).await?;
            ("file".to_string(), git_diff)
        } else {
            return Err(format!("Path is not a file or directory: {}", args.path));
        };

        serde_json::to_string(&FileDeleteOutput {
            path: args.path,
            deleted_type,
            deletions: None,
            diff: None,
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}
