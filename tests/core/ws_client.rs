//! Tests for the WebSocket client module.
//!
//! Covers protocol serialization/deserialization (WsCommand, WsResponse),
//! config parsing, and `spawn()` in disabled mode.

use my_code_agent::core::config::Config;
use my_code_agent::core::ws_client::{WsCommand, WsResponse};
use tokio::sync::mpsc;

// ── WsCommand deserialization (JSON → enum) ───────────────────────────────

#[test]
fn test_deserialize_prompt_with_id() {
    let json = r#"{"type": "prompt", "text": "Hello", "id": "req-001"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::Prompt { text, id } => {
            assert_eq!(text, "Hello");
            assert_eq!(id, Some("req-001".to_string()));
        }
        other => panic!("expected Prompt, got {:?}", other),
    }
}

#[test]
fn test_deserialize_prompt_without_id() {
    let json = r#"{"type": "prompt", "text": "hello"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::Prompt { text, id } => {
            assert_eq!(text, "hello");
            assert_eq!(id, None);
        }
        other => panic!("expected Prompt, got {:?}", other),
    }
}

#[test]
fn test_deserialize_command_with_id() {
    let json = r#"{"type": "command", "cmd": "/status", "id": "req-002"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::Command { cmd, id } => {
            assert_eq!(cmd, "/status");
            assert_eq!(id, Some("req-002".to_string()));
        }
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn test_deserialize_command_without_id() {
    let json = r#"{"type": "command", "cmd": "/tokens"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();
    match cmd {
        WsCommand::Command { cmd, .. } => assert_eq!(cmd, "/tokens"),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn test_deserialize_command_missing_cmd_field() {
    // `cmd` is a required field — omitting it should fail
    let json = r#"{"type": "command"}"#;
    let result: Result<WsCommand, _> = serde_json::from_str(json);
    assert!(result.is_err(), "missing 'cmd' field should fail");
}

#[test]
fn test_deserialize_get_history_with_id() {
    let json = r#"{"type": "get_history", "id": "req-003"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::GetHistory { id } => {
            assert_eq!(id, Some("req-003".to_string()));
        }
        other => panic!("expected GetHistory, got {:?}", other),
    }
}

#[test]
fn test_deserialize_get_history_without_id() {
    let json = r#"{"type": "get_history"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();
    match cmd {
        WsCommand::GetHistory { id } => assert!(id.is_none()),
        other => panic!("expected GetHistory, got {:?}", other),
    }
}

#[test]
fn test_deserialize_interrupt_with_id() {
    let json = r#"{"type": "interrupt", "id": "req-004"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::Interrupt { id } => {
            assert_eq!(id, Some("req-004".to_string()));
        }
        other => panic!("expected Interrupt, got {:?}", other),
    }
}

#[test]
fn test_deserialize_interrupt_without_id() {
    let json = r#"{"type": "interrupt"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();
    match cmd {
        WsCommand::Interrupt { id } => assert!(id.is_none()),
        other => panic!("expected Interrupt, got {:?}", other),
    }
}

#[test]
fn test_deserialize_ping() {
    let json = r#"{"type": "ping"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();

    match cmd {
        WsCommand::Ping { id } => assert!(id.is_none()),
        other => panic!("expected Ping, got {:?}", other),
    }
}

#[test]
fn test_deserialize_ping_with_id() {
    let json = r#"{"type": "ping", "id": "keepalive-1"}"#;
    let cmd: WsCommand = serde_json::from_str(json).unwrap();
    match cmd {
        WsCommand::Ping { id } => {
            assert_eq!(id, Some("keepalive-1".to_string()));
        }
        other => panic!("expected Ping, got {:?}", other),
    }
}

// ── Error / edge cases ─────────────────────────────────────────────────────

#[test]
fn test_deserialize_invalid_type_tag() {
    let json = r#"{"type": "unknown_command", "text": "hello"}"#;
    let result: Result<WsCommand, _> = serde_json::from_str(json);
    assert!(result.is_err(), "unknown type tag should fail");
}

