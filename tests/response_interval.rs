use my_code_agent::app::App;
use my_code_agent::core::agent::client::LlmClient;
use my_code_agent::core::agent::preamble::{Agent, build_preamble};
use my_code_agent::core::agent::stream::process_message_queue;
use my_code_agent::core::config::Config;
use my_code_agent::core::context::context_manager::ContextManager;
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::tools::ToolRegistry;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ── Test helpers ──────────────────────────────────────────────────────────────

fn make_app_with_interval(interval_ms: u64) -> App {
    let mut config = Config::default();
    config.agent.response_interval_ms = interval_ms;
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

fn make_context_manager() -> ContextManager {
    let config = Config::default();
    ContextManager::new(&config)
}

// ── Config tests ──────────────────────────────────────────────────────────────

#[test]
fn test_response_interval_default_is_zero() {
    let config = Config::default();
    assert_eq!(
        config.agent.response_interval_ms, 0,
        "response_interval_ms should default to 0 (no delay)"
    );
}

#[test]
fn test_response_interval_config_from_toml() {
    let toml_str = "[agent]\nresponse_interval_ms = 500\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.response_interval_ms, 500);
}

// ── App cooldown state tests ──────────────────────────────────────────────────

#[test]
fn test_cooldown_initially_none() {
    let app = make_app_with_interval(0);
    assert!(
        app.response_cooldown_until.is_none(),
        "response_cooldown_until should be None initially"
    );
}

#[test]
fn test_cooldown_not_active_when_unset() {
    let app = make_app_with_interval(1000);
    assert!(
        !app.is_response_cooldown_active(),
        "cooldown should not be active when response_cooldown_until is None"
    );
}

#[test]
fn test_cooldown_active_when_future_deadline() {
    let mut app = make_app_with_interval(1000);
    // Set a deadline 5 seconds in the future
    app.response_cooldown_until = Some(Instant::now() + Duration::from_secs(5));
    assert!(
        app.is_response_cooldown_active(),
        "cooldown should be active when deadline is in the future"
    );
}

#[test]
fn test_cooldown_not_active_when_past_deadline() {
    let mut app = make_app_with_interval(1000);
    // Set a deadline in the past (already expired)
    app.response_cooldown_until = Some(Instant::now() - Duration::from_secs(1));
    assert!(
        !app.is_response_cooldown_active(),
        "cooldown should NOT be active when deadline is in the past"
    );
}

#[test]
fn test_cooldown_remaining_none_when_unset() {
    let app = make_app_with_interval(1000);
    assert!(
        app.response_cooldown_remaining().is_none(),
        "remaining should be None when no cooldown is set"
    );
}

#[test]
fn test_cooldown_remaining_some_when_active() {
    let mut app = make_app_with_interval(1000);
    app.response_cooldown_until = Some(Instant::now() + Duration::from_millis(5000));
    let remaining = app.response_cooldown_remaining();
    assert!(
        remaining.is_some(),
        "remaining should be Some when cooldown is active"
    );
    // Should be close to 5 seconds (allow small clock skew)
    let remaining = remaining.unwrap();
    assert!(
        remaining <= Duration::from_millis(5100) && remaining > Duration::from_millis(4000),
        "remaining should be ~5 seconds, got {:?}",
        remaining
    );
}

#[test]
fn test_cooldown_remaining_none_when_expired() {
    let mut app = make_app_with_interval(1000);
    app.response_cooldown_until = Some(Instant::now() - Duration::from_millis(100));
    assert!(
        app.response_cooldown_remaining().is_none(),
        "remaining should be None when cooldown has expired"
    );
}

// ── Message queue cooldown tests ──────────────────────────────────────────────

#[test]
fn test_message_queue_not_processed_during_cooldown() {
    let mut app = make_app_with_interval(5000); // 5 second cooldown
    let mut ctx = make_context_manager();

    // Simulate an active cooldown (5 seconds in the future)
    app.response_cooldown_until = Some(Instant::now() + Duration::from_secs(5));

    // Add a message to the queue
    app.message_queue.push("test message".to_string());

    // Try to process the queue — should NOT process due to cooldown
    let processed = process_message_queue(&mut app, &mut ctx);
    assert!(
        !processed,
        "message queue should not process during cooldown"
    );
    assert_eq!(
        app.message_queue.len(),
        1,
        "message should still be in the queue"
    );
    assert!(
        !app.is_streaming,
        "should not start streaming during cooldown"
    );
}

#[tokio::test]
async fn test_message_queue_processed_after_cooldown_expires() {
    let mut app = make_app_with_interval(1); // 1ms cooldown
    let mut ctx = make_context_manager();

    // Simulate an expired cooldown (1ms ago)
    app.response_cooldown_until = Some(Instant::now() - Duration::from_millis(100));

    // Add a message to the queue
    app.message_queue.push("test message".to_string());

    // Try to process the queue — SHOULD process (cooldown expired)
    let processed = process_message_queue(&mut app, &mut ctx);
    assert!(
        processed,
        "message queue should process after cooldown expires"
    );
    assert!(
        app.message_queue.is_empty(),
        "queue should be empty after processing"
    );
    assert!(
        app.is_streaming,
        "should start streaming after cooldown expires"
    );
}

#[tokio::test]
async fn test_message_queue_processed_when_no_cooldown_configured() {
    let mut app = make_app_with_interval(0); // no cooldown
    let mut ctx = make_context_manager();

    // No cooldown set (response_cooldown_until is None)

    // Add a message to the queue
    app.message_queue.push("test message".to_string());

    // Try to process the queue — SHOULD process (no cooldown)
    let processed = process_message_queue(&mut app, &mut ctx);
    assert!(
        processed,
        "message queue should process when no cooldown configured"
    );
    assert!(app.is_streaming);
}

#[test]
fn test_message_queue_empty_returns_false() {
    let mut app = make_app_with_interval(5000);
    let mut ctx = make_context_manager();

    // Active cooldown but empty queue
    app.response_cooldown_until = Some(Instant::now() + Duration::from_secs(5));

    let processed = process_message_queue(&mut app, &mut ctx);
    assert!(!processed, "should return false when queue is empty");
}

#[test]
fn test_message_queue_not_processed_while_streaming() {
    let mut app = make_app_with_interval(0); // no cooldown
    let mut ctx = make_context_manager();

    // Simulate active streaming
    app.is_streaming = true;
    app.message_queue.push("queued msg".to_string());

    let processed = process_message_queue(&mut app, &mut ctx);
    assert!(!processed, "should not process queue while streaming");
    assert_eq!(app.message_queue.len(), 1);
}
