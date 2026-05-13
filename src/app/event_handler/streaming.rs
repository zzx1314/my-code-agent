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
                    app.streaming_reasoning.push_str(&delta);
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
    if app.last_reasoning.is_empty() && !app.streaming_reasoning.is_empty() {
        app.last_reasoning = std::mem::take(&mut app.streaming_reasoning);
    } else {
        app.streaming_reasoning.clear();
    }
    app.current_tool_call = None;
    app.streaming_events_rx = None;

    if !result.full_response.is_empty() {
        app.chat_history
            .push(("assistant".to_string(), result.full_response.clone()));
        app.show_inline_reasoning = !app.last_reasoning.is_empty();
    }

    // Sync pruned history back
    if !result.updated_history.is_empty() {
        let pruned: Vec<(String, String)> = result
            .updated_history
            .into_iter()
            .filter(|m| m.role != "system" && !m.content.is_empty())
            .map(|m| (m.role, m.content))
            .collect();
        if !pruned.is_empty() {
            app.chat_history = pruned;
        }
    }

    app.token_usage = result.session_usage;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
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
