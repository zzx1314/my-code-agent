use crossterm::{
    event::{self, KeyCode, KeyModifiers, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Write as _;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::app::App;
use crate::app::conversion::convert_app_to_rig;
use crate::core::context::expand_file_refs;
use crate::core::context_manager::ContextManager;
use crate::core::streaming::{stream_response, StreamResult, StreamEvent};

/// Handle key events
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, context_manager: &mut ContextManager) {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C when streaming: handled by the broadcast interrupt
            if !app.is_streaming {
                app.should_exit = true;
            }
        }
        (KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+R: toggle reasoning display
            app.show_reasoning = !app.show_reasoning;
        }
        (KeyCode::Esc, _) => {
            if !app.is_streaming {
                app.should_exit = true;
            }
        }
        (KeyCode::Enter, modifiers) => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                // Shift+Enter: insert newline in textarea
                app.input.input(key);
            } else {
                // Plain Enter or Ctrl+Enter: send
                handle_enter_key(app, context_manager);
            }
        }
        (KeyCode::PageUp, _) => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        (KeyCode::PageDown, _) => {
            let max_scroll = app.total_lines.saturating_sub(1);
            app.scroll = (app.scroll + 3).min(max_scroll);
            app.auto_scroll = false;
        }
        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
            let max_scroll = app.total_lines.saturating_sub(1);
            app.scroll = (app.scroll + 3).min(max_scroll);
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {
            app.input.input(key);
        }
    }
}

/// Handle Enter key press (send message)
fn handle_enter_key(app: &mut App, context_manager: &mut ContextManager) {
    let input_text = app.input.lines().join("\n").trim().to_string();
    if !input_text.is_empty() && !app.is_streaming {
        app.show_banner = false; // Hide startup banner
        app.chat_history.push(("user".to_string(), input_text.clone()));
        app.input = {
            let mut ta = TextArea::default();
            ta.set_block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
            );
            ta.set_cursor_line_style(ratatui::style::Style::default());
            ta
        };
        app.is_streaming = true;
        app.auto_scroll = true;
        app.reasoning_auto_scroll = true;
        app.reasoning_scroll = 0;
        app.streaming_text.clear();
        app.streaming_reasoning.clear();
        app.current_response.clear();
        app.status_messages.clear();
        app.turn_usage_line = None;

        let expanded = expand_file_refs(&input_text, &app.config);
        
        let mut rig_chat_history = convert_app_to_rig(&app.chat_history);
        let agent_clone = app.agent.clone();
        let config_clone = app.config.clone();
        let mut token_usage_clone = app.token_usage.clone();
        let interrupt_rx = app.interrupt_tx.subscribe();
        let (response_tx, response_rx) = mpsc::channel::<StreamResult>(1);
        let (event_tx, event_rx) = mpsc::unbounded_channel::<StreamEvent>();

        let mut ctx_mgr = context_manager.clone();

        app.response_rx = Some(response_rx);
        app.streaming_events_rx = Some(event_rx);

        tokio::spawn(async move {
            let mut interrupt_rx = interrupt_rx;

            let result = stream_response(
                &agent_clone,
                &expanded.expanded,
                &mut rig_chat_history,
                &mut token_usage_clone,
                &mut interrupt_rx,
                &mut ctx_mgr,
                &config_clone.agent,
                Some(event_tx),
            ).await;

            response_tx.send(result).await.ok();
        });
    }
}

/// Handle mouse events
pub fn handle_mouse_event(mouse: event::MouseEvent, app: &mut App) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(1);
            app.scroll = (app.scroll + 3).min(max_scroll);
            // 滚到底部时重新启用 auto_scroll
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {} // 其他鼠标事件忽略，不影响文本选中
    }
}

/// Process streaming events
pub fn process_streaming_events(app: &mut App) {
    // Poll streaming text events for live display
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(StreamEvent::Text(delta)) => {
                    app.streaming_text.push_str(&delta);
                }
                Ok(StreamEvent::ToolCall(name)) => {
                    app.streaming_text.push_str(&format!("\n⟳ [`{}`]\n", name));
                }
                Ok(StreamEvent::ReasoningActive(active)) => {
                    if !active {
                        // 思考结束，保存内容到 last_reasoning
                        if !app.streaming_reasoning.is_empty() {
                            app.last_reasoning = app.streaming_reasoning.clone();
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(StreamEvent::ReasoningDelta(delta)) => {
                    app.streaming_reasoning.push_str(&delta);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    app.streaming_events_rx = None;
                    break;
                }
            }
        }
    }
}

/// Check for completed stream result
pub fn check_stream_result(app: &mut App) {
    if let Some(ref mut rx) = app.response_rx {
        if let Ok(result) = rx.try_recv() {
            process_stream_result(app, result);
            app.response_rx = None;
        }
    }
}

/// Process stream result
fn process_stream_result(app: &mut App, result: StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.streaming_events_rx = None;

    if !result.full_response.is_empty() {
        app.chat_history.push(("assistant".to_string(), result.full_response));
    }

    app.last_reasoning = result.last_reasoning;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
    }
    app.status_messages.clear();
}

/// Enter alternate screen and enable raw mode
pub fn enter_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Enable alternate scroll mode (DECSET 1007): mouse wheel sends arrow keys
    // without requiring mouse capture, so text selection still works.
    let _ = write!(std::io::stdout(), "\x1b[?1007h");
    let _ = std::io::stdout().flush();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
    let _ = write!(std::io::stdout(), "\x1b[?1007l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
