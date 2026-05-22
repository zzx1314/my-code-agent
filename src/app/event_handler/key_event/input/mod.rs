mod enter;
pub mod shell;

pub use enter::handle_enter_key;

use crate::app::App;
use crate::ui::textarea::TextArea;


/// Reset the input textarea to default state (Codex-style: no block/borders).
pub fn reset_input(app: &mut App) {
    let mut ta = TextArea::default();
    ta.set_cursor_line_style(ratatui::style::Style::default());
    app.input = ta;
    app.show_banner = false;
    app.auto_scroll = true;
}

/// Set the input textarea content to a given string (Codex-style: no block/borders).
fn set_input_text(app: &mut App, text: &str) {
    let mut ta = TextArea::from(text);
    ta.set_cursor_line_style(ratatui::style::Style::default());
    app.input = ta;
}

/// Navigate up in input history (toward older entries).
/// Returns `true` if navigation occurred, `false` if no navigation was possible
/// (no history, or already at the oldest entry).
pub fn history_up(app: &mut App) -> bool {
    if app.input_history.is_empty() {
        return false;
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
                return false; // Already at the oldest entry
            }
            idx - 1
        }
    };

    app.history_index = Some(current_index);
    let text = app.input_history[current_index].clone();
    set_input_text(app, &text);
    true
}

/// Navigate down in input history (toward newer entries).
/// Returns `true` if navigation occurred, `false` if no navigation was possible
/// (not browsing history, or past the newest entry which restores the draft).
pub fn history_down(app: &mut App) -> bool {
    match app.history_index {
        None => return false, // Not browsing history
        Some(idx) => {
            if idx + 1 >= app.input_history.len() {
                // Past the end: restore the draft and exit browsing
                app.history_index = None;
                let draft = app.history_draft.clone();
                set_input_text(app, &draft);
                return false;
            } else {
                let new_idx = idx + 1;
                app.history_index = Some(new_idx);
                let text = app.input_history[new_idx].clone();
                set_input_text(app, &text);
                true
            }
        }
    }
}
