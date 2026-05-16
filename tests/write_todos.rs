use my_code_agent::tools::infra::write_todos::{TodoItem, TodoStatus, WriteTodos, WriteTodosOutput};
use my_code_agent::tools::Tool;
use tempfile::TempDir;

fn make_tool() -> WriteTodos {
    WriteTodos::default()
}

/// Helper: call the tool with the given todos, parse and return the output.
async fn call_tool(todos: Vec<TodoItem>) -> (WriteTodosOutput, String) {
    let args = serde_json::json!({ "todos": todos });
    let result = make_tool().call(args).await.unwrap();
    let output: WriteTodosOutput = serde_json::from_str(&result).unwrap();
    (output, result)
}

/// Helper: build a TodoItem with just task + status.
fn todo(task: &str, status: TodoStatus) -> TodoItem {
    TodoItem {
        id: None,
        task: task.to_string(),
        status,
    }
}

/// Helper: build a TodoItem with task, status, and id.
fn todo_with_id(id: u32, task: &str, status: TodoStatus) -> TodoItem {
    TodoItem {
        id: Some(id),
        task: task.to_string(),
        status,
    }
}

// ── Status-specific tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_pending_status() {
    let todos = vec![todo("Do something", TodoStatus::Pending)];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 1);
    assert!(matches!(output.todos[0].status, TodoStatus::Pending));
    assert_eq!(output.todos[0].task, "Do something");
    assert!(output.message.contains("0/1 completed"));
    assert!(output.message.contains("1 pending"));
}

#[tokio::test]
async fn test_in_progress_status() {
    let todos = vec![todo("Working on it", TodoStatus::InProgress)];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 1);
    assert!(matches!(output.todos[0].status, TodoStatus::InProgress));
    assert!(output.message.contains("1 in progress"));
}

#[tokio::test]
async fn test_completed_status() {
    let todos = vec![todo("Done task", TodoStatus::Completed)];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 1);
    assert!(matches!(output.todos[0].status, TodoStatus::Completed));
    assert!(output.message.contains("1/1 completed"));
}

#[tokio::test]
async fn test_failed_status() {
    let todos = vec![todo("Failed task", TodoStatus::Failed)];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 1);
    assert!(matches!(output.todos[0].status, TodoStatus::Failed));
    assert!(output.message.contains("1 failed"));
}

// ── Mixed statuses ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_mixed_statuses() {
    let todos = vec![
        todo_with_id(1, "Step 1", TodoStatus::Completed),
        todo_with_id(2, "Step 2", TodoStatus::InProgress),
        todo_with_id(3, "Step 3", TodoStatus::Pending),
        todo_with_id(4, "Step 4", TodoStatus::Failed),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 4);

    // Verify each item round-trips correctly
    assert!(matches!(output.todos[0].status, TodoStatus::Completed));
    assert_eq!(output.todos[0].id, Some(1));
    assert!(matches!(output.todos[1].status, TodoStatus::InProgress));
    assert_eq!(output.todos[1].id, Some(2));
    assert!(matches!(output.todos[2].status, TodoStatus::Pending));
    assert_eq!(output.todos[2].id, Some(3));
    assert!(matches!(output.todos[3].status, TodoStatus::Failed));
    assert_eq!(output.todos[3].id, Some(4));

    // Message should mention all counts
    assert!(output.message.contains("1/4 completed"));
    assert!(output.message.contains("1 in progress"));
    assert!(output.message.contains("1 pending"));
    assert!(output.message.contains("1 failed"));
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_todo_list() {
    let todos: Vec<TodoItem> = vec![];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 0);
    assert!(output.message.contains("0/0 completed"));
}

#[tokio::test]
async fn test_with_ids() {
    let todos = vec![
        todo_with_id(10, "First", TodoStatus::Completed),
        todo_with_id(20, "Second", TodoStatus::Pending),
        todo_with_id(30, "Third", TodoStatus::Failed),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos[0].id, Some(10));
    assert_eq!(output.todos[1].id, Some(20));
    assert_eq!(output.todos[2].id, Some(30));
}

#[tokio::test]
async fn test_without_ids() {
    let todos = vec![
        todo("No id 1", TodoStatus::Pending),
        todo("No id 2", TodoStatus::Completed),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos[0].id, None);
    assert_eq!(output.todos[1].id, None);
}

#[tokio::test]
async fn test_all_completed() {
    let todos = vec![
        todo_with_id(1, "Task A", TodoStatus::Completed),
        todo_with_id(2, "Task B", TodoStatus::Completed),
        todo_with_id(3, "Task C", TodoStatus::Completed),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 3);
    assert!(output.message.contains("3/3 completed"));
    // The format always shows all counts (including zero), so "0 pending" is present
    assert!(output.message.contains("0 pending"));
    assert!(output.message.contains("0 in progress"));
    assert!(output.message.contains("0 failed"));
}

