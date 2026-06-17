//! WebSocket client module for headless agent mode.
//!
//! When enabled, the application connects as a WebSocket client to an external
//! server that sends JSON commands to direct the agent (prompts, commands,
//! history queries, etc.). Results are pushed back asynchronously over the
//! same WebSocket connection.
//!
//! # Protocol
//!
//! **Incoming** (server → client):
//! ```json
//! {"type": "prompt", "text": "...", "id": "req-001"}
//! {"type": "command", "cmd": "/status", "id": "req-002"}
//! {"type": "get_history", "id": "req-003"}
//! {"type": "interrupt", "id": "req-004"}
//! {"type": "ping"}
//! ```
//!
//! **Outgoing** (client → server):
//! ```json
//! {"type": "result", "ok": true, "summary": "...", "full_response": "...", "id": "req-001"}
//! {"type": "history", "messages": [...], "id": "req-003"}
//! {"type": "status", "streaming": true, "message": "Processing..."}
//! {"type": "text_delta", "delta": "正在..."}
//! {"type": "reasoning_delta", "delta": "思考中..."}
//! {"type": "tool_call", "name": "file_read", "arguments": "..."}
//! {"type": "tool_result", "name": "file_read", "content": "..."}
//! {"type": "pong", "id": null}
//! {"type": "error", "message": "Invalid command: ...", "id": null}
//! ```

use crate::core::config::Config;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

/// Command received from the WebSocket server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsCommand {
    /// Send a prompt to the agent. The agent processes it and pushes the result
    /// back via [`WsResponse::Result`].
    Prompt {
        text: String,
        /// Optional request identifier for correlating responses.
        #[serde(default)]
        id: Option<String>,
    },
    /// Execute a slash command (e.g. `/status`, `/tokens`).
    #[serde(rename_all = "snake_case")]
    Command {
        cmd: String,
        #[serde(default)]
        id: Option<String>,
    },
    /// Get the full conversation history.
    GetHistory {
        #[serde(default)]
        id: Option<String>,
    },
    /// Interrupt the current streaming response.
    Interrupt {
        #[serde(default)]
        id: Option<String>,
    },
    /// Ping / health check. The client responds immediately with `Pong`.
    Ping {
        #[serde(default)]
        id: Option<String>,
    },
}

