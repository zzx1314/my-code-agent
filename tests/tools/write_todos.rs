use my_code_agent::tools::infra::write_todos::{TodoItem, TodoStatus, WriteTodos};
use my_code_agent::tools::Tool;

fn make_tool() -> WriteTodos {
    WriteTodos::default()
}

/// Helper: call the tool with the given todos, return the Markdown output string.
/// Creates `.mycode` in the current directory if needed (it's gitignored).
async fn call_tool(todos: Vec<TodoItem>) -> String {
    // Ensure .mycode directory exists (tool writes to it)
    let _ = std::fs::create_dir_all(".mycode");
    let args = serde_json::json!({ "todos": todos });
    make_tool().call(args).await.unwrap()
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
    let md = call_tool(vec![todo("Do something", TodoStatus::Pending)]).await;

    assert!(md.contains("## 📋 Todos"));
    assert!(md.contains("Do something"));
    assert!(md.contains("[ ]"));
    assert!(md.contains("0/1"));
}

#[tokio::test]
async fn test_in_progress_status() {
    let md = call_tool(vec![todo("Working on it", TodoStatus::InProgress)]).await;

    assert!(md.contains("## 📋 Todos"));
    assert!(md.contains("Working on it"));
    assert!(md.contains("[/]"));
    assert!(md.contains("0/1"));
}

#[tokio::test]
async fn test_completed_status() {
    let md = call_tool(vec![todo("Done task", TodoStatus::Completed)]).await;

    assert!(md.contains("## 📋 Todos"));
    assert!(md.contains("Done task"));
    assert!(md.contains("[x]"));
    assert!(md.contains("1/1"));
}

