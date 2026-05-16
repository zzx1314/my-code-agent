use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct WriteTodosArgs {
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub task: String,
    pub completed: bool,
}

#[derive(Debug, Serialize)]
pub struct WriteTodosOutput {
    pub message: String,
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Default)]
pub struct WriteTodos;

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
                                "task": {
                                    "type": "string",
                                    "description": "Description of the task"
                                },
                                "completed": {
                                    "type": "boolean",
                                    "description": "Whether the task is completed"
                                }
                            },
                            "required": ["task", "completed"]
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

        let completed_count = args.todos.iter().filter(|t| t.completed).count();
        let total_count = args.todos.len();

        let content = serde_json::to_string_pretty(&args.todos)
            .map_err(|e| format!("Failed to serialize todos: {}", e))?;
        std::fs::write(".todos.json", content)
            .map_err(|e| format!("Failed to write .todos.json: {}", e))?;

        let output = WriteTodosOutput {
            message: format!(
                "Todos updated: {}/{} completed",
                completed_count, total_count
            ),
            todos: args.todos,
        };

        serde_json::to_string(&output).map_err(|e| e.to_string())
    }
}
