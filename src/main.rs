use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use std::path::Path;
use base64::engine::general_purpose;

use my_code_agent::app::bootstrap::init_app;
use my_code_agent::app::lifecycle::run_app;
use my_code_agent::core::agent::preamble::build_preamble;
use my_code_agent::core::agent::stream_response::{StreamEvent, StreamResult, stream_response};
use my_code_agent::core::context::context_manager::ContextManager;
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::core::types::Message;
use my_code_agent::core::ws_client::{WsCommand, WsResponse, SessionInfo};

#[tokio::main]
async fn main() -> Result<()> {
    let state = init_app().await?;

    // ── Headless mode: WebSocket client enabled ───────────────────────────
    if state.config.ws_client.enabled {
        return run_headless(
            state.config,
            state.agent,
            state.chat_history,
            state.token_usage,
            state.context_manager,
            state.interrupt_tx,
        )
        .await;
    }

    // ── TUI mode (default) ────────────────────────────────────────────────
    run_app(
        state.chat_history,
        state.token_usage,
        state.last_reasoning,
        state.config,
        state.agent,
        state.orchestrator,
        state.interrupt_tx,
        state.confirmation_rx,
        state.context_manager,
    )
    .await
}

// ── Headless mode types ──────────────────────────────────────────────────────

/// Complete state snapshot returned by a spawned prompt task.
struct PromptResult {
    /// The stream result from the agent.
    stream: StreamResult,
    /// Updated chat history after the stream.
    messages: Vec<Message>,
    /// Updated token usage.
    session_usage: TokenUsage,
    /// Updated context manager state.
    context_manager: ContextManager,
}

/// Result of handling an immediate command.
enum CommandResult {
    /// Regular response sent, no state change.
    ResponseSent,
    /// Session switched, return new state.
    SessionSwitched {
        messages: Vec<Message>,
        session_usage: TokenUsage,
        session_name: String,
    },
}

/// Tracks a prompt that is currently being processed in a background task.
struct PendingPrompt {
    /// JoinHandle — kept alive so the task doesn't get cancelled.
    _handle: JoinHandle<()>,
    /// Receiver for the final result.
    result_rx: mpsc::Receiver<PromptResult>,
    /// Receiver for streaming events (text deltas, tool calls, etc.)
    event_rx: mpsc::UnboundedReceiver<StreamEvent>,
    /// Optional request ID to correlate the response.
    id: Option<String>,
}

// ── Headless loop ────────────────────────────────────────────────────────────

