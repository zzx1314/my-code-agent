use tokio::sync::mpsc;

use crate::app::App;
use crate::core::types::review::{ReviewOutcome, ReviewVerdict};

use super::state::cleanup_stream_state;

/// Check if an auto-review result has arrived from the async task
pub fn check_review_result(app: &mut App) {
    if let Some(ref mut rx) = app.review_result_rx {
        match rx.try_recv() {
            Ok(outcome) => {
                // Add the display text to chat history
                app.chat_history.push(crate::app::ChatEntry::assistant(outcome.display_text));
                app.auto_scroll = true;

                // Determine whether to re-trigger the main agent for fixes
                let should_fix = outcome.auto_trigger
                    && outcome.verdict != ReviewVerdict::Approved
                    && app.review_iteration < app.config.review.max_review_iterations;

                if should_fix {
                    let iteration = app.review_iteration;
                    let max_iterations = app.config.review.max_review_iterations;

                    // Build fix prompt from the report
                    let fix_prompt = if let Some(ref report) = outcome.report {
                        if let Some(ref orchestrator) = app.orchestrator {
                            orchestrator.build_fix_prompt(report, iteration, max_iterations)
                        } else {
                            format!(
                                "Please fix the issues found in the code review (iteration {}/{}). The review needs revision.",
                                iteration + 1,
                                max_iterations,
                            )
                        }
                    } else {
                        format!(
                            "Please fix the issues found in the code review (iteration {}/{}) so the code passes review.",
                            iteration + 1,
                            max_iterations,
                        )
                    };

                    app.review_iteration += 1;
                    app.is_reviewing = false;
                    app.review_result_rx = None;

                    // Add a status message indicating re-review cycle
                    let iteration_status = if iteration + 1 >= max_iterations {
                        format!(
                            "🔄 **Auto-Review Iteration {}/{}** — Last chance! Fixing issues...",
                            iteration + 1,
                            max_iterations,
                        )
                    } else {
                        format!(
                            "🔄 **Auto-Review Iteration {}/{}** — Issues found, fixing...",
                            iteration + 1,
                            max_iterations,
                        )
                    };
                    app.chat_history.push(crate::app::ChatEntry::assistant(iteration_status));
                    app.auto_scroll = true;

                    // Push the fix prompt to message queue so the event loop picks it up
                    app.message_queue.push(fix_prompt);
                } else {
                    // Review complete — show final status
                    if outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved {
                        if app.review_iteration >= app.config.review.max_review_iterations {
                            app.chat_history.push(crate::app::ChatEntry::assistant(
                                "⚠️ **Max review iterations reached.** Manual intervention may be required.".to_string(),
                            ));
                        }
                    }
                    app.is_reviewing = false;
                    app.review_result_rx = None;
                    app.review_iteration = 0; // Reset for next cycle
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.review_result_rx = None;
                app.is_reviewing = false;
                app.review_iteration = 0;
            }
        }
    }
}

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

            // Add a visible message to chat history
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "🔍 **Auto-Review Started** — Analyzing recent code changes...".to_string(),
            ));
            app.auto_scroll = true;

            let orchestrator = orchestrator.clone();
            let history_snapshot = app.chat_history.clone();

            // Create channels for review result communication
            let (result_tx, result_rx) = tokio::sync::mpsc::channel::<ReviewOutcome>(1);
            app.review_result_rx = Some(result_rx);

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
                    let outcome = ReviewOutcome {
                        display_text: "ℹ️ **Auto-Review Complete** — No code changes detected.".to_string(),
                        verdict: ReviewVerdict::Approved,
                        report_summary: String::new(),
                        report: None,
                        auto_trigger: false,
                    };
                    let _ = result_tx.send(outcome).await;
                    return;
                }

                tracing::info!(count = changed_files.len(), "Auto-review started");

                match orchestrator.review(changed_files, None).await {
                    Ok(report) => {
                        let display_text = orchestrator.format_review_report(&report);
                        let report_summary = format!(
                            "Verdict: {} | Score: {:.0}/100 | Issues: {} (Critical: {}, High: {}, Medium: {}, Low: {})",
                            report.summary.verdict.label(),
                            report.summary.overall_score,
                            report.summary.total_issues,
                            report.summary.critical_count,
                            report.summary.high_count,
                            report.summary.medium_count,
                            report.summary.low_count,
                        );
                        let verdict = report.summary.verdict.clone();

                        tracing::info!(
                            issues = report.summary.total_issues,
                            verdict = ?verdict,
                            "Auto-review completed"
                        );

                        let outcome = ReviewOutcome {
                            display_text,
                            verdict,
                            report_summary,
                            report: Some(report),
                            auto_trigger: true, // auto-review triggers iterative fix loop
                        };
                        let _ = result_tx.send(outcome).await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Auto-review failed");
                        let outcome = ReviewOutcome {
                            display_text: format!("⚠️ **Auto-Review Failed** — {e}"),
                            verdict: ReviewVerdict::NeedsRevision,
                            report_summary: String::new(),
                            report: None,
                            auto_trigger: false, // don't loop on errors
                        };
                        let _ = result_tx.send(outcome).await;
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
