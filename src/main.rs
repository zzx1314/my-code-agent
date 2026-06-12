use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use my_code_agent::app::bootstrap::init_app;
use my_code_agent::app::lifecycle::run_app;
use my_code_agent::core::agent::preamble::build_preamble;
use my_code_agent::core::agent::stream_response::{StreamEvent, StreamResult, stream_response};
use my_code_agent::core::context::context_manager::ContextManager;
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::core::types::Message;
use my_code_agent::core::ws_client::{WsCommand, WsResponse};

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
                            // Queue and handle immediately — interrupts are
                            // the only command that needs to cut through.
                            handle_immediate_command(
                                other, &messages, &session_usage,
                                &config, &resp_tx,
                            ).await;
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
                            // ⚡ Sync state back from the spawned task
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

                // ══ Stream streaming events from agent → WebSocket ══
                event = pp.event_rx.recv() => {
                    if let Some(event) = event {
                        forward_stream_event(&event, &resp_tx);
                    }
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
                            handle_immediate_command(
                                other, &messages, &session_usage,
                                &config, &resp_tx,
                            ).await;
                        }

                        None => {
                            tracing::info!("Command channel closed, exiting");
                            break;
                        }
                    }
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
                format!("{}…", &s[..147])
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
) {
    match cmd {
        WsCommand::GetHistory { id } => {
            let response = WsResponse::History {
                messages: messages.to_vec(),
                id,
            };
            let _ = resp_tx.send(response);
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
                "/tokens" => {
                    WsResponse::Result {
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
                    }
                }
                "/clear" => {
                    WsResponse::Result {
                        ok: true,
                        summary: "Use /clear on an idle agent (not supported during active prompt).".to_string(),
                        full_response: None,
                        error: None,
                        id,
                    }
                }
                _ => WsResponse::Error {
                    message: format!("Unsupported command in headless mode: {}", cmd),
                    id,
                },
            };
            let _ = resp_tx.send(response);
        }

        WsCommand::Interrupt { id } => {
            let _ = resp_tx.send(WsResponse::Result {
                ok: true,
                summary: "Nothing to interrupt.".to_string(),
                full_response: None,
                error: None,
                id,
            });
        }

        WsCommand::Ping { id } => {
            let _ = resp_tx.send(WsResponse::Pong { id });
        }

        // Prompt should never reach here — handled in the main loop
        WsCommand::Prompt { .. } => {
            // silently ignore
        }
    }
}
