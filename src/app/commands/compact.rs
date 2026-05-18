use crate::app::App;
use crate::core::context::context_manager::ContextManager;

/// Result returned from the async compact task
pub struct CompactResult {
    /// LLM-generated summary of the compressed messages
    pub summary: String,
    /// Retained messages (the newest ones that weren't compressed)
    pub messages: Vec<crate::core::types::Message>,
    /// Total number of messages before compaction
    pub original_count: usize,
    /// Number of messages retained (excluding the summary)
    pub retained_count: usize,
    /// Estimated tokens saved by compaction
    pub tokens_saved: u64,
}

/// Handle /compact command — manually compact the conversation context
///
/// Usage:
///   /compact           — compact old messages, retain newest 30%
///   /compact N         — retain newest N% (1-99)
///   /compact --all     — maximum compression, keep only summary + last 2 turns
///   /compact --preview — show what would be compacted without doing it
pub fn handle(app: &mut App, input: &str, context_manager: &ContextManager) -> bool {
    // Don't compact while streaming
    if app.is_streaming {
        app.status_messages
            .push("⚠ Cannot compact while streaming — wait for the response to finish".into());
        return true;
    }

    let args = input.trim().strip_prefix("/compact").unwrap_or("").trim();

    // Parse arguments
    let (retain_percent, preview_only) = match args {
        "" => (app.config.context.compact_retain_percent, false),
        "--all" => (5, false),
        "--preview" => (app.config.context.compact_retain_percent, true),
        s if s.starts_with("--preview") => {
            // /compact --preview 50
            let rest = s.strip_prefix("--preview").unwrap().trim();
            let pct = rest.parse::<u64>().unwrap_or(app.config.context.compact_retain_percent);
            (pct.clamp(1, 99), true)
        }
        s => match s.parse::<u64>() {
            Ok(n) if (1..=99).contains(&n) => (n, false),
            _ => {
                app.status_messages.push(
                    "Usage: /compact [N%] [--all] [--preview]\n  \
                     N     — retain newest N% of messages (1-99)\n  \
                     --all — maximum compression\n  \
                     --preview — show compaction plan without executing"
                        .into(),
                );
                return true;
            }
        },
    };

    // Convert chat_history to Message for token estimation
    let messages: Vec<crate::core::types::Message> = app
        .chat_history
        .iter()
        .map(|entry| crate::core::types::Message {
            role: entry.role.clone(),
            content: entry.content.clone(),
            reasoning_content: entry.reasoning_content.clone(),
            tool_calls: entry.tool_calls.clone(),
            tool_call_id: entry.tool_call_id.clone(),
        })
        .collect();

    // Need at least 6 messages to make compaction worthwhile
    if messages.len() < 6 {
        app.status_messages.push(
            format!(
                "ℹ Only {} message(s) — nothing to compact (need at least 6)",
                messages.len()
            ),
        );
        return true;
    }

    // Find the compaction point
    let compact_point = context_manager.find_compact_point_percent(&messages, retain_percent);

    let Some(point) = compact_point else {
        app.status_messages
            .push("ℹ All messages fit within the retention budget — nothing to compact".into());
        return true;
    };

    let total_tokens: u64 = messages
        .iter()
        .map(|m| ContextManager::estimate_message_tokens(m))
        .sum();
    let retained_tokens: u64 = messages[point..]
        .iter()
        .map(|m| ContextManager::estimate_message_tokens(m))
        .sum();
    let tokens_to_compress = total_tokens - retained_tokens;

    // Preview mode — show plan without executing
    if preview_only {
        app.status_messages.push(format!(
            "📋 Compact preview (retain {}%):\n  \
             • {} messages to compress ({}..{})\n  \
             • {} messages to retain ({}..{})\n  \
             • ~{} tokens to compress, ~{} tokens to keep\n  \
             Run `/compact {}` to execute",
            retain_percent,
            point,
            "1..={}",
            point.saturating_sub(1),
            messages.len() - point,
            point + 1,
            messages.len(),
            tokens_to_compress,
            retained_tokens,
            retain_percent,
        ));
        return true;
    }

    // Prepare data for async task
    let old_messages = messages[..point].to_vec();
    let retained_messages = messages[point..].to_vec();
    let reasoning_field = app.config.llm.reasoning_field.clone();

    let client = app.agent.client.clone();
    let original_count = messages.len();
    let retained_count = retained_messages.len();

    // Block user input during compaction
    app.is_streaming = true;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.status_messages
        .push("⏳ Compacting conversation — generating summary...".into());

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    app.compact_rx = Some(rx);

    tokio::spawn(async move {
        let summary = generate_compact_summary(&client, &old_messages, &reasoning_field)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "LLM summarization failed for compact, using fallback");
                generate_fallback_summary(&old_messages)
            });

        let _ = tx
            .send(CompactResult {
                summary,
                messages: retained_messages,
                original_count,
                retained_count,
                tokens_saved: tokens_to_compress,
            })
            .await;
    });

    true
}