#[test]
fn test_deserialize_missing_type_tag() {
    let json = r#"{"text": "hello"}"#;
    let result: Result<WsCommand, _> = serde_json::from_str(json);
    assert!(result.is_err(), "missing type tag should fail");
}

#[test]
fn test_deserialize_malformed_json() {
    let result: Result<WsCommand, _> = serde_json::from_str("not json");
    assert!(result.is_err());
}

// ── WsResponse serialization (enum → JSON) ────────────────────────────────

#[test]
fn test_serialize_result_ok() {
    let resp = WsResponse::Result {
        ok: true,
        summary: "Refactored module".to_string(),
        full_response: Some("Refactored the module successfully.".to_string()),
        error: None,
        id: Some("req-001".to_string()),
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "result");
    assert_eq!(obj["ok"], true);
    assert_eq!(obj["summary"], "Refactored module");
    assert_eq!(obj["full_response"], "Refactored the module successfully.");
    assert_eq!(obj["id"], "req-001");
    assert!(
        !obj.contains_key("error"),
        "error should be skipped when None"
    );
}

#[test]
fn test_serialize_result_error() {
    let resp = WsResponse::Result {
        ok: false,
        summary: "Failed".to_string(),
        full_response: None,
        error: Some("Something went wrong".to_string()),
        id: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "result");
    assert_eq!(obj["ok"], false);
    assert_eq!(obj["summary"], "Failed");
    assert_eq!(obj["error"], "Something went wrong");
    assert!(
        !obj.contains_key("full_response"),
        "full_response should be skipped when None"
    );
    // id may be skipped or null — either is fine
    if obj.contains_key("id") {
        assert!(obj["id"].is_null());
    }
}