/// Response sent back to the WebSocket server.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsResponse {
    /// Result of a `Prompt` or `Command` (with optional error).
    Result {
        ok: bool,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        full_response: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Full conversation history (response to `GetHistory`).
    History {
        messages: Vec<crate::core::types::Message>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Status update (e.g. streaming started/stopped).
    Status {
        streaming: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Response to a `Ping` command.
    Pong {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Streaming text delta from the agent.
    TextDelta {
        delta: String,
    },
    /// Streaming reasoning delta from the agent.
    ReasoningDelta {
        delta: String,
    },
    /// Tool call started.
    ToolCall {
        name: String,
        arguments: String,
    },
    /// Tool result received.
    ToolResult {
        name: String,
        content: String,
    },
    /// Unrecoverable error (e.g. invalid JSON command).
    /// File content data for transfer to client.
    FileData {
        path: String,
        name: String,
        mime: String,
        data: String,
        size: u64,
        encoding: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
}

/// Spawn the WebSocket client as a background task.
///
/// # Arguments
/// * `config` — Application config (reads the `ws_client` section).
/// * `resp_rx` — Receiver for outgoing responses (from the headless loop) to send over WS.
///
/// # Returns
/// `(cmd_rx, shutdown_tx)` where:
/// - `cmd_rx` receives incoming commands from the WebSocket server (for the headless loop).
/// - `shutdown_tx` signals the WS client to shut down gracefully.
///
/// Both channels are `None`-safe: if the WS client is not enabled, the returned
/// channels will never yield messages, and `shutdown_tx.send(())` is a no-op.
pub fn spawn(
    config: &Config,
    resp_rx: mpsc::UnboundedReceiver<WsResponse>,
) -> (
    mpsc::UnboundedReceiver<WsCommand>,
    tokio::sync::broadcast::Sender<()>,
) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WsCommand>();
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    if !config.ws_client.enabled {
        tracing::info!("WebSocket client is disabled");
        return (cmd_rx, shutdown_tx);
    }

    let url = match &config.ws_client.url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            tracing::warn!("WebSocket client enabled but no URL configured in config.toml [ws_client]");
            return (cmd_rx, shutdown_tx);
        }
    };

    let auth_token = config.ws_client.auth_token.clone();
    let reconnect_secs = config.ws_client.reconnect_interval_secs;
    let shutdown_rx = shutdown_tx.subscribe();

    tokio::spawn(async move {
        run_ws_client(&url, auth_token, reconnect_secs, cmd_tx, resp_rx, shutdown_rx).await;
    });

    (cmd_rx, shutdown_tx)
}

#[allow(clippy::too_many_arguments)]
async fn run_ws_client(
    url: &str,
    auth_token: Option<String>,
    base_reconnect_secs: u64,
    cmd_tx: mpsc::UnboundedSender<WsCommand>,
    mut resp_rx: mpsc::UnboundedReceiver<WsResponse>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) {
    const MAX_RECONNECT_SECS: u64 = 60;
    let mut delay = base_reconnect_secs;

    loop {
        tracing::info!(url = %url, "WebSocket client connecting...");

        let connect_result = tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("WebSocket client shut down before connect");
                return;
            }
            r = connect_async(url) => r,
        };

        match connect_result {
            Ok((ws_stream, _)) => {
                tracing::info!(url = %url, "WebSocket connected");
                delay = base_reconnect_secs;

                let (mut ws_sink, mut ws_stream) = ws_stream.split();

                // ── Auth ──────────────────────────────────────────────────
                if let Some(token) = &auth_token {
                    let auth_msg = serde_json::json!({"type": "auth", "token": token});
                    let _ = ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(
                            auth_msg.to_string(),
                        ))
                        .await;
                }

                // ── Drain pending responses that accumulated while disconnected ──
                drain_pending_responses(&mut resp_rx, &mut ws_sink).await;

                // ── Main read/write loop ──────────────────────────────────
                loop {
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            tracing::info!("WebSocket client shutting down");
                            let _ = ws_sink.close().await;
                            return;
                        }

                        // Incoming message from server
                        msg = ws_stream.next() => {
                            match msg {
                                Some(Ok(msg)) => {
                                    if msg.is_text() || msg.is_binary() {
                                        let text = msg.to_text().unwrap_or("").to_string();
                                        match serde_json::from_str::<WsCommand>(&text) {
                                            Ok(cmd) => {
                                                // Ping is handled immediately; everything else is forwarded
                                                if matches!(&cmd, WsCommand::Ping { .. }) {
                                                    let id = extract_id(&cmd);
                                                    let pong = WsResponse::Pong { id };
                                                    if let Ok(json) = serde_json::to_string(&pong) {
                                                        let _ = ws_sink
                                                            .send(tokio_tungstenite::tungstenite::Message::text(json))
                                                            .await;
                                                    }
                                                } else {
                                                    let _ = cmd_tx.send(cmd);
                                                }
                                            }
                                            Err(e) => {
                                                let err = WsResponse::Error {
                                                    message: format!("Invalid command: {}", e),
                                                    id: None,
                                                };
                                                if let Ok(json) = serde_json::to_string(&err) {
                                                    let _ = ws_sink
                                                        .send(tokio_tungstenite::tungstenite::Message::text(json))
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                }
                                Some(Err(e)) => {
                                    tracing::warn!("WebSocket error: {}", e);
                                    break;
                                }
                                None => {
                                    tracing::info!("WebSocket connection closed by server");
                                    break;
                                }
                            }
                        }

                        // Response from headless loop → send over WS
                        response = resp_rx.recv() => {
                            match response {
                                Some(response) => {
                                    if let Ok(json) = serde_json::to_string(&response) {
                                        let _ = ws_sink
                                            .send(tokio_tungstenite::tungstenite::Message::text(json))
                                            .await;
                                    }
                                }
                                None => {
                                    tracing::warn!("Response channel closed, shutting down WS client");
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    url = %url,
                    error = %e,
                    retry_secs = delay,
                    "WebSocket connection failed"
                );
            }
        }

        // ── Reconnection delay with exponential backoff ───────────────────
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("WebSocket client shut down during reconnect delay");
                return;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(delay)) => {}
        }
        delay = (delay * 2).min(MAX_RECONNECT_SECS);
    }
}

/// Non-blocking drain of any pending responses that accumulated while the WS
/// connection was down. Stops at the first `Empty` or `Disconnected`.
async fn drain_pending_responses(
    resp_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    ws_sink: &mut (impl futures_util::Sink<tokio_tungstenite::tungstenite::Message> + Unpin),
) {
    loop {
        match resp_rx.try_recv() {
            Ok(response) => {
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(json))
                        .await;
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
}

/// Extract the optional `id` from any `WsCommand` variant.
fn extract_id(cmd: &WsCommand) -> Option<String> {
    match cmd {
        WsCommand::Prompt { id, .. } => id.clone(),
        WsCommand::Command { id, .. } => id.clone(),
        WsCommand::GetHistory { id, .. } => id.clone(),
        WsCommand::Interrupt { id, .. } => id.clone(),
        WsCommand::Ping { id, .. } => id.clone(),
    }
}
