pub mod markdown;
pub mod render;
pub mod terminal;

mod chat;
mod input;
mod overlays;
mod status;

use ratatui::prelude::*;

use crate::app::App;

use chat::render_chat_area;
use input::{apply_input_wrap, calculate_input_height, calculate_queue_height, render_input};
use overlays::{
    render_completion_menu, render_confirmation_dialog, render_model_picker,
    render_provider_picker, render_session_picker,
};
use status::render_status_bar;

/// Main UI rendering entry point.
///
/// Layout:
/// 1. Chat area (fills remaining space)
/// 2. Input area (dynamic height based on content)
/// 3. Status bar (1 line)
pub fn ui(f: &mut Frame, app: &mut App) {
    // Reset toggle tracking each frame — it gets rebuilt during rendering
    app.collapsed_toggles.clear();

    let area = f.area();
    let text_width = (area.width as usize).saturating_sub(2);

    apply_input_wrap(app, text_width);

    let input_height = calculate_input_height(app, area.width);
    let queue_height = calculate_queue_height(app);

    let mut constraints = vec![
        Constraint::Min(1),                                      // chat area
    ];
    if queue_height > 0 {
        constraints.push(Constraint::Length(queue_height));      // queue display
    }
    constraints.push(Constraint::Length(input_height));          // input area
    constraints.push(Constraint::Length(1));                     // status bar

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let chat_chunk_index = 0;
    let mut next_index = 1;
    let queue_chunk_index = if queue_height > 0 {
        let idx = next_index;
        next_index += 1;
        Some(idx)
    } else {
        None
    };
    let input_chunk_index = next_index;
    let status_chunk_index = next_index + 1;

    render_chat_area(f, app, chunks[chat_chunk_index]);

    if let Some(qi) = queue_chunk_index {
        input::render_queue_display(f, app, chunks[qi]);
    }

    render_input(f, app, chunks[input_chunk_index]);

    if app.show_completion {
        render_completion_menu(f, app, chunks[input_chunk_index]);
    }

    if app.show_model_picker {
        render_model_picker(f, app, chunks[input_chunk_index]);
    }

    if app.show_provider_picker {
        render_provider_picker(f, app, chunks[input_chunk_index]);
    }

    if app.show_session_picker {
        render_session_picker(f, app, chunks[input_chunk_index]);
    }

    if app.pending_confirmation.is_some() {
        render_confirmation_dialog(f, app);
    }

    render_status_bar(f, app, chunks[status_chunk_index]);
}
