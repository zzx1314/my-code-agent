use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::undo_history::{pop_last_entries, history_len, UndoEntry};

#[derive(Debug, thiserror::Error)]
pub enum FileUndoError {
    #[error("No undo history available")]
    NoHistory,
    #[error("Cannot undo {requested} steps, only {available} changes recorded")]
    TooManySteps { requested: usize, available: usize },
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

impl Tool for FileUndo {
    const NAME: &'static str = "file_undo";
    type Error = FileUndoError;
    type Args = FileUndoArgs;
    type Output = FileUndoOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let available = history_len().map_err(|e| FileUndoError::History(e.to_string()))?;
        if available == 0 {
            return Err(FileUndoError::NoHistory);
        }
        if args.steps > available {
            return Err(FileUndoError::TooManySteps {
                requested: args.steps,
                available,
            });
        }

        let entries = pop_last_entries(args.steps).map_err(|e| FileUndoError::History(e.to_string()))?;
        let mut details = Vec::new();

        for entry in &entries {
            apply_undo(entry, &mut details)?;
        }

        Ok(FileUndoOutput {
            undone: entries.len(),
            details,
        })
    }
}

/// Apply a single undo entry: restore old content or delete the file if it was newly created.
pub fn apply_undo(entry: &UndoEntry, details: &mut Vec<UndoDetail>) -> Result<(), FileUndoError> {
    let path_str = entry.file_path.to_string_lossy().to_string();

    match &entry.old_content {
        Some(old_content) => {
            // File existed before — restore its old content.
            std::fs::write(&entry.file_path, old_content).map_err(|e| FileUndoError::IoRestore {
                path: path_str.clone(),
                source: e,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_undo_write_new_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        // Simulate a file_write that created a new file
        super::super::undo_history::record_change(
            &file_path.to_string_lossy(),
            None,
            Some("hello".to_string()),
            "file_write",
        )
        .unwrap();

        // Write the file (simulating the actual write)
        std::fs::write(&file_path, "hello").unwrap();
        assert!(file_path.exists());

        // Now undo
        let entries = pop_last_entries(1).unwrap();
        assert_eq!(entries.len(), 1);
        let mut details = Vec::new();
        apply_undo(&entries[0], &mut details).unwrap();

        // File should be gone
        assert!(!file_path.exists());
        assert_eq!(details[0].action, "deleted file (was newly created)");
    }

    #[test]
    fn test_undo_update_existing_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        // Write initial content
        std::fs::write(&file_path, "old content").unwrap();

        // Simulate a file_update
        super::super::undo_history::record_change(
            &file_path.to_string_lossy(),
            Some("old content".to_string()),
            Some("new content".to_string()),
            "file_update",
        )
        .unwrap();

        // Actually update
        std::fs::write(&file_path, "new content").unwrap();

        // Undo
        let entries = pop_last_entries(1).unwrap();
        let mut details = Vec::new();
        apply_undo(&entries[0], &mut details).unwrap();

        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "old content");
        assert_eq!(details[0].action, "restored previous content");
    }
}