/// Generate a summary using the LLM
async fn generate_compact_summary(
    client: &crate::core::agent::client::LlmClient,
    old_messages: &[crate::core::types::Message],
    reasoning_field: &str,
) -> anyhow::Result<String> {
    let mut api_messages = vec![crate::core::types::Message::system(
        "You are a helpful assistant that summarizes technical conversations concisely. \
         Preserve all important context, decisions, and file paths. \
         Write in the same language as the conversation.",
    )];
    api_messages.extend_from_slice(old_messages);
    api_messages.push(crate::core::types::Message::user(
        "Please provide a concise summary of the above conversation. \n\
         Focus on:\n\
         - User's goals and requirements\n\
         - Key decisions made\n\
         - Files read, modified, or created (with full paths)\n\
         - Important findings or issues discovered\n\
         - Current progress and next steps\n\
         Keep the summary under 300 words. Write in the same language as the conversation.",
    ));

    let response = client.chat(&api_messages, &[], reasoning_field).await?;

    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in compact summary response"))?
        .to_string();

    Ok(content)
}

/// Fallback summary when LLM is unavailable — extracts key info mechanically
fn generate_fallback_summary(old_messages: &[crate::core::types::Message]) -> String {
    let mut summary = String::from("[Auto-generated fallback summary]\n\n");

    let mut user_goals = Vec::new();
    let mut file_paths = Vec::new();

    for msg in old_messages {
        if msg.role == "user" && !msg.content.is_empty() {
            // Keep first 100 chars of each user message as goal hint
            let preview: String = msg.content.chars().take(100).collect();
            if !preview.starts_with('[') {
                // Skip system-injected messages
                user_goals.push(preview);
            }
        }

        // Extract file paths mentioned in any message
        for word in msg.content.split_whitespace() {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
            if (clean.starts_with("src/") || clean.starts_with("./") || clean.contains("/"))
                && (clean.ends_with(".rs")
                    || clean.ends_with(".ts")
                    || clean.ends_with(".js")
                    || clean.ends_with(".py")
                    || clean.ends_with(".toml")
                    || clean.ends_with(".json")
                    || clean.ends_with(".md"))
                && !file_paths.contains(&clean.to_string())
            {
                file_paths.push(clean.to_string());
            }
        }
    }

    if !user_goals.is_empty() {
        summary.push_str("User requests:\n");
        for (i, goal) in user_goals.iter().take(5).enumerate() {
            summary.push_str(&format!("  {}. {}\n", i + 1, goal));
        }
        if user_goals.len() > 5 {
            summary.push_str(&format!("  ... and {} more\n", user_goals.len() - 5));
        }
    }

    if !file_paths.is_empty() {
        summary.push_str("\nFiles discussed:\n");
        for path in file_paths.iter().take(10) {
            summary.push_str(&format!("  - {}\n", path));
        }
    }

    summary
}
