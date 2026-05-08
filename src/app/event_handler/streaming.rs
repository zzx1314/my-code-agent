use std::sync::Arc;
use tokio::sync::mpsc;

use crate::app::App;
use crate::core::context_manager::ContextManager;
use crate::core::streaming::StreamEvent;

/// Reset all streaming-related state in the app
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
    app.streaming_status_messages.clear();
    app.turn_usage_line = None;
}

/// Spawn an async LLM streaming task with the given prompt
pub fn spawn_llm_stream(app: &mut App, context_manager: &mut ContextManager, prompt: &str) {
    use crate::app::conversion::convert_app_to_rig;
    use crate::core::context::expand_file_refs;
    use crate::core::streaming::{StreamResult, stream_response};

    let expanded = expand_file_refs(prompt, &app.config);

    let mut rig_chat_history = convert_app_to_rig(&app.chat_history);
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

        let result = stream_response(
            &agent_clone,
            &expanded.expanded,
            &mut rig_chat_history,
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

/// Rebuild the agent (used for model/provider switching)
pub fn rebuild_agent(
    config: &crate::core::config::Config,
) -> anyhow::Result<crate::core::preamble::Agent> {
    use crate::core::preamble::build_agent;
    use crate::tools::create_mcp_tools;

    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    Ok(build_agent(config, mcp_tools))
}

/// Process streaming events
pub fn process_streaming_events(app: &mut App) {
    // Poll streaming text events for live display
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
                        // Reasoning ended, save content to last_reasoning
                        if !app.streaming_reasoning.is_empty() {
                            app.last_reasoning = app.streaming_reasoning.clone();
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(StreamEvent::ReasoningDelta(delta)) => {
                    app.streaming_reasoning.push_str(&delta);
                    app.current_tool_call = None;
                }
                Ok(StreamEvent::PlanProgress(msg)) => {
                    app.streaming_status_messages.push(msg);
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

/// Check for completed stream result
pub fn check_stream_result(app: &mut App) {
    if let Some(ref mut rx) = app.response_rx {
        match rx.try_recv() {
            Ok(result) => {
                // Only process results while still in streaming state
                // If already force-cleaned by Esc (is_streaming=false), discard old results
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
    app.streaming_status_messages.clear();
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
                // Clean up all streaming state
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.streaming_status_messages.clear();
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
                app.streaming_status_messages.clear();
                app.auto_scroll = true;
            }
        }
    }
}

/// Process stream result
fn process_stream_result(app: &mut App, result: crate::core::streaming::StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();
    // Save any remaining reasoning that wasn't already moved to last_reasoning
    // (ReasoningActive(false) may not have been sent before stream ended)
    if app.last_reasoning.is_empty() && !app.streaming_reasoning.is_empty() {
        app.last_reasoning = std::mem::take(&mut app.streaming_reasoning);
    } else {
        app.streaming_reasoning.clear();
    }
    app.current_tool_call = None;
    app.streaming_events_rx = None;
    app.streaming_status_messages.clear();

    if !result.full_response.is_empty() {
        app.chat_history
            .push(("assistant".to_string(), result.full_response.clone()));
        // Mark that reasoning should be rendered inline with this LLM response
        app.show_inline_reasoning = !app.last_reasoning.is_empty();
    }

    app.token_usage = result.session_usage;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
    }
}

/// Process queued messages after streaming completes.
/// Returns true if there was a queued message and it started sending.
pub fn process_message_queue(app: &mut App, context_manager: &mut ContextManager) -> bool {
    if !app.is_streaming && !app.message_queue.is_empty() {
        let next_message = app.message_queue.remove(0);
        // Remove the "[Queued]" entry from chat history since we're now sending the real message
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
