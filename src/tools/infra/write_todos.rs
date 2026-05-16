//! Tool for writing a todo list JSON file to `.mycode/.todos.json` to
//! track multi-step task progress. The `.mycode` directory is created
//! automatically with restricted permissions on first write.

use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Path (relative to project root) where the todos JSON file is stored.
pub const TODOS_FILE_PATH: &str = ".mycode/.todos.json";

/// Directory that contains the todos file.
const TODOS_DIR: &str = ".mycode";

/// The status of a todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Task is not yet started.
    Pending,
    /// Task is currently being worked on.
    InProgress,
    /// Task is completed successfully.
    Completed,
    /// Task has failed or encountered an error.
    Failed,
}

impl Default for TodoStatus {
    fn default() -> Self {
        TodoStatus::Pending
    }
}

#[derive(Debug, Deserialize)]
pub struct WriteTodosArgs {
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique, stable identifier for tracking and logging purposes.
    /// Assigned by the agent (e.g. 1, 2, 3...) and stable across rewrite calls.
    pub id: Option<u32>,
    pub task: String,
    /// Current status of the task. Defaults to "pending" if not provided.
    #[serde(default)]
    pub status: TodoStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteTodosOutput {
    pub message: String,
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Default)]
pub struct WriteTodos;
/// Ensure the `.mycode` directory exists. Safe to call repeatedly.
fn ensure_todos_dir() -> Result<(), String> {
    std::fs::create_dir_all(TODOS_DIR)
        .map_err(|e| format!("Failed to create {} directory: {}", TODOS_DIR, e))
}
struct TodoStats {
    completed: usize,
    pending: usize,
    in_progress: usize,
    failed: usize,
}

fn compute_stats(todos: &[TodoItem]) -> TodoStats {
    let mut s = TodoStats { completed: 0, pending: 0, in_progress: 0, failed: 0 };
    for t in todos {
        match t.status {
            TodoStatus::Completed => s.completed += 1,
            TodoStatus::Pending => s.pending += 1,
            TodoStatus::InProgress => s.in_progress += 1,
            TodoStatus::Failed => s.failed += 1,
        }
    }
    s
}

#[async_trait::async_trait]
impl Tool for WriteTodos {
    fn name(&self) -> &str {
        "write_todos"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Write a todo list to track tasks for multi-step implementations. \
                 Call this after gathering context to plan steps, and after completing each step \
                 to update progress. Rewrite ALL todos each time with current status."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "integer",
                                    "description": "Stable unique identifier for tracking (1, 2, 3...)"
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Description of the task"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed", "failed"],
                                    "description": "Current status of the task. One of: pending, in_progress, completed, failed. Default: pending"
                                }
                            },
                            "required": ["task"]
                        },
                        "description": "List of todos with completion status. Rewrite ALL todos each call."
                    }
                },
                "required": ["todos"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: WriteTodosArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let content = serde_json::to_string_pretty(&args.todos)
            .map_err(|e| format!("Failed to serialize todos: {}", e))?;

        ensure_todos_dir()?;
        std::fs::write(TODOS_FILE_PATH, &content)
            .map_err(|e| format!("Failed to write {}: {}", TODOS_FILE_PATH, e))?;

        let stats = compute_stats(&args.todos);
        let total = args.todos.len();

        let output = WriteTodosOutput {
            message: format!(
                "🔄 Todos: {}/{} completed · {} pending · {} in progress · {} failed",
                stats.completed, total, stats.pending, stats.in_progress, stats.failed,
            ),
            todos: args.todos,
        };

        serde_json::to_string(&output).map_err(|e| e.to_string())
    }
}