async fn run_headless(
    config: my_code_agent::core::config::Config,
    agent: std::sync::Arc<my_code_agent::core::agent::preamble::Agent>,
    chat_history: Vec<my_code_agent::app::ChatEntry>,
    token_usage: TokenUsage,
    mut context_manager: ContextManager,
    interrupt_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<()> {
    let system_prompt = build_preamble();

    // Convert chat_history from ChatEntry → Message
    let mut messages: Vec<Message> = chat_history
        .into_iter()
        .map(|e| Message {
            role: e.role,
            content: e.content,
            reasoning_content: e.reasoning_content,
            tool_calls: e.tool_calls,
            tool_call_id: e.tool_call_id,
        })
        .collect();

    let mut session_usage = token_usage;

    // Channels for communicating with the WebSocket client
    let (resp_tx, resp_rx) = mpsc::unbounded_channel::<WsResponse>();
    let (mut cmd_rx, _shutdown_tx) =
        my_code_agent::core::ws_client::spawn(&config, resp_rx);

    tracing::info!("Headless mode started — waiting for WebSocket commands");

    // Notify that the agent is ready
    let _ = resp_tx.send(WsResponse::Status {
        streaming: false,
        message: Some("Agent ready".to_string()),
    });

    // ── Signal handling (Ctrl+C) ──────────────────────────────────────────
    let mut interrupt_signal = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::interrupt(),
    ).expect("Failed to set up SIGINT handler");

    // ── Current async prompt task (None = idle) ───────────────────────────
    let mut pending: Option<PendingPrompt> = None;

    // ── Main command loop ─────────────────────────────────────────────────
    loop {
        if let Some(ref mut pp) = pending {
            // ── We have a prompt in-flight ────────────────────────────────
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(WsCommand::Interrupt { id }) => {
                            tracing::info!("Processing interrupt during prompt");
                            let _ = interrupt_tx.send(());
                            let _ = resp_tx.send(WsResponse::Result {
                                ok: true,
                                summary: "Interrupted".to_string(),
                                full_response: None,
                                error: None,
                                id,
                            });
                            pending = None;
                        }
                        Some(other) => {
                            match handle_immediate_command(
                                other, &messages, &session_usage,
                                &config, &resp_tx,
                            ).await {
                                CommandResult::SessionSwitched {
                                    messages: new_messages,
                                    session_usage: new_usage,
                                    ..
                                } => {
                                    messages = new_messages;
                                    session_usage = new_usage;
                                }
                                CommandResult::ResponseSent => {}
                            }
                        }
                        None => {
                            tracing::info!("Command channel closed, exiting");
                            break;
                        }
                    }
                }

                result = pp.result_rx.recv() => {
                    match result {
                        Some(pr) => {
                            let id = pp.id.take();
                            messages = pr.messages;
                            session_usage = pr.session_usage;
                            context_manager = pr.context_manager;
                            send_prompt_result(pr.stream, &resp_tx, id).await;
                            pending = None;
                        }
                        None => {
                            tracing::warn!("Prompt result channel closed unexpectedly");
                            pending = None;
                        }
                    }
                }

                event = pp.event_rx.recv() => {
                    if let Some(event) = event {
                        forward_stream_event(&event, &resp_tx);
                    }
                }

                // ── Ctrl+C ───────────────────────────────────────────────
                _ = interrupt_signal.recv() => {
                    tracing::info!("SIGINT received, shutting down");
                    let _ = resp_tx.send(WsResponse::Status {
                        streaming: false,
                        message: Some("Shutting down (SIGINT)".to_string()),
                    });
                    break;
                }
            }
        } else {
            // ── Idle — waiting for commands ───────────────────────────────
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(WsCommand::Prompt { text, id }) => {
                            tracing::info!(text_len = text.len(), "Starting prompt");

                            let _ = resp_tx.send(WsResponse::Status {
                                streaming: true,
                                message: Some("Processing prompt...".to_string()),
                            });

                            // ── Spawn the agent in a background task ──────
                            let (result_tx, result_rx) = mpsc::channel(1);
                            let (event_tx, event_rx) = mpsc::unbounded_channel();
                            let agent_clone = agent.clone();
                            let sys_prompt = system_prompt.clone();
                            let agent_config = config.agent.clone();
                            let reasoning_field = config.llm.reasoning_field.clone();
                            let mut interrupt_rx = interrupt_tx.subscribe();
                            // Clone current state for the task
                            let mut task_messages = messages.clone();
                            let mut task_ctx = context_manager.clone();
                            let mut task_usage = session_usage.clone();

                            let handle = tokio::spawn(async move {
                                let result = stream_response(
                                    &agent_clone.client,
                                    &sys_prompt,
                                    &text,
                                    &mut task_messages,
                                    &agent_clone.tools,
                                    &mut task_usage,
                                    &mut interrupt_rx,
                                    &mut task_ctx,
                                    &agent_config,
                                    Some(event_tx),
                                    &reasoning_field,
                                )
                                .await;

                                let _ = result_tx.send(PromptResult {
                                    stream: result,
                                    messages: task_messages,
                                    session_usage: task_usage,
                                    context_manager: task_ctx,
                                }).await;
                            });

                            pending = Some(PendingPrompt {
                                _handle: handle,
                                result_rx,
                                event_rx,
                                id,
                            });
                        }

                        Some(other) => {
                            match handle_immediate_command(
                                other, &messages, &session_usage,
                                &config, &resp_tx,
                            ).await {
                                CommandResult::SessionSwitched {
                                    messages: new_messages,
                                    session_usage: new_usage,
                                    ..
                                } => {
                                    messages = new_messages;
                                    session_usage = new_usage;
                                }
                                CommandResult::ResponseSent => {}
                            }
                        }

                        None => {
                            tracing::info!("Command channel closed, exiting");
                            break;
                        }
                    }
                }

                // ── Ctrl+C ───────────────────────────────────────────────
                _ = interrupt_signal.recv() => {
                    tracing::info!("SIGINT received, shutting down");
                    let _ = resp_tx.send(WsResponse::Status {
                        streaming: false,
                        message: Some("Shutting down (SIGINT)".to_string()),
                    });
                    break;
                }
            }
        }
    }

    tracing::info!("Headless mode shut down");
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Forward a streaming event from the agent to the WebSocket client.
fn forward_stream_event(
    event: &StreamEvent,
    resp_tx: &mpsc::UnboundedSender<WsResponse>,
) {
    // ── File transfer: intercept file_read and send_file tool results ──
    if let StreamEvent::ToolResult { name, content } = event {
        if name == "send_file" {
            // send_file tool already returns encoded data — just forward it
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let name = val.get("name").and_then(|v| v.as_str()).unwrap_or("file");
                let mime = val.get("mime").and_then(|v| v.as_str()).unwrap_or("application/octet-stream");
                let data = val.get("data").and_then(|v| v.as_str()).unwrap_or("");
                let size = val.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

                if !data.is_empty() {
                    let _ = resp_tx.send(WsResponse::FileData {
                        path: path.to_string(),
                        name: name.to_string(),
                        mime: mime.to_string(),
                        data: data.to_string(),
                        size,
                        encoding: "base64".to_string(),
                        id: None,
                    });
                }
            }
        } else if name == "file_read" {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(path_str) = val.get("path").and_then(|v| v.as_str()) {
                    if let Ok(raw) = std::fs::read(path_str) {
                        use base64::Engine;
                        let b64 = general_purpose::STANDARD.encode(&raw);
                        let fname = Path::new(path_str)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let _ = resp_tx.send(WsResponse::FileData {
                            path: path_str.to_string(),
                            name: fname,
                            mime: infer_mime(path_str),
                            data: b64,
                            size: raw.len() as u64,
                            encoding: "base64".to_string(),
                            id: None,
                        });
                    }
                }
            }
        }
    }

    let response = match event {
        StreamEvent::Text(delta) => WsResponse::TextDelta {
            delta: delta.clone(),
        },
        StreamEvent::ReasoningDelta(delta) => WsResponse::ReasoningDelta {
            delta: delta.clone(),
        },
        StreamEvent::ToolCall { name, arguments } => WsResponse::ToolCall {
            name: name.clone(),
            arguments: arguments.clone(),
        },
        StreamEvent::ToolResult { name, content } => WsResponse::ToolResult {
            name: name.clone(),
            content: content.clone(),
        },
        // Status events (e.g. "⏳ Waiting for model response...") forwarded as status updates
        StreamEvent::Status(msg) => WsResponse::Status {
            streaming: true,
            message: Some(msg.clone()),
        },
        // ReasoningActive is implicit via ReasoningDelta — skip to reduce noise
        StreamEvent::ReasoningActive(_) => return,
    };
    let _ = resp_tx.send(response);
}

