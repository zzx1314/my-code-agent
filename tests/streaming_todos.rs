use my_code_agent::app::App;
use my_code_agent::core::agent::client::LlmClient;
use my_code_agent::core::agent::preamble::{Agent, build_preamble};
use my_code_agent::core::agent::stream::{cleanup_stream_state, process_streaming_events, reset_streaming_state};
use my_code_agent::core::agent::stream_response::StreamEvent;
use my_code_agent::core::config::Config;
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::mpsc;

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Build a minimal App for testing (dummy client, no API key needed).
fn make_app(event_rx: mpsc::UnboundedReceiver<StreamEvent>) -> App {
    let config = Config::default();
    let client = LlmClient::new("http://localhost:9999", "", "test-model");
    let system_prompt = build_preamble();
    let tools = ToolRegistry::from_config(&config);
    let agent = Arc::new(Agent::new(client, system_prompt, tools));
    let (interrupt_tx, _) = tokio::sync::broadcast::channel(1);
    let token_usage = TokenUsage::with_config(&config);
    let mut app = App::new(
        vec![],
        token_usage,
        String::new(),
        config,
        agent,
        interrupt_tx,
    );
    app.is_streaming = true;
    app.streaming_events_rx = Some(event_rx);
    app
}

/// Sample todo content in the Markdown format returned by write_todos.
fn sample_todo(completed: usize, total: usize) -> String {
    format!(
        "## 📋 Todos ({}/{})\n\n{} pending\n\n- [ ] Step 1\n- [ ] Step 2\n- [ ] Step 3\n",
        completed, total, total - completed
    )
}

/// Sample non-todo tool result content.
fn sample_non_todo() -> String {
    r#"{"path":"src/main.rs","git_diff":"+fn hello()"}"#.to_string()
}

// ── Tests: streaming_todos is set on tool result ─────────────────────────────

#[test]
fn test_todo_tool_result_sets_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Send a ToolResult with todo content
    let content = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content: content.clone(),
    })
    .unwrap();

    process_streaming_events(&mut app);

    assert_eq!(
        app.streaming_todos.as_deref(),
        Some(content.as_str()),
        "streaming_todos should be set when tool result contains todo content"
    );
    // streaming_tool_result should also be set (the rendering uses both)
    assert_eq!(
        app.streaming_tool_result.as_ref().map(|(_, c)| c.as_str()),
        Some(content.as_str()),
        "streaming_tool_result should also be set"
    );
}

#[test]
fn test_non_todo_tool_result_does_not_set_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Send a ToolResult with non-todo content (e.g., file write result)
    let content = sample_non_todo();
    tx.send(StreamEvent::ToolResult {
        name: "file_write".to_string(),
        content: content.clone(),
    })
    .unwrap();

    process_streaming_events(&mut app);

    assert!(
        app.streaming_todos.is_none(),
        "streaming_todos should NOT be set for non-todo tool results"
    );
    // streaming_tool_result should still be set
    assert!(
        app.streaming_tool_result.is_some(),
        "streaming_tool_result should still be set for non-todo results"
    );
}

// ── Tests: streaming_todos is cleared on new text ────────────────────────────

#[test]
fn test_new_text_clears_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // First, set streaming_todos by sending a tool result
    let content = sample_todo(1, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Now send streaming text (model responds)
    tx.send(StreamEvent::Text("Here is the result...".to_string()))
        .unwrap();
    process_streaming_events(&mut app);

    assert!(
        app.streaming_todos.is_none(),
        "streaming_todos should be cleared when new text arrives"
    );
    assert!(
        app.streaming_tool_result.is_none(),
        "streaming_tool_result should also be cleared when new text arrives"
    );
    assert_eq!(
        app.streaming_text, "Here is the result...",
        "streaming_text should contain the new text"
    );
}

#[test]
fn test_multiple_todo_updates_replace_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // First batch of todos
    let first = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content: first.clone(),
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert_eq!(app.streaming_todos.as_deref(), Some(first.as_str()));

    // Second batch (e.g., after completing step 1)
    let second = sample_todo(1, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content: second.clone(),
    })
    .unwrap();
    process_streaming_events(&mut app);

    assert_eq!(
        app.streaming_todos.as_deref(),
        Some(second.as_str()),
        "streaming_todos should be replaced with the latest todos"
    );
}

// ── Tests: existing text is preserved when tool result arrives ───────────────

