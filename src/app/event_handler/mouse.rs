use crate::app::App;

/// Handle mouse events
pub fn handle_mouse_event(mouse: ratatui::crossterm::event::MouseEvent, app: &mut App) {
    use ratatui::crossterm::event::MouseEventKind;

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            // Re-enable auto_scroll when scrolled to the bottom
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {} // Ignore other mouse events without affecting text selection
    }
}