#[test]
fn test_serialize_history() {
    let msg = my_code_agent::core::types::Message::user("test");
    let resp = WsResponse::History {
        messages: vec![msg.clone()],
        id: Some("req-003".to_string()),
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "history");
    assert_eq!(obj["id"], "req-003");
    let msgs = obj["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "test");
}

#[test]
fn test_serialize_history_without_id() {
    // When `id` is None, it should be omitted from JSON
    let resp = WsResponse::History {
        messages: vec![],
        id: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains(r#""type":"history""#));
    assert!(json.contains(r#""messages":[]"#));
    assert!(!json.contains("id"), "id should be omitted when None");
}

#[test]
fn test_serialize_status_streaming() {
    let resp = WsResponse::Status {
        streaming: true,
        message: Some("Processing...".to_string()),
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "status");
    assert_eq!(obj["streaming"], true);
    assert_eq!(obj["message"], "Processing...");
}

#[test]
fn test_serialize_status_idle_without_message() {
    let resp = WsResponse::Status {
        streaming: false,
        message: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "status");
    assert_eq!(obj["streaming"], false);
    assert!(
        !obj.contains_key("message"),
        "message should be skipped when None"
    );
}

#[test]
fn test_serialize_pong_with_id() {
    let resp = WsResponse::Pong {
        id: Some("keepalive-1".to_string()),
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "pong");
    assert_eq!(obj["id"], "keepalive-1");
}

#[test]
fn test_serialize_pong_without_id() {
    let resp = WsResponse::Pong { id: None };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "pong");
    if obj.contains_key("id") {
        assert!(obj["id"].is_null());
    }
}

#[test]
fn test_serialize_error() {
    let resp = WsResponse::Error {
        message: "Invalid JSON".to_string(),
        id: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    let obj = json.as_object().unwrap();

    assert_eq!(obj["type"], "error");
    assert_eq!(obj["message"], "Invalid JSON");
}

// ── Round-trip: Ping → Pong with same id ─────────────────────────────────

#[test]
fn test_ping_pong_round_trip() {
    // Server sends ping → client responds with pong (same id)
    let cases = vec![
        (r#"{"type": "ping"}"#, None::<&str>),
        (r#"{"type": "ping", "id": "req-001"}"#, Some("req-001")),
        (r#"{"type": "ping", "id": ""}"#, Some("")),
    ];

    for (json_str, expected_id) in cases {
        let cmd: WsCommand = serde_json::from_str(json_str).unwrap();
        let id = match &cmd {
            WsCommand::Ping { id } => id.clone(),
            _ => unreachable!(),
        };
        assert_eq!(
            id.as_deref(),
            expected_id,
            "ping with JSON {} should have id={:?}",
            json_str,
            expected_id
        );

        // Build pong response (same logic as in run_ws_client)
        let pong = WsResponse::Pong { id };
        let pong_json = serde_json::to_value(&pong).unwrap();
        assert_eq!(pong_json["type"], "pong");
        match expected_id {
            Some(eid) => assert_eq!(pong_json["id"], eid),
            None => {
                if pong_json.as_object().unwrap().contains_key("id") {
                    assert!(pong_json["id"].is_null());
                }
            }
        }
    }
}

// ── All variants produce valid JSON ────────────────────────────────────────

#[test]
fn test_all_response_variants_serialize() {
    let variants: Vec<WsResponse> = vec![
        WsResponse::Result {
            ok: true,
            summary: "s".into(),
            full_response: None,
            error: None,
            id: None,
        },
        WsResponse::History {
            messages: vec![],
            id: None,
        },
        WsResponse::Status {
            streaming: false,
            message: None,
        },
        WsResponse::Pong { id: None },
        WsResponse::Error {
            message: "e".into(),
            id: None,
        },
    ];

    for v in variants {
        let json = serde_json::to_string(&v).unwrap_or_else(|e| panic!("serialize failed: {e}"));
        assert!(
            json.starts_with("{\"type\":"),
            "should start with type field, got: {json}"
        );
    }
}

// ── Config parsing ─────────────────────────────────────────────────────────

#[test]
fn test_config_defaults_ws_client_disabled() {
    let config = Config::default();
    assert!(
        !config.ws_client.enabled,
        "ws_client should be disabled by default"
    );
    assert!(
        config.ws_client.url.is_none(),
        "url should be None by default"
    );
    assert_eq!(
        config.ws_client.reconnect_interval_secs, 2,
        "default reconnect interval should be 2"
    );
    assert!(
        config.ws_client.auth_token.is_none(),
        "auth_token should be None by default"
    );
}

#[test]
fn test_config_deserialize_ws_client_full() {
    let toml_str = r#"
[ws_client]
enabled = true
url = "ws://localhost:8080/agent"
reconnect_interval_secs = 5
auth_token = "my-secret-token"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.ws_client.enabled);
    assert_eq!(
        config.ws_client.url.as_deref(),
        Some("ws://localhost:8080/agent")
    );
    assert_eq!(config.ws_client.reconnect_interval_secs, 5);
    assert_eq!(
        config.ws_client.auth_token.as_deref(),
        Some("my-secret-token")
    );
}

#[test]
fn test_config_deserialize_ws_client_minimal() {
    // Only enabled=true, everything else falls back to defaults
    let toml_str = r#"
[ws_client]
enabled = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.ws_client.enabled);
    assert!(config.ws_client.url.is_none());
    assert_eq!(config.ws_client.reconnect_interval_secs, 2);
    assert!(config.ws_client.auth_token.is_none());
}

// ── spawn() in disabled mode ────────────────────────────────────────────────

#[tokio::test]
async fn test_spawn_disabled_returns_channels() {
    let (_resp_tx, resp_rx) = mpsc::unbounded_channel::<WsResponse>();

    let config = Config::default(); // ws_client.enabled = false
    let (mut cmd_rx, shutdown_tx) = my_code_agent::core::ws_client::spawn(&config, resp_rx);

    // When disabled, the mpsc sender is dropped immediately inside spawn(),
    // so cmd_rx is disconnected (no senders).
    let cmd_result = cmd_rx.try_recv();
    assert!(
        matches!(cmd_result, Err(mpsc::error::TryRecvError::Disconnected)),
        "cmd_rx should be disconnected when disabled, got {:?}",
        cmd_result
    );

    // The broadcast sender exists but has no receivers (the spawned task
    // that would hold one was never started). This is correct.
    let _ = shutdown_tx.send(()); // ignore error — no receivers is expected

    // Drop channels cleanly — ensures no panics
    drop(cmd_rx);
    drop(shutdown_tx);
}