#[tokio::test]
async fn test_all_pending() {
    let todos = vec![
        todo("Task 1", TodoStatus::Pending),
        todo("Task 2", TodoStatus::Pending),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 2);
    assert!(output.message.contains("0/2 completed"));
    assert!(output.message.contains("2 pending"));
}

#[tokio::test]
async fn test_single_each_status() {
    let todos = vec![
        todo("pending", TodoStatus::Pending),
        todo("in_progress", TodoStatus::InProgress),
        todo("completed", TodoStatus::Completed),
        todo("failed", TodoStatus::Failed),
    ];
    let (output, _) = call_tool(todos).await;

    assert_eq!(output.todos.len(), 4);
    // Each status appears once; total is 4 items
    assert!(output.message.contains("1/4 completed"));
    assert!(output.message.contains("1 pending"));
    assert!(output.message.contains("1 in progress"));
    assert!(output.message.contains("1 failed"));
}

// ── Default status (backward compatibility) ──────────────────────────────────

#[tokio::test]
async fn test_default_status_is_pending() {
    // Simulate what happens when `status` is not provided in JSON:
    // the serde default kicks in.
    let args = serde_json::json!({
        "todos": [
            { "task": "Default status task" }
        ]
    });
    let result = make_tool().call(args).await.unwrap();
    let output: WriteTodosOutput = serde_json::from_str(&result).unwrap();

    assert_eq!(output.todos.len(), 1);
    assert!(
        matches!(output.todos[0].status, TodoStatus::Pending),
        "default status should be Pending, got {:?}",
        output.todos[0].status
    );
}

// ── .todos.json file is written ──────────────────────────────────────────────

#[tokio::test]
async fn test_todos_json_file_written() {
    // The tool writes .todos.json to the current directory.
    // Use a temp dir to avoid polluting the project root.
    let tmp = TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let todos = vec![
        todo_with_id(1, "File test", TodoStatus::Completed),
        todo_with_id(2, "Another", TodoStatus::Pending),
    ];
    let args = serde_json::json!({ "todos": todos });
    let _result = make_tool().call(args).await.unwrap();

    // Verify .todos.json exists and contains the right data
    let json_path = tmp.path().join(".todos.json");
    assert!(json_path.exists(), ".todos.json should be written");

    let content = std::fs::read_to_string(&json_path).unwrap();
    let written: Vec<TodoItem> = serde_json::from_str(&content).unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].task, "File test");
    assert!(matches!(written[0].status, TodoStatus::Completed));
    assert_eq!(written[0].id, Some(1));
    assert_eq!(written[1].task, "Another");

    // Restore original cwd
    std::env::set_current_dir(original).unwrap();
}

// ── Serialization roundtrip ──────────────────────────────────────────────────

#[tokio::test]
async fn test_json_roundtrip() {
    let todos = vec![
        todo_with_id(42, "Roundtrip", TodoStatus::Completed),
        todo_with_id(99, "Test", TodoStatus::Failed),
    ];
    let (output, raw_json) = call_tool(todos).await;

    // Parse the raw JSON output to verify format
    let parsed: serde_json::Value = serde_json::from_str(&raw_json).unwrap();
    assert!(parsed.get("message").is_some());
    assert!(parsed.get("todos").is_some());

    // Verify the parsed output matches
    assert_eq!(output.todos.len(), 2);
    assert_eq!(output.todos[0].task, "Roundtrip");
    assert_eq!(output.todos[1].task, "Test");
}

// ── TodoStatus serialization format ──────────────────────────────────────────

#[tokio::test]
async fn test_status_serializes_as_snake_case() {
    let todos = vec![
        todo("pending", TodoStatus::Pending),
        todo("in_progress", TodoStatus::InProgress),
        todo("completed", TodoStatus::Completed),
        todo("failed", TodoStatus::Failed),
    ];
    let (output, _) = call_tool(todos).await;

    // The serialized output should use snake_case for status values
    let output_json = serde_json::to_value(&output).unwrap();
    let items = output_json["todos"].as_array().unwrap();
    let statuses: Vec<&str> = items
        .iter()
        .map(|v| v["status"].as_str().unwrap())
        .collect();

    assert_eq!(statuses, vec!["pending", "in_progress", "completed", "failed"]);
}

// ── Error paths ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_invalid_missing_todos() {
    // Missing "todos" field should fail
    let result = make_tool().call(serde_json::json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_invalid_wrong_type() {
    // "todos" should be an array, not a string
    let result = make_tool().call(serde_json::json!({
        "todos": "not an array"
    })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_invalid_status_value() {
    // Invalid status value should fall back to Pending (serde default)
    // since status has #[serde(default)] and is not in "required"
    let result = make_tool().call(serde_json::json!({
        "todos": [
            { "task": "bad status", "status": "invalid_value" }
        ]
    })).await;
    // Invalid enum values cause deserialization error
    assert!(result.is_err(), "invalid status should cause error");
}
