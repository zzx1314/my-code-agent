use tokio::sync::mpsc;

use crate::app::{App, ChatEntry};
use crate::core::agent::review::{ReviewAgent, ReviewEvent};
use crate::core::types::review::{ReviewOutcome, ReviewVerdict};
use crate::ui::render::strip_model_metadata;

use super::state::cleanup_stream_state;

/// Process review events (phase progress updates) from the review agent
/// and write them to chat_history in real-time.
pub fn process_review_events(app: &mut App) {
    if let Some(ref mut rx) = app.review_event_rx {
        loop {
            match rx.try_recv() {
                Ok(ReviewEvent::Started { .. }) => {}
                Ok(ReviewEvent::Progress { .. }) => {
                    app.review_reasoning.clear();
                    app.review_feedback.clear();
                }
                Ok(ReviewEvent::ReasoningDelta(delta)) => {
                    app.review_reasoning.push_str(&delta);
                }
                Ok(ReviewEvent::ReviewFeedbackDelta(delta)) => {
                    app.review_feedback.push_str(&delta);
                }
                Ok(ReviewEvent::Completed { .. }) => {}
                Ok(ReviewEvent::Error { message }) => {
                    app.chat_history
                        .push(ChatEntry::assistant(format!("❌ {}", message)));
                    app.auto_scroll = true;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    app.review_event_rx = None;
                    break;
                }
            }
        }
    }
}

