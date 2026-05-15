use tokio::sync::mpsc;

use crate::app::App;

use super::state::cleanup_stream_state;
/// Check if a streaming result has arrived from the async task
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

/// Process the final result of a streaming response
fn process_stream_result(app: &mut App, result: crate::core::agent::stream_response::StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_status.clear();

    // Use the authoritative reasoning from the backend ReasoningTracker.
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

    // Sync the full backend history first.
    if !result.updated_history.is_empty() {
        let pruned: Vec<crate::app::ChatEntry> = result
            .updated_history
            .into_iter()
            .filter(|m| {
                m.role != "system"
                    && (!m.content.is_empty() || m.reasoning_content.is_some() || m.tool_calls.is_some() || m.tool_call_id.is_some())
            })
            .map(crate::app::ChatEntry::from_message)
            .collect();
        if !pruned.is_empty() {
            app.chat_history = pruned;
        }
    }

    // Deduplicate reasoning prefix from the response.
    let has_assistant = app.chat_history.last().map(|e| e.role.as_str()) == Some("assistant");
    if has_assistant {
        let last = app.chat_history.last_mut().unwrap();
        let deduped = build_response_display(&last.content, &app.last_reasoning);
        last.content = deduped;
    } else {
        let display_text = build_response_display(&result.full_response, &app.last_reasoning);
        if !display_text.is_empty() {
            app.chat_history.push(crate::app::ChatEntry::assistant(display_text));
        } else if !app.last_reasoning.is_empty() {
            app.chat_history.push(crate::app::ChatEntry::assistant_with_reasoning(
                "",
                &app.last_reasoning,
            ));
        } else {
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "_(no response)_",
            ));
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

    // ── Auto-review: trigger after main agent completes file changes ──────────
    if let Some(ref orchestrator) = app.orchestrator {
        let should_review = {
            let history: Vec<crate::core::types::Message> = app
                .chat_history
                .iter()
                .map(|e| crate::core::types::Message {
                    role: e.role.clone(),
                    content: e.content.clone(),
                    reasoning_content: e.reasoning_content.clone(),
                    tool_calls: e.tool_calls.clone(),
                    tool_call_id: e.tool_call_id.clone(),
                })
                .collect();
            orchestrator.should_auto_review(&history) && !app.is_reviewing
        };

        if should_review {
            app.is_reviewing = true;
            tracing::info!("Auto-review triggered after main agent response");

            let orchestrator = orchestrator.clone();
            let history_snapshot = app.chat_history.clone();

            tokio::spawn(async move {
                let messages: Vec<crate::core::types::Message> = history_snapshot
                    .iter()
                    .map(|e| crate::core::types::Message {
                        role: e.role.clone(),
                        content: e.content.clone(),
                        reasoning_content: e.reasoning_content.clone(),
                        tool_calls: e.tool_calls.clone(),
                        tool_call_id: e.tool_call_id.clone(),
                    })
                    .collect();

                let changed_files = orchestrator.detect_changed_files(&messages);

                if changed_files.is_empty() {
                    tracing::info!("Auto-review: no changed files detected");
                    return;
                }

                tracing::info!(count = changed_files.len(), "Auto-review started");

                match orchestrator.review(changed_files, None).await {
                    Ok(report) => {
                        let _output = orchestrator.format_review_report(&report);
                        tracing::info!(
                            issues = report.summary.total_issues,
                            verdict = ?report.summary.verdict,
                            "Auto-review completed"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Auto-review failed");
                    }
                }
            });
        }
    }
}

/// Strip reasoning_content prefix from the response text if it was duplicated.
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
            full_response.to_string()
        } else {
            rest.to_string()
        }
    } else {
        full_response.to_string()
    }
}
