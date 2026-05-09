use crate::app::App;

use super::super::init::build_init_prompt;

pub(super) fn handle(app: &mut App) -> bool {
    let knowledge_file = crate::core::preamble::KNOWLEDGE_FILE.to_string();
    let is_update = std::path::Path::new(&knowledge_file).exists();
    let prompt = build_init_prompt(is_update);

    app.chat_history
        .push(("user".to_string(), "/init".to_string()));
    app.chat_history.push((
        "assistant".to_string(),
        if is_update {
            "⏳ Updating knowledge file — exploring project..."
        } else {
            "⏳ Creating knowledge file — exploring project..."
        }
        .to_string(),
    ));
    app.show_banner = false;
    app.auto_scroll = true;
    app.scroll = u16::MAX;

    // Set up streaming channels
    let (event_tx, event_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::core::streaming::StreamEvent>();
    app.streaming_events_rx = Some(event_rx);
    app.is_streaming = true;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;

    let agent_clone = app.agent.clone();
    let config_clone = app.config.clone();
    let (init_tx, init_rx) = tokio::sync::mpsc::channel::<crate::app::InitResult>(1);
    app.init_rx = Some(init_rx);

    let interrupt_rx = app.interrupt_tx.subscribe();

    tokio::spawn(async move {
        let mut chat_history = Vec::new();
        let mut token_usage = crate::core::token_usage::TokenUsage::with_config(&config_clone);
        let mut interrupt_rx = interrupt_rx;
        let mut ctx_mgr = crate::core::context_manager::ContextManager::new(&config_clone);

        let result = crate::core::streaming::stream_response(
            &agent_clone,
            &prompt,
            &mut chat_history,
            &mut token_usage,
            &mut interrupt_rx,
            &mut ctx_mgr,
            &config_clone.agent,
            Some(event_tx),
        )
        .await;

        // Extract content: use LLM response, or fallback to local generation
        let new_content = if result.full_response.is_empty() {
            tracing::warn!(
                "LLM returned empty response for /init, falling back to local generation"
            );
            crate::core::knowledge::generate_knowledge_content_local()
        } else {
            let raw = result.full_response.trim();
            let stripped = crate::core::knowledge::strip_code_fences(raw);
            let cleaned = crate::core::knowledge::strip_preamble_before_heading(stripped);
            tracing::info!(bytes = cleaned.len(), "Generated knowledge content via LLM");
            cleaned.to_string()
        };

        let init_result = crate::core::knowledge::build_init_result(
            &knowledge_file,
            &new_content,
            &config_clone,
            is_update,
        );
        init_tx.send(init_result).await.ok();
    });

    true
}