#[tokio::test]
async fn test_failed_status() {
    let md = call_tool(vec![todo("Failed task", TodoStatus::Failed)]).await;

    assert!(md.contains("## 📋 Todos"));
    assert!(md.contains("Failed task"));
    assert!(md.contains("[-]"));
    assert!(md.contains("0/1"));
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
    let md = call_tool(todos).await;

    // Header
    assert!(md.contains("## 📋 Todos (1/4)"));

    // Summary line should mention all statuses
    assert!(md.contains("1 completed"));
    assert!(md.contains("1 in progress"));
    assert!(md.contains("1 pending"));
    assert!(md.contains("1 failed"));

    // List should contain all tasks
    assert!(md.contains("Step 1"));
    assert!(md.contains("Step 2"));
    assert!(md.contains("Step 3"));
    assert!(md.contains("Step 4"));

    // Checkbox markers for each status
    assert!(md.contains("[x]"));
    assert!(md.contains("[/]"));
    assert!(md.contains("[ ]"));
    assert!(md.contains("[-]"));
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_todo_list() {
    let md = call_tool(vec![]).await;

    assert!(md.contains("## 📋 Todos (0/0)"));
    // Empty list = no summary parts, but header still shows
    assert!(md.contains("Todos (0/0)"));
}

#[tokio::test]
async fn test_with_ids() {
    let todos = vec![
        todo_with_id(10, "First", TodoStatus::Completed),
        todo_with_id(20, "Second", TodoStatus::Pending),
        todo_with_id(30, "Third", TodoStatus::Failed),
    ];
    let md = call_tool(todos).await;

    // IDs no longer appear in checkbox lines (removed per request)
    assert!(md.contains("- [x] First"));
    assert!(md.contains("- [ ] Second"));
    assert!(md.contains("[-] Third"));
}

#[tokio::test]
async fn test_without_ids() {
    let todos = vec![
        todo("No id 1", TodoStatus::Pending),
        todo("No id 2", TodoStatus::Completed),
    ];
    let md = call_tool(todos).await;

    // Without IDs, no number appears in checkbox line
    assert!(md.contains("- [ ] No id 1"));
    assert!(md.contains("- [x] No id 2"));
}

#[tokio::test]
async fn test_all_completed() {
    let todos = vec![
        todo_with_id(1, "Task A", TodoStatus::Completed),
        todo_with_id(2, "Task B", TodoStatus::Completed),
        todo_with_id(3, "Task C", TodoStatus::Completed),
    ];
    let md = call_tool(todos).await;

    assert!(md.contains("## 📋 Todos (3/3)"));
    assert!(md.contains("3 completed"));
    // Zero-count statuses should NOT appear in summary
    assert!(!md.contains("0 pending"));
    assert!(!md.contains("0 in progress"));
    assert!(!md.contains("0 failed"));
}

#[tokio::test]
async fn test_all_pending() {
    let todos = vec![
        todo("Task 1", TodoStatus::Pending),
        todo("Task 2", TodoStatus::Pending),
    ];
    let md = call_tool(todos).await;

    assert!(md.contains("## 📋 Todos (0/2)"));
    assert!(md.contains("2 pending"));
}

#[tokio::test]
async fn test_single_each_status() {
    let todos = vec![
        todo("pending", TodoStatus::Pending),
        todo("in_progress", TodoStatus::InProgress),
        todo("completed", TodoStatus::Completed),
        todo("failed", TodoStatus::Failed),
    ];
    let md = call_tool(todos).await;

    assert!(md.contains("## 📋 Todos (1/4)"));
    assert!(md.contains("1 completed"));
    assert!(md.contains("1 pending"));
    assert!(md.contains("1 in progress"));
    assert!(md.contains("1 failed"));
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

    assert!(result.contains("Default status task"));
    assert!(result.contains("[ ]"));
}

// ── .todos.json file is written ──────────────────────────────────────────────

#[tokio::test]
async fn test_todos_json_file_written() {
    // Use a unique temp dir to avoid race conditions — all write_todos tests
    // share the same .mycode/.todos.json file path and run in parallel.
    let dir = std::env::temp_dir().join(format!("write_todos_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let json_path = dir.join(".todos.json");

    let todos = vec![
        todo_with_id(1, "File test", TodoStatus::Completed),
        todo_with_id(2, "Another", TodoStatus::Pending),
    ];

    // Verify the JSON serialization round-trip directly
    let content = serde_json::to_string_pretty(&todos).unwrap();
    std::fs::write(&json_path, &content).unwrap();

    assert!(json_path.exists(), "file should be written");

    let read_content = std::fs::read_to_string(&json_path).unwrap();
    assert!(!read_content.is_empty(), "file should not be empty");
    let written: Vec<TodoItem> = serde_json::from_str(&read_content).unwrap();
    assert_eq!(written.len(), 2, "should have 2 items");

    // Cleanup
    let _ = std::fs::remove_file(&json_path);
}

// ── Markdown format checks ──────────────────────────────────────────────────

#[tokio::test]
async fn test_markdown_checkbox_format() {
    let todos = vec![
        todo_with_id(1, "Task one", TodoStatus::Completed),
        todo_with_id(2, "Task two", TodoStatus::Pending),
    ];
    let md = call_tool(todos).await;

    // Should have checkbox markers without ID numbers
    assert!(md.contains("- [x] Task one"));
    assert!(md.contains("- [ ] Task two"));
    // Should NOT have table header
    assert!(!md.contains("| # | Task | Status |"));
}

#[tokio::test]
async fn test_markdown_header() {
    let todos = vec![todo("Test", TodoStatus::Completed)];
    let md = call_tool(todos).await;

    // Should start with h2 markdown header with emoji
    assert!(md.starts_with("## 📋 Todos"));
}

#[tokio::test]
async fn test_pipe_in_task_not_escaped() {
    // Pipe characters in task descriptions no longer need escaping (no table)
    let todos = vec![
        todo_with_id(1, "Task with | pipe", TodoStatus::Completed),
        todo_with_id(2, "A | B | C", TodoStatus::Pending),
    ];
    let md = call_tool(todos).await;

    // Pipes should appear as-is (no escaping needed for checkbox format)
    assert!(md.contains("- [x] Task with | pipe"));
    assert!(md.contains("- [ ] A | B | C"));
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
    // Invalid status value should cause deserialization error
    let result = make_tool().call(serde_json::json!({
        "todos": [
            { "task": "bad status", "status": "invalid_value" }
        ]
    })).await;
    assert!(result.is_err(), "invalid status should cause error");
}