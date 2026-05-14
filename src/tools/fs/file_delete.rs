use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::context::tool_dedup::get_global_tool_dedup;
use crate::tools::exec::confirmation::ConfirmationHandle;
use crate::tools::exec::safety::{confirm_action, is_dangerous_deletion, is_dangerous_snippet_deletion};
use crate::tools::infra::undo_history;

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

            let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

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

            // Record the change for undo before writing
            let _ = undo_history::record_change(
                &args.path,
                Some(content.clone()),
                Some(new_content.clone()),
                "file_delete (snippet)",
            );

            std::fs::write(path, &new_content).map_err(|e| e.to_string())?;

            // Invalidate dedup cache for this path — file content has changed
            {
                let dedup = get_global_tool_dedup();
                let mut guard = dedup.lock().unwrap();
                guard.invalidate_path(&args.path);
            }

            let diff = super::build_diff(&snippet, "", &content);

            let git_diff = super::run_git_diff(&args.path).await;

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
                std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
                ("directory".to_string(), git_diff)
            } else {
                std::fs::remove_dir(path).map_err(|e| e.to_string())?;
                ("directory".to_string(), None)
            }
        } else if path.is_file() {
            // Run git diff before deletion
            let git_diff = super::run_git_diff(&args.path).await;
            // Record file content for undo before deletion
            let old_content = std::fs::read_to_string(path).ok();
            let _ = undo_history::record_change(&args.path, old_content, None, "file_delete");
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
            ("file".to_string(), git_diff)
        } else {
            return Err(format!("Path is not a file or directory: {}", args.path));
        };

        // Invalidate dedup cache for this path — file has been deleted
        {
            let dedup = get_global_tool_dedup();
            let mut guard = dedup.lock().unwrap();
            guard.invalidate_path(&args.path);
        }

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
