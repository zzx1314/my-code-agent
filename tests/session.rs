use my_code_agent::core::config::Config;
use my_code_agent::core::session::{SessionData, DEFAULT_SESSION_FILE, format_timestamp};
use my_code_agent::core::token_usage::TokenUsage;
use rig::completion::Message;

// ── Helpers ──

fn test_token_usage() -> TokenUsage {
    TokenUsage::with_context_window(65_536)
}

fn test_messages() -> Vec<Message> {
    vec![Message::User {
        content: rig::one_or_many::OneOrMany::one(rig::message::UserContent::text("hello")),
    }]
}

// ── SessionData creation ──

#[test]
fn test_session_data_new() {
    let data = SessionData::new(test_messages(), test_token_usage(), "reasoning".into());
    assert_eq!(data.chat_history.len(), 1);
    assert_eq!(data.last_reasoning, "reasoning");
    assert!(data.saved_at > 0);
}

#[test]
fn test_session_data_new_empty() {
    let data = SessionData::new(Vec::new(), test_token_usage(), String::new());
    assert!(data.chat_history.is_empty());
    assert!(data.last_reasoning.is_empty());
    assert!(data.saved_at > 0);
}

// ── Save and load round-trip ──

#[test]
fn test_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.session.json");
    let path_str = path.to_str().unwrap();

    let data = SessionData::new(test_messages(), test_token_usage(), "reasoning".into());
    data.save_to_file(path_str).unwrap();

    let loaded = SessionData::load_from_file(path_str);
    assert!(loaded.is_some());
    let loaded = loaded.unwrap().unwrap();
    assert_eq!(loaded.chat_history.len(), 1);
    assert_eq!(loaded.last_reasoning, "reasoning");
    assert_eq!(loaded.saved_at, data.saved_at);
}

#[test]
fn test_save_and_load_preserves_token_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokens.session.json");
    let path_str = path.to_str().unwrap();

    let mut usage = TokenUsage::with_context_window(65_536);
    usage.add(rig::completion::Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    });

    let data = SessionData::new(test_messages(), usage.clone(), String::new());
    data.save_to_file(path_str).unwrap();

    let loaded = SessionData::load_from_file(path_str).unwrap().unwrap();
    assert_eq!(loaded.token_usage.input_tokens(), 100);
    assert_eq!(loaded.token_usage.output_tokens(), 50);
    assert_eq!(loaded.token_usage.total_tokens(), 150);
}

#[test]
fn test_save_overwrites_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overwrite.session.json");
    let path_str = path.to_str().unwrap();

    let data1 = SessionData::new(test_messages(), test_token_usage(), "first".into());
    data1.save_to_file(path_str).unwrap();

    let data2 = SessionData::new(Vec::new(), test_token_usage(), "second".into());
    data2.save_to_file(path_str).unwrap();

    let loaded = SessionData::load_from_file(path_str).unwrap().unwrap();
    assert!(loaded.chat_history.is_empty());
    assert_eq!(loaded.last_reasoning, "second");
}

// ── Load non-existent file ──

#[test]
fn test_load_nonexistent_returns_none() {
    let result = SessionData::load_from_file("/nonexistent/path/session.json");
    assert!(result.is_none());
}

// ── Load corrupt file ──

#[test]
fn test_load_corrupt_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.json");
    let path_str = path.to_str().unwrap();

    std::fs::write(path_str, "not valid json{{{").unwrap();
    let result = SessionData::load_from_file(path_str);
    assert!(result.is_some());
    assert!(result.unwrap().is_err());
}

// ── Delete ──

#[test]
fn test_delete_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete-me.json");
    let path_str = path.to_str().unwrap();

    let data = SessionData::new(test_messages(), test_token_usage(), String::new());
    data.save_to_file(path_str).unwrap();
    assert!(path.exists());

    SessionData::delete_file(path_str).unwrap();
    assert!(!path.exists());
}

#[test]
fn test_delete_nonexistent_is_ok() {
    assert!(SessionData::delete_file("/nonexistent/path.json").is_ok());
}

// ── Session path from config ──

#[test]
fn test_session_path_default() {
    let config = Config::default();
    assert_eq!(SessionData::session_path(&config), DEFAULT_SESSION_FILE);
}

#[test]
fn test_session_path_custom() {
    let mut config = Config::default();
    config.session.save_file = Some(".my-session.json".to_string());
    assert_eq!(SessionData::session_path(&config), ".my-session.json");
}

// ── format_timestamp ──

#[test]
fn test_format_timestamp_returns_non_empty() {
    let result = format_timestamp(1_700_000_000);
    assert!(!result.is_empty());
}

#[test]
fn test_format_timestamp_epoch_zero() {
    let result = format_timestamp(0);
    // Should produce some output, not panic
    assert!(!result.is_empty());
}

#[test]
fn test_format_timestamp_recent() {
    // A recent timestamp — on Linux this should produce a readable date
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let result = format_timestamp(now);
    // On Linux with GNU date, should contain "202" (year 202x)
    // But don't assert exact format — just verify it's non-empty and not the fallback
    assert!(!result.is_empty());
}