/// Check if an auto-review result has arrived from the async task
pub fn check_review_result(app: &mut App) {
    if let Some(ref mut rx) = app.review_result_rx {
        match rx.try_recv() {
            Ok(outcome) => {
                // Add the display text to chat history
                app.chat_history
                    .push(crate::app::ChatEntry::assistant(outcome.display_text));
                app.auto_scroll = true;

                // Set the completion message for status bar display (~3 seconds)
                let verdict_icon = outcome.verdict.icon();
                let verdict_label = outcome.verdict.label();
                let complete_msg = format!("{} Review: {}", verdict_icon, verdict_label);
                app.review_complete_message = Some(complete_msg);
                app.review_complete_verdict = Some(outcome.verdict.clone());
                app.review_complete_timer = 30; // ~3 seconds at ~10fps

                // Determine whether to re-trigger the main agent for fixes
                let should_fix = outcome.auto_trigger
                    && outcome.verdict != ReviewVerdict::Approved
                    && app.review_iteration < app.config.review.max_review_iterations;

                // Clear review reasoning display
                app.review_reasoning.clear();

                if should_fix {
                    let iteration = app.review_iteration;
                    let max_iterations = app.config.review.max_review_iterations;

                    // Update completion message to show iteration info
                    app.review_complete_message = Some(format!(
                        "{} Iteration {}/{} — Fixing...",
                        verdict_icon,
                        iteration + 1,
                        max_iterations,
                    ));
                    app.review_complete_verdict = Some(outcome.verdict.clone());

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

                    // Save current issues as previous_review_issues for fingerprint
                    // deduplication in the next iteration.
                    app.previous_review_issues = outcome
                        .report
                        .as_ref()
                        .map(|r| r.issues.clone())
                        .unwrap_or_default();

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
                    app.chat_history
                        .push(crate::app::ChatEntry::assistant(iteration_status));
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
                    app.previous_review_issues.clear(); // Clear for next review cycle
                    app.review_reasoning.clear();
                    app.review_feedback.clear();
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.review_result_rx = None;
                app.is_reviewing = false;
                app.review_iteration = 0;
                app.previous_review_issues.clear();
                app.review_reasoning.clear();
                app.review_feedback.clear();
                app.review_complete_message = Some("⚠️ Review Disconnected".to_string());
                app.review_complete_timer = 30;
                app.review_complete_verdict = None;
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

/// Trigger auto-review after file changes have been made via the main agent.
///
/// Extracted into a standalone function so it can also be called from
/// non-streaming code paths (e.g. direct tool calls in session context)
/// where `process_stream_result` is not invoked.
pub fn trigger_auto_review(app: &mut App) {
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
            tracing::info!("Auto-review triggered");

            // Add a visible message to chat history
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "🔍 **Auto-Review Started** — Analyzing recent code changes...".to_string(),
            ));
            app.auto_scroll = true;

            // Create channels: one for real-time phase events, one for the final result
            let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<ReviewEvent>();
            let (result_tx, result_rx) = tokio::sync::mpsc::channel::<ReviewOutcome>(1);

            app.review_event_rx = Some(event_rx);
            app.review_result_rx = Some(result_rx);

            let orchestrator = orchestrator.clone();
            let history_snapshot = app.chat_history.clone();
            // Pass previous review issues for fingerprint-based deduplication
            let previous_issues = app.previous_review_issues.clone();

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

                // Always detect all changes from HEAD (like codebuff's approach)
                let changed_files =
                    crate::core::agent::orchestrator::detect_changed_files_from_git().await;

                if changed_files.is_empty() {
                    tracing::info!("Auto-review: no changed files detected");
                    let _ = event_tx.send(ReviewEvent::Error {
                        message: "No code changes detected.".to_string(),
                    });
                    let outcome = ReviewOutcome {
                        display_text: "ℹ️ **Auto-Review Complete** — No code changes detected."
                            .to_string(),
                        verdict: ReviewVerdict::Approved,
                        report_summary: String::new(),
                        report: None,
                        auto_trigger: false,
                    };
                    let _ = result_tx.send(outcome).await;
                    return;
                }

                tracing::info!(count = changed_files.len(), "Auto-review started");

                // Extract user's original request from chat history as review context
                let context = ReviewAgent::extract_context_from_history(&messages);
                let context_opt = if context.is_empty() {
                    None
                } else {
                    Some(context)
                };

                // Extract conversation history summary for consistency checking
                let history_summary = ReviewAgent::extract_history_summary(&messages);

                // Use phased review with events — sends phase progress through event_tx
                match orchestrator
                    .review_with_events(
                        changed_files,
                        context_opt.as_deref(),
                        history_summary.as_deref(),
                        event_tx,
                    )
                    .await
                {
                    Ok(mut report) => {
                        // ── Fingerprint-based deduplication ─────────────────────
                        let before = report.issues.len();
                        report.issues =
                            crate::core::types::review::ReviewIssue::deduplicate_against(
                                report.issues,
                                &previous_issues,
                                &report.changed_files,
                            );
                        let dedup_count = before.saturating_sub(report.issues.len());
                        if dedup_count > 0 {
                            tracing::info!(
                                count = dedup_count,
                                "Auto-review: filtered duplicate issues from previous iteration"
                            );
                            report = orchestrator.review_agent.rebuild_report(
                                &report.issues,
                                &report.changed_files,
                                &report.llm_feedback,
                            );
                        }

                        let display_text = orchestrator.format_review_report(&report);
                        let report_summary = format!(
                            "Verdict: {} | Issues: {} (Critical: {}, High: {}, Medium: {}, Low: {})",
                            report.summary.verdict.label(),
                            report.issues.len(),
                            report.summary.critical_count,
                            report.summary.high_count,
                            report.summary.medium_count,
                            report.summary.low_count,
                        );
                        let verdict = report.summary.verdict.clone();
                        tracing::info!(
                            issues = report.issues.len(),
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

/// Process the final result of a streaming response
fn process_stream_result(app: &mut App, result: crate::core::agent::stream_response::StreamResult) {
    // ── Defensive filtering: strip model metadata lines that may have
    // slipped through during streaming (e.g. cross-chunk patterns).
    let result = crate::core::agent::stream_response::StreamResult {
        full_response: strip_model_metadata(&result.full_response),
        last_reasoning: strip_model_metadata(&result.last_reasoning),
        ..result
    };

    // Save streaming_todos before clearing — we'll re-add it after the
    // chat_history is synced so the plan stays visible after streaming
    // completes, right alongside the assistant's final response.
    let final_todos = app.streaming_todos.take();

    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_status.clear();

    app.streaming_tool_result = None;

    if !app.last_reasoning.is_empty() {
        app.streaming_reasoning.clear();
    } else if !result.last_reasoning.is_empty() {
        app.last_reasoning = result.last_reasoning;
        app.streaming_reasoning.clear();
    } else if !app.streaming_reasoning.is_empty() {
        app.last_reasoning = std::mem::take(&mut app.streaming_reasoning);
    } else {
        app.streaming_reasoning.clear();
    }

    for segment in app.completed_pre_text_segments.drain(..) {
        let trimmed = segment.trim_end();
        if !trimmed.is_empty() {
            if !app.last_reasoning.is_empty() {
                app.last_reasoning.push('\n');
            }
            app.last_reasoning.push_str(trimmed);
        }
    }

    for segment in app.completed_post_text_segments.drain(..) {
        let trimmed = segment.trim_end();
        if !trimmed.is_empty() {
            if !app.last_reasoning.is_empty() {
                app.last_reasoning.push('\n');
            }
            app.last_reasoning.push_str(trimmed);
        }
    }
    if !app.post_text_reasoning.is_empty() {
        let trimmed = app.post_text_reasoning.trim_end();
        if !trimmed.is_empty() {
            if !app.last_reasoning.is_empty() {
                app.last_reasoning.push('\n');
            }
            app.last_reasoning.push_str(trimmed);
        }
        app.post_text_reasoning.clear();
    }

    app.current_tool_call = None;
    app.streaming_events_rx = None;

    check_todo_continuation(app, &result.updated_history);

    if !result.updated_history.is_empty() {
        let pruned: Vec<crate::app::ChatEntry> = result
            .updated_history
            .into_iter()
            .filter(|m| {
                m.role != "system"
                    && (!m.content.is_empty()
                        || m.reasoning_content.is_some()
                        || m.tool_calls.is_some()
                        || m.tool_call_id.is_some())
            })
            .map(crate::app::ChatEntry::from_message)
            .collect();
        if !pruned.is_empty() {
            app.chat_history = pruned;
        }
    }

    if app.review_iteration > 0 {
        if let Some(idx) = app
            .chat_history
            .iter()
            .rposition(|e| e.role == "user" && is_auto_fix_prompt(&e.content))
        {
            let max_iterations = app.config.review.max_review_iterations;
            let iteration = app.review_iteration.min(max_iterations);
            app.chat_history[idx].content = format!(
                "🔄 Fixing issues (auto-review iteration {}/{})...",
                iteration, max_iterations,
            );
        }
    }

    if let Some(last) = app.chat_history.last_mut() {
        if last.role == "assistant" {
            let deduped = build_response_display(&last.content, &app.last_reasoning);
            last.content = deduped;
            if !app.last_reasoning.is_empty() && last.reasoning_content.is_none() {
                last.reasoning_content = Some(app.last_reasoning.clone());
            }
        } else {
            let display_text = build_response_display(&result.full_response, &app.last_reasoning);
            if !display_text.is_empty() {
                if !app.last_reasoning.is_empty() {
                    app.chat_history
                        .push(crate::app::ChatEntry::assistant_with_reasoning(
                            display_text,
                            &app.last_reasoning,
                        ));
                } else {
                    app.chat_history
                        .push(crate::app::ChatEntry::assistant(display_text));
                }
            } else if !app.last_reasoning.is_empty() {
                app.chat_history
                    .push(crate::app::ChatEntry::assistant_with_reasoning(
                        "",
                        &app.last_reasoning,
                    ));
            } else {
                app.chat_history
                    .push(crate::app::ChatEntry::assistant("_(no response)_"));
            }
        }
    } else {
        let display_text = build_response_display(&result.full_response, &app.last_reasoning);
        if !display_text.is_empty() {
            if !app.last_reasoning.is_empty() {
                app.chat_history
                    .push(crate::app::ChatEntry::assistant_with_reasoning(
                        display_text,
                        &app.last_reasoning,
                    ));
            } else {
                app.chat_history
                    .push(crate::app::ChatEntry::assistant(display_text));
            }
        } else if !app.last_reasoning.is_empty() {
            app.chat_history
                .push(crate::app::ChatEntry::assistant_with_reasoning(
                    "",
                    &app.last_reasoning,
                ));
        } else {
            app.chat_history
                .push(crate::app::ChatEntry::assistant("_(no response)_"));
        }
    }
    app.show_inline_reasoning = !app.last_reasoning.is_empty();

    if let Some(ref todos_md) = final_todos {
        if !todos_md.is_empty() {
            if let Some(last) = app.chat_history.last_mut() {
                if last.role == "assistant" {
                    // Add newline separator so the markdown header doesn't merge
                    // with the assistant's preceding text.
                    last.content.push('\n');
                    last.content.push_str(todos_md);
                }
            }
        }
    }

    app.token_usage = result.session_usage;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    let interval_ms = app.config.agent.response_interval_ms;
    if interval_ms > 0 {
        app.response_cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(interval_ms));
    }

    if result.should_exit {
        app.should_exit = true;
    }

    // ── Auto-review: trigger after main agent completes file changes ──────────
    trigger_auto_review(app);
}

const MAX_AUTO_CONTINUATIONS: u32 = 10;
const CONTINUATION_PREFIX: &str = "Continue working on the plan.";

/// Returns true if the last user message in `updated_history` looks like
/// a continuation prompt (meaning we're already in a continuation loop).
fn last_user_is_continuation(updated_history: &[crate::core::types::Message]) -> bool {
    updated_history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map_or(false, |m| m.content.starts_with(CONTINUATION_PREFIX))
}

/// Returns true if the model's most recent response (last assistant message
/// before any tool results) included tool calls, indicating active work.
fn last_response_had_tool_calls(updated_history: &[crate::core::types::Message]) -> bool {
    let mut saw_tool_result = false;
    for m in updated_history.iter().rev() {
        match m.role.as_str() {
            "tool" => saw_tool_result = true,
            "assistant" => {
                if m.tool_calls.is_some() || saw_tool_result {
                    return true;
                }
                return false;
            }
            _ => {}
        }
    }
    false
}

/// Read `.mycode/.todos.json` and push a continuation prompt to the message queue
/// when there are still pending or in-progress tasks.
fn check_todo_continuation(app: &mut App, updated_history: &[crate::core::types::Message]) {
    if app.continuation_count >= MAX_AUTO_CONTINUATIONS {
        return;
    }

    let is_continuation = last_user_is_continuation(updated_history);
    let was_working = last_response_had_tool_calls(updated_history);
    if !is_continuation && !was_working {
        return;
    }

    let path = crate::tools::infra::write_todos::TODOS_FILE_PATH;
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let todos: Vec<serde_json::Value> = match serde_json::from_str(&content) {
        Ok(t) => t,
        Err(_) => return,
    };

    let mut pending_tasks: Vec<String> = Vec::new();
    let mut completed_count = 0;
    for todo in &todos {
        let status = todo
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let task = todo.get("task").and_then(|t| t.as_str()).unwrap_or("");
        match status {
            "pending" | "in_progress" => {
                if !task.is_empty() {
                    pending_tasks.push(task.to_string());
                }
            }
            "completed" => completed_count += 1,
            _ => {}
        }
    }

    if pending_tasks.is_empty() {
        return;
    }

    app.continuation_count += 1;

    let list: String = pending_tasks
        .iter()
        .enumerate()
        .map(|(i, task)| format!("{}. {}", i + 1, task))
        .collect::<Vec<_>>()
        .join("\n");

    let total = completed_count + pending_tasks.len();
    let msg = format!(
        "Continue working on the plan. Remaining tasks ({}/{}):\n{}",
        completed_count, total, list,
    );
    app.message_queue.push(msg);
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

/// Check if a message content is an auto-fix prompt that should be hidden from chat display.
pub fn is_auto_fix_prompt(content: &str) -> bool {
    content.starts_with("## 🔄 Code Review - Iteration")
        || content.starts_with("Please fix the issues found in the code review")
}
