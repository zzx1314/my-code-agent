use std::sync::Arc;
use tokio::sync::mpsc;

use crate::app::App;
use crate::core::context_manager::ContextManager;
use crate::core::preamble::Agent;
use crate::core::streaming::StreamEvent;

pub fn reset_streaming_state(app: &mut App) {
    app.is_streaming = true;
    app.auto_scroll = true;
    app.reasoning_auto_scroll = true;
    app.reasoning_scroll = 0;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.last_reasoning.clear();
    app.current_tool_call = None;
    app.current_response.clear();
    app.status_messages.clear();
    app.turn_usage_line = None;
}

pub fn spawn_llm_stream(app: &mut App, context_manager: &mut ContextManager, prompt: &str) {
    use crate::core::context::expand_file_refs;
    use crate::core::streaming::{StreamResult, stream_response};
    use crate::core::types::Message;

    let expanded = expand_file_refs(prompt, &app.config);

    let mut messages: Vec<Message> = app
        .chat_history
        .iter()
        .map(|(role, content)| Message {
            role: role.clone(),
            content: content.clone(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        })
        .collect();

    let agent_clone = app.agent.clone();
    let config_clone = app.config.clone();
    let mut token_usage_clone = app.token_usage.clone();
    let interrupt_rx = app.interrupt_tx.subscribe();
    let (response_tx, response_rx) = mpsc::channel::<StreamResult>(1);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<StreamEvent>();

    let mut ctx_mgr = context_manager.clone();

    app.response_rx = Some(response_rx);
    app.streaming_events_rx = Some(event_rx);

    tokio::spawn(async move {
        let mut interrupt_rx = interrupt_rx;

        // Pre-send pruning
        let estimated_new_input =
            ctx_mgr.estimate_messages_tokens(&[Message::user(&expanded.expanded)], false);
        let estimated_total =
            ctx_mgr.estimate_messages_tokens(&messages, true) + estimated_new_input;
        if ctx_mgr.should_prune_before_send(estimated_total) {
            messages = ctx_mgr.prune_messages(&messages);
        }

        let result = stream_response(
            &agent_clone.client,
            &agent_clone.system_prompt,
            &expanded.expanded,
            &mut messages,
            &agent_clone.tools,
            &mut token_usage_clone,
            &mut interrupt_rx,
            &mut ctx_mgr,
            &config_clone.agent,
            Some(event_tx),
        )
        .await;

        response_tx.send(result).await.ok();
    });
}

pub fn rebuild_agent(
    config: &crate::core::config::Config,
) -> anyhow::Result<Agent> {
    use crate::core::preamble::build_client;
    use crate::core::preamble::build_preamble;
    use crate::tools::create_mcp_tools;

    let client = build_client(config);
    let system_prompt = build_preamble();
    let mut tools = crate::core::tool::ToolRegistry::from_config(config);
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    for tool in mcp_tools {
        tools.register_dyn(tool);
    }

    Ok(Agent::new(client, system_prompt, tools))
}

pub fn process_streaming_events(app: &mut App) {
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(StreamEvent::Text(delta)) => {
                    if app.current_tool_call.is_some() {
                        app.streaming_text.push_str("\n\n");
                    }
                    app.streaming_text.push_str(&delta);
                    app.current_tool_call = None;
                }
                Ok(StreamEvent::ToolCall(name)) => {
                    app.current_tool_call = Some(name);
                }
                Ok(StreamEvent::ReasoningActive(active)) => {
                    if !active {
                        if !app.streaming_reasoning.is_empty() {
                            app.last_reasoning = app.streaming_reasoning.clone();
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(StreamEvent::ReasoningDelta(delta)) => {
                    // Some API providers send FULL accumulated reasoning_content
                    // in each SSE chunk rather than incremental deltas. If the
                    // new delta starts with what we already have, replace the
                    // buffer to avoid exponential duplication.
                    if delta.starts_with(&app.streaming_reasoning) {
                        app.streaming_reasoning = delta;
                    } else {
                        app.streaming_reasoning.push_str(&delta);
                    }
                    app.current_tool_call = None;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    app.streaming_events_rx = None;
                    break;
                }
            }
        }
    }
}

pub fn check_stream_result(app: &mut App) {
    if let Some(ref mut rx) = app.response_rx {
        match rx.try_recv() {
            Ok(result) => {
                if app.is_streaming {
                    process_stream_result(app, result);
                }
                app.response_rx = None;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                if app.is_streaming {
                    cleanup_stream_state(app);
                }
                app.response_rx = None;
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
    }
}

fn cleanup_stream_state(app: &mut App) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;
    app.streaming_events_rx = None;
    app.auto_scroll = true;
}

pub fn check_init_result(app: &mut App) {
    if let Some(ref mut rx) = app.init_rx {
        match rx.try_recv() {
            Ok(result) => {
                app.chat_history
                    .push(("assistant".to_string(), result.message));
                if let Some(new_agent) = result.new_agent {
                    app.agent = Arc::new(new_agent);
                }
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.auto_scroll = true;
                app.scroll = u16::MAX;
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.auto_scroll = true;
            }
        }
    }
}

fn process_stream_result(app: &mut App, result: crate::core::streaming::StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();

    // Use the authoritative reasoning from the backend ReasoningTracker.
    // The streaming events path can miss segments (e.g. multiple reasoning
    // segments or missing ReasoningActive(false) for tool-call transitions)
    // and may produce incomplete last_reasoning.
    if !result.last_reasoning.is_empty() {
        app.last_reasoning = result.last_reasoning;
        app.streaming_reasoning.clear();
    } else if app.last_reasoning.is_empty() && !app.streaming_reasoning.is_empty() {
        app.last_reasoning = std::mem::take(&mut app.streaming_reasoning);
    } else {
        app.streaming_reasoning.clear();
    }
    app.current_tool_call = None;
    app.streaming_events_rx = None;

    // Sync the full backend history first. This replaces the entire chat
    // history with the pruned message list from the streaming response.
    if !result.updated_history.is_empty() {
        let pruned: Vec<(String, String)> = result
            .updated_history
            .into_iter()
            .filter(|m| {
                if m.role == "tool" {
                    return false;
                }
                if m.role == "assistant" && m.tool_calls.is_some() {
                    return false;
                }
                m.role != "system" && !m.content.is_empty()
            })
            .map(|m| (m.role, m.content))
            .collect();
        if !pruned.is_empty() {
            app.chat_history = pruned;
        }
    }

    // Some providers duplicate reasoning_content into the start of the
    // content field, making the same thinking appear both in the reasoning
    // block and in the assistant message. Strip the overlapping prefix from
    // the last assistant message after the history sync.
    let has_assistant = app.chat_history.last().map(|(r, _)| r.as_str()) == Some("assistant");
    if has_assistant {
        let last = app.chat_history.last_mut().unwrap();
        let deduped = build_response_display(&last.1, &app.last_reasoning);
        last.1 = deduped;
    } else {
        // No assistant message yet (no content from model). If there's
        // reasoning, add a placeholder so the conversation shows progress.
        let display_text = build_response_display(&result.full_response, &app.last_reasoning);
        if !display_text.is_empty() {
            app.chat_history.push(("assistant".to_string(), display_text));
        } else if !app.last_reasoning.is_empty() {
            app.chat_history.push(("assistant".to_string(), "_(thinking)_".to_string()));
        }
    }
    app.show_inline_reasoning = !app.last_reasoning.is_empty();

    app.token_usage = result.session_usage;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
    }
}

/// Some API providers duplicate `reasoning_content` into the beginning of
/// the `content` field of the first text chunk. Strip this overlap so the
/// reasoning text appears only in the reasoning block, not also at the
/// start of the assistant message body.
fn build_response_display(full_response: &str, last_reasoning: &str) -> String {
    if full_response.is_empty() {
        return String::new();
    }
    if last_reasoning.is_empty() {
        return full_response.to_string();
    }

    let trimmed_reasoning = last_reasoning.trim_end();
    if full_response.starts_with(trimmed_reasoning) {
        let rest = full_response[trimmed_reasoning.len()..].trim_start();
        if rest.is_empty() {
            // All of the response text was reasoning — keep it as-is rather
            // than showing nothing.
            full_response.to_string()
        } else {
            rest.to_string()
        }
    } else {
        full_response.to_string()
    }
}

pub fn process_message_queue(app: &mut App, context_manager: &mut ContextManager) -> bool {
    if !app.is_streaming && !app.message_queue.is_empty() {
        let next_message = app.message_queue.remove(0);
        if let Some(last) = app.chat_history.last() {
            if last.1.starts_with("⏳ [Queued] ") {
                app.chat_history.pop();
            }
        }
        super::message::send_message_to_llm(app, context_manager, next_message);
        true
    } else {
        false
    }
}
