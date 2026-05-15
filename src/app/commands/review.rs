//! `/review` command — Manually trigger code review
//!
//! Syntax:
//!   /review              — Review code involved in the current conversation
//!   /review <path>       — Review code at the specified path
//!   /review --auto       — Toggle auto-review mode on/off

use crate::app::App;
use crate::core::context::context_manager::ContextManager;
use crate::core::types::review::{ChangedFile, ChangeType};

/// Handle `/review` command
pub fn handle(app: &mut App, input: &str, _context_manager: &mut ContextManager) -> bool {
    let args = input.trim();
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.get(1).copied() {
        Some("--auto" | "-a") => {
            toggle_auto_review(app);
            true
        }
        Some("--help" | "-h") => {
            show_help(app);
            true
        }
        Some(path) => {
            let path_str = path.to_string();
            app.chat_history.push(crate::app::ChatEntry::assistant(
                format!("🔍 Reviewing `{}`...", path_str),
            ));
            spawn_review(app, Some(path_str));
            false
        }
        None => {
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "🔍 Reviewing recent code changes...".to_string(),
            ));
            spawn_review(app, None);
            false
        }
    }
}

/// Toggle auto-review on/off
fn toggle_auto_review(app: &mut App) {
    let new_state = {
        let orchestrator = match app.orchestrator.as_mut() {
            Some(o) => o,
            None => {
                app.chat_history.push(crate::app::ChatEntry::assistant(
                    "⚠️ Review system not initialized. Please restart the app.".to_string(),
                ));
                return;
            }
        };

        // Get unique ownership via Arc::get_mut (refcount should be 1)
        let orch = std::sync::Arc::get_mut(orchestrator)
            .expect("Orchestrator should have unique ownership at this point");
        let new_state = !orch.auto_review_enabled;
        orch.auto_review_enabled = new_state;
        new_state
    };

    let status = if new_state { "✅ Enabled" } else { "❌ Disabled" };
    app.chat_history.push(crate::app::ChatEntry::assistant(
        format!("**Auto Code Review** {} Auto-review is now {}", status, if new_state { "enabled" } else { "disabled" }),
    ));
}

/// Show help
fn show_help(app: &mut App) {
    app.chat_history.push(crate::app::ChatEntry::assistant(
        "\
/review command — Code Review

**Usage:**
- `/review` — Review code changes involved in the current conversation
- `/review <path>` — Review the specified file or directory
- `/review --auto` or `/review -a` — Toggle auto-review mode
- `/review --help` or `/review -h` — Show this help

**Auto Review:**
After the main agent completes code modifications, the review agent will automatically analyze the changed code.
You can enable or disable this feature with `/review --auto`."
            .trim(),
    ));
}

/// Execute review asynchronously
fn spawn_review(app: &mut App, path: Option<String>) {
    let orchestrator = match app.orchestrator.clone() {
        Some(o) => o,
        None => {
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "⚠️ Review system not initialized. Please restart the app.".to_string(),
            ));
            return;
        }
    };

    // Take snapshot from chat_history
    let history_snapshot: Vec<crate::app::ChatEntry> = app.chat_history.clone();

    let (result_tx, result_rx) = tokio::sync::mpsc::channel::<crate::core::types::review::ReviewOutcome>(1);
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<ReviewEvent>();

    app.review_event_rx = Some(event_rx);
    app.is_reviewing = true;

    // Save result_rx for later result checking
    app.review_result_rx = Some(result_rx);

    tokio::spawn(async move {
        let changed_files = if let Some(ref path) = path {
            vec![ChangedFile {
                path: path.clone(),
                change_type: ChangeType::Modified,
                lines_added: 0,
                lines_removed: 0,
                diff: String::new(),
            }]
        } else {
            // Detect changes from conversation history snapshot
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
            orchestrator.detect_changed_files(&messages)
        };

        if changed_files.is_empty() {
            let msg = if path.is_some() {
                "No code file found at the specified path. Please verify the path is correct.".to_string()
            } else {
                "No code changes detected for review. Please make changes with the main agent first, or use `/review <path>` to specify a path."
                    .to_string()
            };
            let _ = result_tx.send(crate::core::types::review::ReviewOutcome {
                display_text: msg.clone(),
                verdict: crate::core::types::review::ReviewVerdict::Approved,
                report_summary: String::new(),
                report: None,
                auto_trigger: false,
            }).await;
            let _ = event_tx.send(ReviewEvent::Error {
                message: msg,
            });
            return;
        }

        let _ = event_tx.send(ReviewEvent::Started {
            file_count: changed_files.len(),
        });

        match orchestrator.review(changed_files, None).await {
            Ok(report) => {
                let display_text = orchestrator.format_review_report(&report);
                let verdict = report.summary.verdict.clone();
                let report_summary = format!(
                    "Verdict: {} | Score: {:.0}/100 | Issues: {}",
                    verdict.label(),
                    report.summary.overall_score,
                    report.summary.total_issues,
                );
                let _ = result_tx.send(crate::core::types::review::ReviewOutcome {
                    display_text,
                    verdict,
                    report_summary,
                    report: Some(report.clone()),
                    auto_trigger: false, // manual review: no auto-fix loop
                }).await;
                let _ = event_tx.send(ReviewEvent::Completed { report });
            }
            Err(e) => {
                let err_msg = format!("⚠️ Review failed: {}", e);
                let _ = result_tx.send(crate::core::types::review::ReviewOutcome {
                    display_text: err_msg.clone(),
                    verdict: crate::core::types::review::ReviewVerdict::NeedsRevision,
                    report_summary: String::new(),
                    report: None,
                    auto_trigger: false,
                }).await;
                let err_str: String = format!("{}", e);
                let _ = event_tx.send(ReviewEvent::Error {
                    message: err_str,
                });
            }
        }
    });
}

// Re-export for app
pub use crate::core::agent::review_agent::ReviewEvent;
