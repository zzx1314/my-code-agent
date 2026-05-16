use tokio::sync::mpsc;

use crate::app::{App, ChatEntry};
use crate::core::agent::review_agent::{ReviewAgent, ReviewEvent};
use crate::core::types::review::{ReviewOutcome, ReviewVerdict};

use super::state::cleanup_stream_state;

/// Process review events (phase progress updates) from the review agent
/// and write them to chat_history in real-time.
pub fn process_review_events(app: &mut App) {
    if let Some(ref mut rx) = app.review_event_rx {
        loop {
            match rx.try_recv() {
                Ok(ReviewEvent::PhaseCompleted {
                    phase_index,
                    total_phases,
                    phase_name,
                    categories: _,
                    issues_found,
                    passed: _passed,
                    details,
                }) => {
                    let prefix = if issues_found > 0 {
                        format!("⚠️ **Phase {}/{} — {}** ({} issue(s))\n",
                            phase_index, total_phases, phase_name, issues_found)
                    } else {
                        format!("✅ **Phase {}/{} — {}** (passed)\n",
                            phase_index, total_phases, phase_name)
                    };
                    let msg = format!("{}   {}", prefix, details);
                    app.chat_history.push(ChatEntry::assistant(msg));
                    app.auto_scroll = true;

                    // If this is the last phase, phase events are done;
                    // the final completed event will be handled by check_review_result.
                }
                Ok(ReviewEvent::Started { .. }) => {
                    // Already shown via the "Auto-Review Started" or "Reviewing..." message
                    // in result.rs or commands/review.rs respectively.
                    // Just leave it as visible status.
                }
                Ok(ReviewEvent::Progress { .. }) => {
                    // Clear accumulated reasoning from the previous phase when a new phase starts.
                    // This prevents multi-phase reasoning from cluttering the display.
                    app.review_reasoning.clear();
                }
                Ok(ReviewEvent::FileAnalyzed { file, issues_found }) => {
                    let msg = format!("📄 Analyzed `{}` — {} issue(s) found", file, issues_found);
                    app.chat_history.push(ChatEntry::assistant(msg));
                    app.auto_scroll = true;
                }
                Ok(ReviewEvent::ReasoningDelta(delta)) => {
                    // Accumulate reasoning deltas from streaming for frontend display.
                    // NOT added to chat history — only shown transiently in the UI.
                    app.review_reasoning.push_str(&delta);
                }
                Ok(ReviewEvent::Completed { .. }) => {
                    // Handled by check_review_result — it sends the final display_text
                }
                Ok(ReviewEvent::Error { message }) => {
                    app.chat_history.push(ChatEntry::assistant(format!("❌ {}", message)));
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
                app.chat_history.push(crate::app::ChatEntry::assistant(outcome.display_text));
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
                    app.review_reasoning.clear();
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.review_result_rx = None;
                app.is_reviewing = false;
                app.review_iteration = 0;
                app.review_reasoning.clear();
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

    // Hide auto-fix prompts from chat display — they contain the full review report
    // which is too verbose for the user. Replace with a concise status message.
    if app.review_iteration > 0 {
        if let Some(idx) = app.chat_history.iter().rposition(|e| e.role == "user" && is_auto_fix_prompt(&e.content)) {
            let max_iterations = app.config.review.max_review_iterations;
            let iteration = app.review_iteration.min(max_iterations);
            app.chat_history[idx].content = format!(
                "🔄 Fixing issues (auto-review iteration {}/{})...",
                iteration,
                max_iterations,
            );
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

            // Create channels: one for real-time phase events, one for the final result
            let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<ReviewEvent>();
            let (result_tx, result_rx) = tokio::sync::mpsc::channel::<ReviewOutcome>(1);

            app.review_event_rx = Some(event_rx);
            app.review_result_rx = Some(result_rx);

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
                    let _ = event_tx.send(ReviewEvent::Error {
                        message: "No code changes detected.".to_string(),
                    });
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

                // Extract user's original request from chat history as review context
                let context = ReviewAgent::extract_context_from_history(&messages);
                let context_opt = if context.is_empty() { None } else { Some(context) };

                // Use phased review with events — sends phase progress through event_tx
                match orchestrator.review_with_events(changed_files, context_opt.as_deref(), event_tx).await {
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

/// Check if a message content is an auto-fix prompt that should be hidden from chat display.
///
/// Auto-fix prompts are generated by `build_fix_prompt` in the orchestrator and
/// contain the full review report with all issues listed — too verbose for the user.
pub fn is_auto_fix_prompt(content: &str) -> bool {
    content.starts_with("## 🔄 Code Review - Iteration")
        || content.starts_with("Please fix the issues found in the code review")
}
