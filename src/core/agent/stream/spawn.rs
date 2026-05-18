use tokio::sync::mpsc;

use crate::app::App;
use crate::core::context::context_manager::ContextManager;
use crate::core::agent::preamble::Agent;

use super::result::is_auto_fix_prompt;
use super::state::reset_streaming_state;

/// Send a message to the LLM (extracted for reuse by message queue)
pub fn send_message_to_llm(
    app: &mut App,
    context_manager: &mut ContextManager,
    input_text: String,
) {
    use tui_textarea::TextArea;

    app.show_banner = false; // Hide startup banner
    let display_text = if is_auto_fix_prompt(&input_text) {
        let max_iterations = app.config.review.max_review_iterations;
        let iteration = app.review_iteration.min(max_iterations);
        format!(
            "🔄 Fixing issues (auto-review iteration {}/{})...",
            iteration, max_iterations,
        )
    } else {
        input_text.clone()
    };
    app.chat_history.push(crate::app::ChatEntry::user(display_text));
    app.input = {
        let mut ta = TextArea::default();
        ta.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
        );
        ta.set_cursor_line_style(ratatui::style::Style::default());
        ta
    };
    reset_streaming_state(app);
    spawn_llm_stream(app, context_manager, &input_text);
}

/// Spawn an LLM streaming response
pub fn spawn_llm_stream(app: &mut App, context_manager: &mut ContextManager, prompt: &str) {
    use crate::core::context::expand_file_refs;
    use crate::core::agent::stream_response::{StreamResult, stream_response};
    use crate::core::types::Message;

    let expanded = expand_file_refs(prompt, &app.config);

    let mut messages: Vec<Message> = app
        .chat_history
        .iter()
        .map(|entry| Message {
            role: entry.role.clone(),
            content: entry.content.clone(),
            reasoning_content: entry.reasoning_content.clone(),
            tool_calls: entry.tool_calls.clone(),
            tool_call_id: entry.tool_call_id.clone(),
        })
        .collect();

    let agent_clone = app.agent.clone();
    let config_clone = app.config.clone();
    let mut token_usage_clone = app.token_usage.clone();
    let interrupt_rx = app.interrupt_tx.subscribe();
    let (response_tx, response_rx) = mpsc::channel::<StreamResult>(1);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<crate::core::agent::stream_response::StreamEvent>();

    let mut ctx_mgr = context_manager.clone();
    let prompt_owned = prompt.to_string();

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
            &config_clone.llm.reasoning_field,
        )
        .await;

        // Restore original @filename in updated_history — stream_response
        // overwrote the last user message with expanded file content for the
        // LLM, but chat_history syncs this back to the display, which should
        // show the user's original @reference, not raw file text.
        let mut result = result;
        if let Some(last_user) = result.updated_history.iter_mut().rfind(|m| m.role == "user") {
            last_user.content = prompt_owned;
        }

        response_tx.send(result).await.ok();
    });
}

/// Rebuild the Agent from config (client, system prompt, tools)
pub fn rebuild_agent(
    config: &crate::core::config::Config,
) -> anyhow::Result<Agent> {
    use crate::core::agent::preamble::build_client;
    use crate::core::agent::preamble::build_preamble;
    use crate::tools::create_mcp_tools;

    let client = build_client(config);
    let system_prompt = build_preamble();
    let mut tools = crate::tools::ToolRegistry::from_config(config);
    tools.register(crate::tools::SpawnAgents::new(client.clone(), config.llm.reasoning_field.clone()));
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    for tool in mcp_tools {
        tools.register_boxed(tool);
    }

    Ok(Agent::new(client, system_prompt, tools))
}
