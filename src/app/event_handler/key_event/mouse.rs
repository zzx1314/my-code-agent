use crate::app::App;

/// Handle mouse events
pub fn handle_mouse_event(mouse: ratatui::crossterm::event::MouseEvent, app: &mut App) {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Left-click on a collapsible toggle: compute virtual line from
            // screen y + scroll offset, then find the matching toggle.
            // The y=mouse.row is the row within the terminal (0-indexed).
            // We need to account for the chat area's position in the layout.
            // For simplicity, we approximate: virtual_line ≈ mouse.row + scroll.
            let clicked_virtual = (mouse.row as u16).saturating_add(app.scroll);

            // Find the toggle whose position is closest to the click.
            // Use a dynamic tolerance based on the section's content line count:
            // larger sections accumulate more word-wrap discrepancy between our
            // simplified `visual_lines` calculation and Ratatui's actual wrapping.
            // Formula: min(10, max(3, content_lines / 5))
            let mut found: Option<String> = None;
            for &(toggle_line, ref section_id, content_lines) in &app.collapsed_toggles {
                // Dynamic tolerance: proportional to section size
                let tol: u16 = (content_lines as u16 / 5).clamp(3, 10);
                let diff = if toggle_line >= clicked_virtual {
                    toggle_line - clicked_virtual
                } else {
                    clicked_virtual - toggle_line
                };
                if diff <= tol {
                    found = Some(section_id.clone());
                    break;
                }
            }

            if let Some(section_id) = found {
                if app.collapsed_sections.contains(&section_id) {
                    app.collapsed_sections.remove(&section_id);
                } else {
                    app.collapsed_sections.insert(section_id);
                }
            }
        }
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
