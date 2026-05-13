use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::undo_history::{UndoEntry, current_session_history_len, pop_current_session_entries};

#[derive(Debug, thiserror::Error)]
pub enum FileUndoError {
    #[error("No undo history available")]
    NoHistory,
    #[error("IO error while restoring file {path}: {source}")]
    IoRestore {
        path: String,
        source: std::io::Error,
    },
    #[error("IO error while deleting file {path}: {source}")]
    IoDelete {
        path: String,
        source: std::io::Error,
    },
    #[error("History error: {0}")]
    History(String),
}

#[derive(Deserialize, Serialize)]
pub struct FileUndoArgs {
    /// Number of changes to undo (default: 1).
    #[serde(default = "default_steps")]
    pub steps: usize,
}

fn default_steps() -> usize {
    1
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileUndoOutput {
    /// Number of changes successfully undone.
    pub undone: usize,
    /// Details of each undone change.
    pub details: Vec<UndoDetail>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UndoDetail {
    pub file_path: String,
    pub operation: String,
    pub action: String,
}

#[derive(Debug, Clone, Default)]
pub struct FileUndo;

#[async_trait::async_trait]
impl Tool for FileUndo {
    fn name(&self) -> &str {
        "file_undo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Undo recent file changes made by file_write, file_update, or file_delete. \
                Restores files to their previous state. Use `steps` to specify how many recent changes \
                to undo (default: 1). Changes are undone in reverse chronological order (most recent first)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "integer",
                        "description": "Number of recent changes to undo. Default: 1."
                    }
                },
                "required": []
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileUndoArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let available =
            current_session_history_len().map_err(|e| format!("History error: {}", e))?;
        if available == 0 {
            return Err("No undo history available".to_string());
        }

        // Pop all current session entries (session-scoped undo)
        let mut entries =
            pop_current_session_entries().map_err(|e| format!("History error: {}", e))?;

        // If steps < available, only undo the N most recent
        if args.steps < entries.len() {
            entries.truncate(args.steps);
        }

        let mut details = Vec::new();

        for entry in &entries {
            apply_undo(entry, &mut details).map_err(|e| e.to_string())?;
        }

        serde_json::to_string(&FileUndoOutput {
            undone: entries.len(),
            details,
        })
        .map_err(|e| e.to_string())
    }
}

/// Apply a single undo entry: restore old content or delete the file if it was newly created.
pub fn apply_undo(entry: &UndoEntry, details: &mut Vec<UndoDetail>) -> Result<(), FileUndoError> {
    let path_str = entry.file_path.to_string_lossy().to_string();

    match &entry.old_content {
        Some(old_content) => {
            // File existed before — restore its old content.
            std::fs::write(&entry.file_path, old_content).map_err(|e| {
                FileUndoError::IoRestore {
                    path: path_str.clone(),
                    source: e,
                }
            })?;
            details.push(UndoDetail {
                file_path: path_str,
                operation: entry.operation.clone(),
                action: "restored previous content".to_string(),
            });
        }
        None => {
            // File did not exist before this change — delete it.
            if entry.file_path.exists() {
                std::fs::remove_file(&entry.file_path).map_err(|e| FileUndoError::IoDelete {
                    path: path_str.clone(),
                    source: e,
                })?;
                details.push(UndoDetail {
                    file_path: path_str,
                    operation: entry.operation.clone(),
                    action: "deleted file (was newly created)".to_string(),
                });
            } else {
                details.push(UndoDetail {
                    file_path: path_str,
                    operation: entry.operation.clone(),
                    action: "file already absent, nothing to do".to_string(),
                });
            }
        }
    }

    Ok(())
}
