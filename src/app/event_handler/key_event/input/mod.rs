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