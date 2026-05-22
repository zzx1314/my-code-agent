use ratatui::crossterm::event::MouseEventKind;
use crate::app::App;

/// Handle mouse events — scroll wheel and click events.
///
/// Under basic mouse tracking (`?1000h`), the scroll wheel generates `ScrollUp` /
/// `ScrollDown` mouse events, and left-button clicks generate `Down(MouseButton::Left)`.
///
/// Click events on collapsible toggle lines (thinking blocks, git diffs, stdout/stderr,
/// file outlines) toggle the section's expanded/collapsed state.
pub fn handle_mouse_event(mouse: ratatui::crossterm::event::MouseEvent, app: &mut App) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            app.auto_scroll = false;
        }
        MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
            handle_click(mouse, app);
        }
        _ => {
            // Ignore other mouse events (drag, move, right/ middle clicks, etc.)
        }
    }
}

/// Handle left-click: check if the click position hits a collapsible toggle line,
/// and if so, toggle the section's collapsed/expanded state.
///
/// `mouse.row` is 1-based from crossterm; `app.chat_area_y` is 0-based from ratatui layout.
/// We compute the content line index as:
///   content_line = (mouse.row - 1) - chat_area_y + app.scroll
///
/// Then we search toggles for a match, using a dynamic tolerance that scales
/// with the section's content line count (larger sections accumulate more
/// word-wrap discrepancy and need a wider search radius).
fn handle_click(mouse: ratatui::crossterm::event::MouseEvent, app: &mut App) {
    if app.collapsed_toggles.is_empty() {
        return;
    }

    // Convert terminal row → content visual line index.
    // mouse.row is 1-based; area.y is 0-based.
    let rel_y = (mouse.row as i16).saturating_sub(1) - app.chat_area_y as i16;
    if rel_y < 0 {
        return; // Click is above the chat area
    }
    let content_line = rel_y as u16 + app.scroll;

    // Search for a matching toggle with dynamic tolerance.
    // Base tolerance of 2 visual lines, plus 1 per 40 content lines (rounded up).
    for &(toggle_line, ref section_id, content_count) in &app.collapsed_toggles {
        let tolerance = 2u16 + ((content_count as u16) + 39) / 40;
        let lower = toggle_line.saturating_sub(tolerance);
        let upper = toggle_line.saturating_add(tolerance);

        if content_line >= lower && content_line <= upper {
            // Toggle the section: remove returns true if present (expanding)
            // so the else branch inserts the section_id back (collapsing).
            if !app.collapsed_sections.remove(section_id) {
                app.collapsed_sections.insert(section_id.clone());
            }
            // Reset auto-scroll so the user stays where they clicked
            app.auto_scroll = false;
            return;
        }
    }
}
