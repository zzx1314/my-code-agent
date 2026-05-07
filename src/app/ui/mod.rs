mod chat;
mod input;
mod overlays;
mod status;

use ratatui::prelude::*;

use crate::app::App;

use chat::render_chat_area;
use input::{apply_input_wrap, calculate_input_height, render_input};
use overlays::{
    render_completion_menu, render_confirmation_dialog, render_model_picker, render_provider_picker,
    render_session_picker,
};
use status::render_status_bar;

/// Main UI rendering entry point.
///
/// Layout:
/// 1. Chat area (fills remaining space)
/// 2. Input area (dynamic height based on content)
/// 3. Status bar (1 line)
pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let text_width = (area.width as usize).saturating_sub(2);

    apply_input_wrap(app, text_width);

    let input_height = calculate_input_height(app, area.width);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(area);

    let chat_chunk_index = 0;
    let input_chunk_index = 1;
    let status_chunk_index = 2;

    render_chat_area(f, app, chunks[chat_chunk_index]);

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
