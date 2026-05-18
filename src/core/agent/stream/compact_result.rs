use crate::app::App;

/// Check if a /compact result has arrived from the async task
pub fn check_compact_result(app: &mut App) {
    if let Some(ref mut rx) = app.compact_rx {
        match rx.try_recv() {
            Ok(result) => {
                // Build the new chat_history: summary + retained messages
                let mut new_history = Vec::new();

                // Summary as system message (visible in UI)
                new_history.push(crate::app::ChatEntry {
                    role: "system".into(),
                    content: format!("📝 Previous conversation summary:\n{}", result.summary),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                });

                // Retained messages (the newest ones that weren't compressed)
                for msg in result.messages {
                    new_history.push(crate::app::ChatEntry::from_message(msg));
                }

                app.chat_history = new_history;

                // Show compaction stats as a status message
                app.status_messages.push(format!(
                    "✓ Compacted {} messages → summary ({} messages remaining, ~{} tokens saved)",
                    result.original_count,
                    result.retained_count + 1, // +1 for summary
                    result.tokens_saved,
                ));

                // Reset streaming state
                app.compact_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.auto_scroll = true;
                app.scroll = u16::MAX;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                app.status_messages
                    .push("✗ Compact task failed unexpectedly".into());
                app.compact_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.auto_scroll = true;
            }
        }
    }
}
