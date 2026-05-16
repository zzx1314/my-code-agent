use my_code_agent::app::App;
use my_code_agent::core::config::Config;
use my_code_agent::core::agent::client::LlmClient;
use my_code_agent::core::agent::preamble::{Agent, build_preamble};
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::tools::ToolRegistry;
use std::sync::Arc;

fn make_app() -> App {
    let config = Config::default();
    // Use a dummy client — these tests only test message_queue (Vec<String>),
    // never make actual LLM calls, so no API key is needed.
    let client = LlmClient::new("http://localhost:9999", "", "test-model");
    let system_prompt = build_preamble();
    let tools = ToolRegistry::from_config(&config);
    let agent = Arc::new(Agent::new(client, system_prompt, tools));
    let (interrupt_tx, _) = tokio::sync::broadcast::channel(1);
    let token_usage = TokenUsage::with_config(&config);
    App::new(
        vec![],
        token_usage,
        String::new(),
        config,
        agent,
        interrupt_tx,
    )
}

#[test]
fn test_message_queue_initially_empty() {
    let app = make_app();
    assert!(app.message_queue.is_empty());
}

#[test]
fn test_message_queue_push() {
    let mut app = make_app();
    app.message_queue.push("first".to_string());
    assert_eq!(app.message_queue.len(), 1);
}

#[test]
fn test_message_queue_order() {
    let mut app = make_app();
    app.message_queue.push("first".to_string());
    app.message_queue.push("second".to_string());
    assert_eq!(app.message_queue[0], "first");
    assert_eq!(app.message_queue[1], "second");
}

#[test]
fn test_message_queue_remove() {
    let mut app = make_app();
    app.message_queue.push("first".to_string());
    app.message_queue.push("second".to_string());
    let removed = app.message_queue.remove(0);
    assert_eq!(removed, "first");
    assert_eq!(app.message_queue.len(), 1);
    assert_eq!(app.message_queue[0], "second");
}
