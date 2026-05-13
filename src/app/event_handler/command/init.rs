use crate::app::App;

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
            &agent_clone.client,
            &agent_clone.system_prompt,
            &prompt,
            &mut chat_history,
            &agent_clone.tools,
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
            crate::core::bootstrap::knowledge::generate_knowledge_content_local()
        } else {
            let raw = result.full_response.trim();
            let stripped = crate::core::bootstrap::knowledge::strip_code_fences(raw);
            let cleaned = crate::core::bootstrap::knowledge::strip_preamble_before_heading(stripped);
            tracing::info!(bytes = cleaned.len(), "Generated knowledge content via LLM");
            cleaned.to_string()
        };

        let init_result = crate::core::bootstrap::knowledge::build_init_result(
            &knowledge_file,
            &new_content,
            &config_clone,
            is_update,
        );
        init_tx.send(init_result).await.ok();
    });

    true
}

/// Build the LLM prompt for `/init` command
pub fn build_init_prompt(is_update: bool) -> String {
    if is_update {
        let existing_content =
            std::fs::read_to_string(crate::core::preamble::KNOWLEDGE_FILE).unwrap_or_default();
        format!(
            r#"You are a technical documentation expert. Your task is to UPDATE the project knowledge document.

Current knowledge document:
```markdown
{}
```

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to explore the current project structure
2. Check the project root for README.md, Cargo.toml, package.json, or similar config files
3. Look at the source directory structure to understand the codebase layout
4. Update the knowledge document to accurately reflect the current state of the project
5. Keep the existing Markdown structure but update all content
6. Add any new important files, modules, or patterns you discover
7. Remove references to files or features that no longer exist

## Output Rules
- Respond ONLY with the complete updated Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#,
            existing_content
        )
    } else {
        r#"You are a technical documentation expert. Your task is to CREATE a comprehensive project knowledge document.

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to thoroughly explore the project
2. Read README.md, Cargo.toml/package.json, and other config files in the project root
3. Explore the source directory structure (src/, lib/, etc.)
4. Identify the project type, key dependencies, architecture patterns, and conventions
5. Create a well-structured Markdown knowledge document

## Document Structure
The document should include these sections:
- **## What This Is** — Brief project description (from README or code)
- **## Features** — Key features and capabilities
- **## Project Structure** — Directory/file layout with descriptions
- **## Key Dependencies** — Major libraries and their purposes
- **## Configuration** — How the project is configured
- **## Conventions & Gotchas** — Important patterns, naming conventions, things to know

## Output Rules
- Respond ONLY with the complete Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#.to_string()
    }
}