#[test]
fn test_tool_result_does_not_clear_streaming_text() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Model sends some text first
    tx.send(StreamEvent::Text("Analyzing the code...".to_string()))
        .unwrap();
    process_streaming_events(&mut app);
    assert_eq!(app.streaming_text, "Analyzing the code...");

    // Then a tool result arrives (model calls write_todos)
    let content = sample_todo(0, 2);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);

    // streaming_text should be preserved
    assert_eq!(
        app.streaming_text, "Analyzing the code...",
        "existing streaming text should not be cleared by tool result"
    );
    assert!(
        app.streaming_todos.is_some(),
        "streaming_todos should be set"
    );
}

// ── Tests: state reset/cleanup clears streaming_todos ───────────────────────

#[test]
fn test_reset_streaming_state_clears_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos
    let content = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Reset streaming state (happens when a new LLM response starts)
    reset_streaming_state(&mut app);

    assert!(
        app.streaming_todos.is_none(),
        "streaming_todos should be cleared by reset_streaming_state"
    );
    assert!(app.streaming_text.is_empty());
    assert!(app.streaming_status.is_empty());
    assert!(app.current_tool_call.is_none());
}

#[test]
fn test_cleanup_stream_state_clears_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos
    let content = sample_todo(2, 2);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Clean up stream state (happens on disconnect/error)
    cleanup_stream_state(&mut app);

    assert!(
        app.streaming_todos.is_none(),
        "streaming_todos should be cleared by cleanup_stream_state"
    );
    assert!(!app.is_streaming);
}

// ── Tests: status events do not clear streaming_todos ────────────────────────

#[test]
fn test_status_event_does_not_clear_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos by sending a tool result
    let content = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Clear the tool result (it was consumed by .take() in the renderer)
    app.streaming_tool_result = None;

    // Now send a status event (inter-turn waiting period)
    tx.send(StreamEvent::Status("⏳ Waiting for model response...".to_string()))
        .unwrap();
    process_streaming_events(&mut app);

    assert!(
        app.streaming_todos.is_some(),
        "streaming_todos should persist through status events"
    );
    assert_eq!(app.streaming_status, "⏳ Waiting for model response...");
}

// ── Tests: ReasoningDelta does not clear streaming_todos ─────────────────────

#[test]
fn test_reasoning_delta_does_not_clear_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos
    let content = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Reasoning delta events should NOT clear streaming_todos
    tx.send(StreamEvent::ReasoningDelta("thinking about the code...".to_string()))
        .unwrap();
    process_streaming_events(&mut app);

    assert!(
        app.streaming_todos.is_some(),
        "streaming_todos should persist through reasoning deltas"
    );
}

// ── Tests: ToolCall does not clear streaming_todos ──────────────────────────

#[test]
fn test_tool_call_event_does_not_clear_streaming_todos() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos
    let content = sample_todo(1, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content,
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // A new tool call event should NOT clear streaming_todos
    // (the todos stay visible while the next tool executes)
    tx.send(StreamEvent::ToolCall {
        name: "file_read".to_string(),
        arguments: r#"{"path":"src/main.rs"}"#.to_string(),
    })
    .unwrap();
    process_streaming_events(&mut app);

    assert!(
        app.streaming_todos.is_some(),
        "streaming_todos should persist through tool call events"
    );
}

// ── Tests: streaming_todos survives across frames without tool result ────────

#[test]
fn test_streaming_todos_persists_after_tool_result_consumed() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = make_app(rx);

    // Set streaming_todos via tool result
    let content = sample_todo(0, 3);
    tx.send(StreamEvent::ToolResult {
        name: "write_todos".to_string(),
        content: content.clone(),
    })
    .unwrap();
    process_streaming_events(&mut app);
    assert!(app.streaming_todos.is_some(), "precondition: streaming_todos should be set");

    // Simulate the renderer consuming streaming_tool_result (via .take())
    app.streaming_tool_result = None;
    // Add a status event (simulating the inter-turn waiting period)
    tx.send(StreamEvent::Status("⏳ Waiting...".to_string()))
        .unwrap();
    process_streaming_events(&mut app);

    // streaming_todos should still be present even after streaming_tool_result is consumed
    assert_eq!(
        app.streaming_todos.as_deref(),
        Some(content.as_str()),
        "streaming_todos should persist after streaming_tool_result is consumed"
    );
}
