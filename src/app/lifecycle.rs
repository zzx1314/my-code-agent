//! Application main loop and shutdown/cleanup logic.

use std::io::Write;
use std::sync::Arc;
use anyhow::Result;
use ratatui::crossterm::event::{Event, poll, read};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::app;
use crate::app::App;
use crate::app::PendingConfirmation;
use crate::core::context::context_manager::ContextManager;
use crate::core::session::SessionData;
use crate::core::context::token_usage::TokenUsage;

// ═══════════════════════════════════════════════════════════════════════════════
// Cursor color palette — cycles through these hues for the native terminal
// cursor (OSC 12). Each pulse (~2 s) smoothly interpolates between adjacent
// colors via a ease-in-out sine wave.
// ═══════════════════════════════════════════════════════════════════════════════

const CURSOR_PULSE_MS: u128 = 2000;

const CURSOR_PALETTE: &[(u8, u8, u8)] = &[
    (255, 120, 0),     // warm orange
    (255, 60, 60),     // coral red
    (255, 180, 0),     // gold
    (0,   200, 255),   // cyan
    (0,   255, 120),   // spring green
    (200, 100, 255),   // purple
];

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f64 * (1.0 - t) + b.0 as f64 * t).round() as u8,
        (a.1 as f64 * (1.0 - t) + b.1 as f64 * t).round() as u8,
        (a.2 as f64 * (1.0 - t) + b.2 as f64 * t).round() as u8,
    )
}

// Sine-wave interpolation between adjacent palette entries — each "pulse"
// transitions smoothly from one color to the next, wrapping at the end.
fn cursor_color_at(elapsed_ms: u128) -> String {
    let cycle_num = elapsed_ms / CURSOR_PULSE_MS;
    let pos = elapsed_ms % CURSOR_PULSE_MS;
    let phase = pos as f64 / CURSOR_PULSE_MS as f64;

    let i = (cycle_num as usize) % CURSOR_PALETTE.len();
    let j = (i + 1) % CURSOR_PALETTE.len();

    let a = CURSOR_PALETTE[i];
    let b = CURSOR_PALETTE[j];

    let t = ((2.0 * std::f64::consts::PI * phase).sin() + 1.0) / 2.0;

    let (r, g, b) = lerp_rgb(a, b, t);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Run the main event loop until the user exits.
///
/// After the loop exits, performs all shutdown cleanup:
/// terminal teardown, undo-history cleanup, session auto-save.
pub async fn run_app(
    chat_history: Vec<crate::app::ChatEntry>,
    token_usage: TokenUsage,
    last_reasoning: String,
    config: crate::core::config::Config,
    agent: Arc<crate::core::agent::preamble::Agent>,
    orchestrator: Arc<crate::core::agent::orchestrator::AgentOrchestrator>,
    interrupt_tx: tokio::sync::broadcast::Sender<()>,
    confirmation_rx: Option<
        tokio::sync::mpsc::UnboundedReceiver<
            crate::tools::exec::confirmation::ConfirmationRequest,
        >,
    >,
    mut context_manager: ContextManager,
) -> Result<()> {
    let mut terminal = app::terminal::enter_terminal()?;

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
    app.orchestrator = Some(orchestrator);

    // ── Event loop ──────────────────────────────────────────────────────────
    loop {
        // Always advance for cursor blink; streaming spinners check is_streaming separately.
        app.marquee_frame = app.marquee_frame.wrapping_add(1);

        crate::core::agent::stream::process_streaming_events(&mut app);
        crate::core::agent::stream::check_stream_result(&mut app);
        crate::core::agent::stream::check_init_result(&mut app);
        crate::core::agent::stream::check_review_result(&mut app);

        // Decay review-complete status bar message
        if app.review_complete_timer > 0 {
            app.review_complete_timer -= 1;
            if app.review_complete_timer == 0 {
                app.review_complete_message = None;
            }
        }

        terminal.draw(|f| crate::ui::ui(f, &mut app))?;

        // ── Native cursor styling (after frame flush) ──────────────────────
        // Must happen AFTER terminal.draw() so ratatui's internal flush
        // (which may send its own cursor sequences via the crossterm backend)
        // doesn't overwrite our DECSCUSR / OSC 12 sequences.
        //
        // Cursor is always shown as a blinking bar (narrow shape) with a
        // dynamically cycling color.
        //
        // Using write! + flush() directly — BufWriter would buffer and never
        // deliver the sequences to the terminal.
        let cursor_color = if app.shell_mode {
            "#64c85a".to_string() // shell mode: static bright green
        } else {
            let elapsed_ms = app.cursor_anim_start.elapsed().as_millis();
            cursor_color_at(elapsed_ms)
        };
        let _ = std::io::Write::write_fmt(
            &mut std::io::stdout(),
            format_args!(
                "\x1b]12;{cursor_color}\x1b\\\x1b[?25h\x1b[5 q"
            ),
        );
        let _ = std::io::stdout().flush();

        crate::core::agent::stream::process_message_queue(&mut app, &mut context_manager);

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
    app::terminal::leave_terminal(terminal)?;

    // Clean up undo history for current session if configured
    if app.config.session.cleanup_undo_history {
        match crate::tools::infra::undo_history::clear_current_session_entries() {
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
