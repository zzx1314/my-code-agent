mod enter;
pub mod shell;

pub use enter::handle_enter_key;

use tui_textarea::TextArea;

use crate::app::App;

/// Reset the input textarea to default state
pub fn reset_input(app: &mut App) {
    let title = if app.shell_mode {
        " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
    } else {
        " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
    };
    app.input = {
        let mut ta = TextArea::default();
        ta.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(title),
        );
        ta.set_cursor_line_style(ratatui::style::Style::default());
        ta
    };
    app.show_banner = false;
    app.auto_scroll = true;
}

/// Set the input textarea content to a given string, preserving the block style.
fn set_input_text(app: &mut App, text: &str) {
    let title = if app.shell_mode {
        " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
    } else {
        " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
    };
    let mut ta = TextArea::default();
    if !text.is_empty() {
        ta.insert_str(text);
    }
    ta.set_block(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(title),
    );
    ta.set_cursor_line_style(ratatui::style::Style::default());
    app.input = ta;
}

/// Navigate up in input history (toward older entries).
pub fn history_up(app: &mut App) {
    if app.input_history.is_empty() {
        return;
    }

    let current_index = match app.history_index {
        None => {
            // Save the current draft before navigating
            app.history_draft = app.input.lines().join("\n").trim().to_string();
            // Start from the newest entry
            app.input_history.len() - 1
        }
        Some(idx) => {
            if idx == 0 {
                return; // Already at the oldest entry
            }
            idx - 1
        }
    };

    app.history_index = Some(current_index);
    let text = app.input_history[current_index].clone();
    set_input_text(app, &text);
}

/// Navigate down in input history (toward newer entries).
pub fn history_down(app: &mut App) {
    match app.history_index {
        None => return, // Not browsing history
        Some(idx) => {
            if idx + 1 >= app.input_history.len() {
                // Past the end: restore the draft
                app.history_index = None;
                let draft = app.history_draft.clone();
                set_input_text(app, &draft);
            } else {
                let new_idx = idx + 1;
                app.history_index = Some(new_idx);
                let text = app.input_history[new_idx].clone();
                set_input_text(app, &text);
            }
        }
    }
}
