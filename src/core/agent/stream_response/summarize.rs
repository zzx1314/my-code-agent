use crate::core::agent::client::LlmClient;
use crate::core::types::Message;

/// Generate a concise summary of old conversation messages using a non-streaming
/// LLM call. Called during context compaction on the first compaction event to
/// preserve semantic content while saving tokens.
///
/// Returns the summary text on success. The caller should fall back to
/// [`ContextManager::prune_messages`] if this function fails.
pub(super) async fn generate_context_summary(
    client: &LlmClient,
    old_messages: &[Message],
    reasoning_field: &str,
) -> anyhow::Result<String> {
    let summary_prompt = Message::user(
        "Please provide a concise summary of the above conversation. \
         Focus on: user goals, decisions made, files changed, key findings, \
         and any important context that would help continue the work. \
         Keep the summary under 300 words and write in the same language as the conversation.",
    );
    let mut api_messages = vec![Message::system(
        "You are a helpful assistant that summarizes technical conversations concisely. \
                 Preserve all important context, decisions, and file paths.",
    )];
    api_messages.extend_from_slice(old_messages);
    api_messages.push(summary_prompt);

    let response = client.chat(&api_messages, &[], reasoning_field).await?;

    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in summary response"))?
        .to_string();

    Ok(content)
}
