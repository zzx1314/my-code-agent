//! Application main loop and shutdown/cleanup logic.

use std::sync::Arc;

use anyhow::Result;
use ratatui::crossterm::event::{Event, poll, read};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::app;
use crate::app::App;
use crate::app::PendingConfirmation;
use crate::core::context_manager::ContextManager;
use crate::core::session::SessionData;
use crate::core::token_usage::TokenUsage;

/// Run the main event loop until the user exits.
///
/// After the loop exits, performs all shutdown cleanup:
/// terminal teardown, undo-history cleanup, session auto-save.
pub async fn run_app(
    chat_history: Vec<crate::app::ChatEntry>,
    token_usage: TokenUsage,
    last_reasoning: String,
    config: crate::core::config::Config,
    agent: Arc<crate::core::preamble::Agent>,
    interrupt_tx: tokio::sync::broadcast::Sender<()>,
    confirmation_rx: Option<
        tokio::sync::mpsc::UnboundedReceiver<
            crate::tools::confirmation::ConfirmationRequest,
        >,
    >,
    mut context_manager: ContextManager,
) -> Result<()> {
    // Enter alternate screen
    let mut terminal = app::event_handler::enter_terminal()?;

    // Build the App
    let mut app = App::new(
        chat_history,
        token_usage,
        last_reasoning,
        config,
        agent,
        interrupt_tx,
    );
    app.confirmation_rx = confirmation_rx;

    // ── Event loop ──────────────────────────────────────────────────────────
    loop {
        if app.is_streaming {
            app.marquee_frame = app.marquee_frame.wrapping_add(1);
        } else {
            app.marquee_frame = 0;
        }

        app::event_handler::process_streaming_events(&mut app);
        app::event_handler::check_stream_result(&mut app);
        app::event_handler::check_init_result(&mut app);

        terminal.draw(|f| crate::ui::ui(f, &mut app))?;

        app::event_handler::process_message_queue(&mut app, &mut context_manager);

        // Check for confirmation requests from tools
        if app.pending_confirmation.is_none() {
            if let Some(rx) = &mut app.confirmation_rx {
                if let Ok(req) = rx.try_recv() {
                    app.pending_confirmation = Some(PendingConfirmation {
                        reason: req.reason,
                        detail: req.detail,
                        response_tx: req.response_tx,
                    });
                }
            }
        }

        if poll(std::time::Duration::from_millis(100))? {
            match read()? {
                Event::Key(key) => {
                    app::event_handler::handle_key_event(key, &mut app, &mut context_manager);
                }
                Event::Mouse(mouse) => {
                    app::event_handler::handle_mouse_event(mouse, &mut app);
                }
                Event::Paste(text) => {
                    app::event_handler::handle_paste_event(&text, &mut app);
                }
                _ => {}
            }
        }

        if app.should_exit {
            break;
        }
    }

    // ── Shutdown ────────────────────────────────────────────────────────────
    shutdown(&mut app, &mut terminal)?;

    Ok(())
}

/// All post-loop cleanup: terminal teardown, undo-history cleanup, session save.
fn shutdown(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<()> {
    // Leave alternate screen
    app::event_handler::leave_terminal(terminal)?;

    // Clean up undo history for current session if configured
    if app.config.session.cleanup_undo_history {
        match crate::tools::undo_history::clear_current_session_entries() {
            Ok(cleared) if cleared > 0 => {
                tracing::info!(
                    cleared,
                    "Cleaned up undo history for current session on exit"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "Failed to clean up undo history on exit");
            }
        }
    }

    // Auto-save a timestamped session to .sessions/ (always, regardless of session.enabled)
    if !app.chat_history.is_empty() {
        save_timestamped_session(app);
    }

    // Save to default session file if session.enabled (for auto-resume)
    if app.config.session.enabled && !app.chat_history.is_empty() {
        save_default_session(app);
    }

    Ok(())
}

/// Convert app chat_history to Message vector for session persistence.
/// Preserves `reasoning_content` and tool metadata for DeepSeek reasoning
/// models.
fn to_session_messages(
    chat_history: &[crate::app::ChatEntry],
) -> Vec<crate::core::types::Message> {
    chat_history
        .iter()
        .map(|entry| crate::core::types::Message {
            role: entry.role.clone(),
            content: entry.content.clone(),
            reasoning_content: entry.reasoning_content.clone(),
            tool_calls: entry.tool_calls.clone(),
            tool_call_id: entry.tool_call_id.clone(),
        })
        .collect()
}

/// Save a timestamped session to .sessions/ and prune old ones.
fn save_timestamped_session(app: &App) {
    let history = to_session_messages(&app.chat_history);
    let name = crate::core::session::generate_session_name();
    let data = SessionData::with_name(
        history,
        app.token_usage.clone(),
        app.last_reasoning.clone(),
        name.clone(),
    );
    match data.save_with_name(&name) {
        Ok(()) => tracing::info!(name = %name, "Auto-saved session on exit"),
        Err(e) => tracing::error!(error = %e, "Failed to auto-save session on exit"),
    }

    match SessionData::prune_old_sessions(5) {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "Pruned old session files"),
        Err(e) => tracing::warn!(error = %e, "Failed to prune old sessions"),
    }
}

/// Save to the default session file for auto-resume next time.
fn save_default_session(app: &App) {
    let history = to_session_messages(&app.chat_history);
    let data = SessionData::new(
        history,
        app.token_usage.clone(),
        app.last_reasoning.clone(),
    );
    if let Err(e) = data.save_default(app.config.session.save_file.as_deref()) {
        tracing::error!(error = %e, "Failed to save session");
    }
}