/// Infer MIME type from file extension.
fn infer_mime(path: &str) -> String {
    match path.rsplit('.').next().unwrap_or("") {
        "rs" | "js" | "ts" | "py" | "go" | "java" | "c" | "cpp" | "h"
        | "rb" | "php" | "swift" | "kt" | "scala" => "text/plain".to_string(),
        "html" | "htm" => "text/html".to_string(),
        "css" => "text/css".to_string(),
        "json" => "application/json".to_string(),
        "md" | "txt" | "log" => "text/plain".to_string(),
        "xml" => "application/xml".to_string(),
        "yaml" | "yml" => "application/yaml".to_string(),
        "toml" => "application/toml".to_string(),
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "gif" => "image/gif".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "ico" => "image/x-icon".to_string(),
        "pdf" => "application/pdf".to_string(),
        "zip" | "tar" | "gz" | "bz2" | "xz" => "application/zip".to_string(),
        "wasm" => "application/wasm".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// Send the stream result back as a `WsResponse::Result`.
async fn send_prompt_result(
    result: StreamResult,
    resp_tx: &mpsc::UnboundedSender<WsResponse>,
    id: Option<String>,
) {
    let _ = resp_tx.send(WsResponse::Status {
        streaming: false,
        message: None,
    });

    // Build a summary: first non-empty line, truncated to ~150 chars
    let summary = result
        .full_response
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|s| {
            if s.len() > 150 {
                let truncated: String = s.chars().take(147).collect();
                format!("{}…", truncated)
            } else {
                s.to_string()
            }
        })
        .unwrap_or_default();

    let response = WsResponse::Result {
        ok: true,
        summary,
        full_response: Some(result.full_response),
        error: None,
        id,
    };
    let _ = resp_tx.send(response);
}

/// Handle a command that does NOT require spawning a long-running task.
async fn handle_immediate_command(
    cmd: WsCommand,
    messages: &[Message],
    session_usage: &TokenUsage,
    _config: &my_code_agent::core::config::Config,
    resp_tx: &mpsc::UnboundedSender<WsResponse>,
) -> CommandResult {
    match cmd {
        WsCommand::GetHistory { id } => {
            let _ = resp_tx.send(WsResponse::History {
                messages: messages.to_vec(),
                id,
            });
            CommandResult::ResponseSent
        }

        WsCommand::Command { cmd, id } => {
            let response = match cmd.as_str() {
                "/status" => {
                    let turn_count = messages.iter().filter(|m| m.role == "user").count();
                    WsResponse::Result {
                        ok: true,
                        summary: format!(
                            "Agent running. {} user turns, {} total messages.",
                            turn_count,
                            messages.len(),
                        ),
                        full_response: None,
                        error: None,
                        id,
                    }
                }
                "/tokens" => WsResponse::Result {
                    ok: true,
                    summary: format!(
                        "Total tokens: {} (input: {}, output: {})",
                        session_usage.total_tokens(),
                        session_usage.input_tokens(),
                        session_usage.output_tokens(),
                    ),
                    full_response: None,
                    error: None,
                    id,
                },
                "/clear" => WsResponse::Result {
                    ok: true,
                    summary: "Use /clear on an idle agent (not supported during active prompt)."
                        .to_string(),
                    full_response: None,
                    error: None,
                    id,
                },
                _ => WsResponse::Error {
                    message: format!("Unsupported command in headless mode: {}", cmd),
                    id,
                },
            };
            let _ = resp_tx.send(response);
            CommandResult::ResponseSent
        }

        WsCommand::Interrupt { id } => {
            let _ = resp_tx.send(WsResponse::Result {
                ok: true,
                summary: "Nothing to interrupt.".to_string(),
                full_response: None,
                error: None,
                id,
            });
            CommandResult::ResponseSent
        }

        WsCommand::Ping { id } => {
            let _ = resp_tx.send(WsResponse::Pong { id });
            CommandResult::ResponseSent
        }

        WsCommand::ListSessions { id } => {
            let sessions = my_code_agent::core::session::SessionData::list_sessions();
            let session_infos: Vec<SessionInfo> = sessions
                .into_iter()
                .map(|s| SessionInfo {
                    name: s.name,
                    message_count: s.turns,
                    saved_at: s.saved_at,
                    id: s.id,
                })
                .collect();
            let _ = resp_tx.send(WsResponse::SessionList {
                sessions: session_infos,
                id,
            });
            CommandResult::ResponseSent
        }
        WsCommand::CreateSession { session_id, name, id } => {
            let session_id = session_id.unwrap_or_else(|| {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                format!("sess-{}-{}", timestamp, std::process::id())
            });
            let saved_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let session_data = my_code_agent::core::session::SessionData {
                chat_history: Vec::new(),
                token_usage: TokenUsage::new(),
                last_reasoning: String::new(),
                saved_at,
                name: name.clone(),
                id: Some(session_id.clone()),
            };
            match session_data.save_with_id(&session_id) {
                Ok(_) => {
                    let display_name = name.unwrap_or_else(|| session_id.clone());
                    let _ = resp_tx.send(WsResponse::Result {
                        ok: true,
                        summary: format!("Session {} created", display_name),
                        full_response: None,
                        error: None,
                        id,
                    });
                }
                Err(e) => {
                    let _ = resp_tx.send(WsResponse::Error {
                        message: format!("Failed to create session: {}", e),
                        id,
                    });
                }
            }
            CommandResult::ResponseSent
        }

        WsCommand::SwitchSession { session_id, name, id } => {
            // Try loading by session_id first, fall back to name for backward compatibility
            let load_result = session_id
                .as_ref()
                .and_then(|sid| my_code_agent::core::session::SessionData::load_by_id(sid))
                .or_else(|| {
                    name.as_ref()
                        .and_then(|n| my_code_agent::core::session::SessionData::load_by_name(n))
                });
            let display_name = name.clone().unwrap_or_else(|| session_id.clone().unwrap_or_default());
            match load_result {
                Some(Ok(session_data)) => {
                    let new_messages: Vec<Message> = session_data.chat_history;
                    let new_usage = session_data.token_usage.clone();
                    let _ = resp_tx.send(WsResponse::History {
                        messages: new_messages.clone(),
                        id: id.clone(),
                    });
                    let _ = resp_tx.send(WsResponse::Result {
                        ok: true,
                        summary: format!(
                            "Switched to session {} ({} messages)",
                            display_name,
                            new_messages.len()
                        ),
                        full_response: None,
                        error: None,
                        id,
                    });
                    CommandResult::SessionSwitched {
                        messages: new_messages,
                        session_usage: new_usage,
                        session_name: display_name,
                    }
                }
                Some(Err(e)) => {
                    let _ = resp_tx.send(WsResponse::Error {
                        message: format!("Failed to load session: {}", e),
                        id,
                    });
                    CommandResult::ResponseSent
                }
                None => {
                    let err_name = name.clone().unwrap_or_else(|| session_id.clone().unwrap_or_default());
                    let _ = resp_tx.send(WsResponse::Error {
                        message: format!("Session {} not found", err_name),
                        id,
                    });
                    CommandResult::ResponseSent
                }
            }
        }
        WsCommand::DeleteSession { session_id, name, id } => {
            // Try deleting by session_id first, fall back to name
            let result = match session_id {
                Some(ref sid) => my_code_agent::core::session::SessionData::delete_by_id(sid),
                None => {
                    match name {
                        Some(ref n) => my_code_agent::core::session::SessionData::delete_by_name(n),
                        None => Err("Neither session_id nor name provided".to_string()),
                    }
                }
            };
            match result {
                Ok(_) => {
                    let _ = resp_tx.send(WsResponse::Result {
                        ok: true,
                        summary: "Session deleted".to_string(),
                        full_response: None,
                        error: None,
                        id,
                    });
                }
                Err(e) => {
                    let _ = resp_tx.send(WsResponse::Error {
                        message: format!("Failed to delete session: {}", e),
                        id,
                    });
                }
            }
            CommandResult::ResponseSent
        }

        WsCommand::GetSessionInfo { id } => {
            let turn_count = messages.iter().filter(|m| m.role == "user").count();
            let _ = resp_tx.send(WsResponse::SessionInfoResponse {
                name: "current".to_string(),
                message_count: turn_count,
                id,
            });
            CommandResult::ResponseSent
        }

        WsCommand::Prompt { .. } => CommandResult::ResponseSent,
    }
}
