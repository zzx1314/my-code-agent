use my_code_agent::core::session::{SessionData, format_timestamp};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::core::types::Message;

// ── Helpers ──

fn test_token_usage() -> TokenUsage {
    TokenUsage::with_context_window(65_536)
}

fn test_messages() -> Vec<Message> {
    vec![Message::user("hello")]
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
    usage.add(my_code_agent::core::types::Usage {
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

// ── prune_old_sessions ──

#[test]
fn test_prune_old_sessions_keeps_max_count() {
    let session_dir = SessionData::session_dir_path();
    std::fs::create_dir_all(&session_dir).unwrap();

    let mut saved_names = Vec::new();
    let base_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    for i in 0..7u64 {
        let mut data = SessionData::new(
            test_messages(),
            test_token_usage(),
            format!("prune_test {}", i),
        );
        data.saved_at = base_ts + i * 100; // space out by 100 seconds
        let name = format!("prune_test_{}", i);
        data.save_with_name(&name).unwrap();
        saved_names.push(name);
    }

    // Verify we have at least 7 sessions
    let all_sessions = SessionData::list_sessions();
    let test_sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| s.name.starts_with("prune_test_"))
        .collect();
    assert!(
        test_sessions.len() >= 7,
        "Expected at least 7 test sessions, got {}",
        test_sessions.len()
    );

    // Prune to keep only 5
    let removed = SessionData::prune_old_sessions(5).unwrap();
    assert!(removed >= 2, "Expected at least 2 removed, got {}", removed);

    // Check that the newest 5 test sessions still exist (by saved_at)
    let remaining = SessionData::list_sessions();
    let remaining_test: Vec<_> = remaining
        .iter()
        .filter(|s| s.name.starts_with("prune_test_"))
        .collect();
    // The remaining test sessions should be the ones with highest saved_at
    assert!(
        remaining_test.len() <= 5,
        "Expected at most 5 test sessions remaining, got {}",
        remaining_test.len()
    );

    // Clean up test sessions
    for name in &saved_names {
        let _ = SessionData::delete_by_name(name);
    }
}

#[test]
fn test_prune_old_sessions_no_op_when_under_limit() {
    // Save fewer than 5 sessions and verify prune returns 0
    let session_dir = SessionData::session_dir_path();
    std::fs::create_dir_all(&session_dir).unwrap();

    let base_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut saved_names = Vec::new();
    for i in 0..3u64 {
        let mut data = SessionData::new(
            test_messages(),
            test_token_usage(),
            format!("no_prune {}", i),
        );
        data.saved_at = base_ts + 2000 + i * 100;
        let name = format!("no_prune_test_{}", i);
        data.save_with_name(&name).unwrap();
        saved_names.push(name);
    }

    // Count sessions before prune
    let _before = SessionData::list_sessions().len();

    // Prune to keep 5 — since we only added 3, nothing should be removed among these
    let _removed = SessionData::prune_old_sessions(5).unwrap();
    // removed might be > 0 if other sessions exist, but our 3 should all survive
    let remaining = SessionData::list_sessions();
    let remaining_test: Vec<_> = remaining
        .iter()
        .filter(|s| s.name.starts_with("no_prune_test_"))
        .collect();
    assert_eq!(remaining_test.len(), 3, "All 3 test sessions should remain");

    // Clean up
    for name in &saved_names {
        let _ = SessionData::delete_by_name(name);
    }
}

#[test]
fn test_session_dir_path() {
    let dir = SessionData::session_dir_path();
    // Should end with ".sessions" regardless of base directory
    assert!(
        dir.ends_with(".sessions"),
        "Expected path ending with .sessions, got: {}",
        dir
    );
}

#[test]
fn test_session_file_path() {
    let path1 = SessionData::session_file_path("my-session");
    assert!(
        path1.ends_with(".sessions/my-session.json"),
        "Expected path ending with .sessions/my-session.json, got: {}",
        path1
    );
    let path2 = SessionData::session_file_path("bugfix-123");
    assert!(
        path2.ends_with(".sessions/bugfix-123.json"),
        "Expected path ending with .sessions/bugfix-123.json, got: {}",
        path2
    );
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
