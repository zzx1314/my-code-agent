//! Knowledge file generation and management for the `/init` command.

pub mod detect;
pub mod sections;

use crate::app::InitResult;
use crate::core::agent::preamble::Agent;
use crate::tools::create_mcp_tools;

pub use sections::generate_knowledge_content_local;

pub fn strip_code_fences(raw: &str) -> &str {
    if raw.starts_with("```") && raw.ends_with("```") {
        let inner = &raw[raw.find('\n').unwrap_or(3)..raw.len() - 3];
        inner.trim()
    } else {
        raw
    }
}

/// Strip any preamble text before the first Markdown heading (#).
/// LLMs sometimes prepend explanatory text like "Here is the knowledge document:"
/// before the actual content. This function finds the first line starting with `#`
/// and removes everything before it.
pub fn strip_preamble_before_heading(raw: &str) -> &str {
    for (i, line) in raw.lines().enumerate() {
        if line.starts_with('#') {
            // Found the first heading — return from here
            let offset: usize = raw.lines().take(i).map(|l| l.len() + 1).sum();
            return &raw[offset..];
        }
    }
    // No heading found — return as-is
    raw
}

// ── Result building ──────────────────────────────────────────────────────────

/// Write knowledge content to disk and rebuild the agent, returning an `InitResult`
pub fn build_init_result(
    knowledge_file: &str,
    new_content: &str,
    config: &crate::core::config::Config,
    is_update: bool,
) -> InitResult {
    let action = if is_update { "Updated" } else { "Created" };
    match std::fs::write(knowledge_file, new_content) {
        Ok(_) => match rebuild_agent(config) {
            Ok(new_agent) => InitResult {
                message: format!(
                    "✅ {} '{}' ({} bytes) with current project info.\nAgent reloaded with updated knowledge.",
                    action,
                    knowledge_file,
                    new_content.len()
                ),
                new_agent: Some(new_agent),
            },
            Err(e) => InitResult {
                message: format!(
                    "✅ {} '{}' with current project info.\n⚠️ Failed to reload agent: {}",
                    action, knowledge_file, e
                ),
                new_agent: None,
            },
        },
        Err(e) => InitResult {
            message: format!("❌ Failed to write '{}': {}", knowledge_file, e),
            new_agent: None,
        },
    }
}

/// Rebuild the agent (used for /init)
fn rebuild_agent(config: &crate::core::config::Config) -> anyhow::Result<Agent> {
    use crate::core::agent::preamble::{build_client, build_preamble};
    use crate::tools::ToolRegistry;
    let client = build_client(config);
    let system_prompt = build_preamble();
    let mut tools = ToolRegistry::from_config(config);
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    for tool in mcp_tools {
        tools.register_boxed(tool);
    }
    Ok(Agent::new(client, system_prompt, tools))
}
