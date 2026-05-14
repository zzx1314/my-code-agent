use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_textarea::TextArea;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;

const MIN_INPUT_HEIGHT: u16 = 4;
const MAX_INPUT_HEIGHT: u16 = 14;

/// Update the input textarea's visual style based on the current app state.
///
/// - **Idle**: cyan borders, subtle cursor line highlight
/// - **Streaming**: dark gray (dimmed) borders, no cursor highlight
/// - **Shell mode**: magenta borders to indicate command mode
fn update_input_style(app: &mut App) {
    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let (border_color, title_text, cursor_style) = if app.is_streaming {
        let frame = spinner_frames[(app.marquee_frame as usize / 2) % spinner_frames.len()];
        (
            Color::DarkGray,
            format!(" {} Processing... ", frame),
            Style::default(), // dimmed
        )
    } else if app.shell_mode {
        (
            Color::Magenta,
            " ⚡ Shell Mode ".to_string(),
            Style::default().bg(Color::Rgb(40, 0, 60)),
        )
    } else {
        (
            Color::Cyan,
            " ✎  Input (Enter: send, Alt+Enter: ↵, Esc: interrupt) ".to_string(),
            Style::default().bg(Color::Rgb(20, 40, 50)),
        )
    };

    app.input.set_block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .border_type(ratatui::widgets::BorderType::Double),
    );
    app.input.set_cursor_line_style(cursor_style);
}

/// Wrap a single line of text to fit within `text_width`, tracking cursor position.
///
/// Returns a tuple of `(wrapped_lines, cursor_row, cursor_col)` where `cursor_row`
/// and `cursor_col` are the mapped position of the original cursor within the wrapped output.
/// Pass `cursor_char_idx = usize::MAX` to skip cursor tracking for non-target lines.
fn wrap_line(line: &str, text_width: usize, cursor_char_idx: usize) -> (Vec<String>, usize, usize) {
    let mut result: Vec<String> = Vec::new();
    let mut curr = String::new();
    let mut width: usize = 0;
    let mut char_idx: usize = 0;
    let mut out_row: usize = 0;
    let mut out_col: usize = 0;

    for (_, ch) in line.char_indices() {
        let cw = ch.width().unwrap_or(1);
        if width + cw > text_width && !curr.is_empty() {
            result.push(curr);
            curr = String::new();
            width = 0;
            char_idx = 0;
            out_row += 1;
        }
        curr.push(ch);
        width += cw;
        if char_idx == cursor_char_idx {
            out_row = result.len();
            out_col = curr.chars().count() - 1;
        }
        char_idx += 1;
    }
    if !curr.is_empty() {
        result.push(curr);
    }
    if cursor_char_idx == line.chars().count() {
        out_row = result.len() - 1;
        out_col = result.last().map(|s| s.chars().count()).unwrap_or(0);
    }
    (result, out_row, out_col)
}

/// Apply word-wrap to all lines in the input buffer to fit within `text_width`.
///
/// Rebuilds the `TextArea` with wrapped lines and adjusts the cursor position
/// to match the original location in the wrapped layout. Skips if no line exceeds
/// the text width.
pub fn apply_input_wrap(app: &mut App, text_width: usize) {
    if text_width == 0 {
        return;
    }
    let (cursor_row, cursor_char_idx) = app.input.cursor();
    let original_lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();

    let mut needs_wrap = false;
    for line in &original_lines {
        if line.width() > text_width {
            needs_wrap = true;
            break;
        }
    }
    if !needs_wrap {
        return;
    }

    let mut new_lines: Vec<String> = Vec::new();
    let mut row_offset: usize = 0;
    let mut new_cursor_row: usize = 0;
    let mut new_cursor_col: usize = 0;

    for (line_idx, line) in original_lines.iter().enumerate() {
        if line.is_empty() {
            new_lines.push(String::new());
            if line_idx == cursor_row {
                new_cursor_row = row_offset;
                new_cursor_col = 0;
            }
            row_offset += 1;
            continue;
        }

        let (wrapped, wr, wc) = wrap_line(
            line,
            text_width,
            if line_idx == cursor_row {
                cursor_char_idx
            } else {
                usize::MAX
            },
        );
        if line_idx == cursor_row && wr != usize::MAX {
            new_cursor_row = row_offset + wr;
            new_cursor_col = wc;
        }
        for wl in &wrapped {
            new_lines.push(wl.clone());
        }
        row_offset += wrapped.len();

        if line_idx == cursor_row && cursor_char_idx == line.chars().count() {
            new_cursor_row = row_offset - 1;
            new_cursor_col = wrapped.last().map(|s| s.chars().count()).unwrap_or(0);
        }
    }

    let mut new_ta = TextArea::from(new_lines.iter().map(|s| s.as_str()));
    new_ta.set_block(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
    );
    new_ta.set_cursor_line_style(ratatui::style::Style::default());
    new_ta.move_cursor(tui_textarea::CursorMove::Jump(
        new_cursor_row as u16,
        new_cursor_col as u16,
    ));
    app.input = new_ta;
}

/// Calculate the dynamic height for the input area based on content and available width.
///
/// Returns a value clamped between `MIN_INPUT_HEIGHT` (4) and `MAX_INPUT_HEIGHT` (14).
/// An empty input returns the minimum height; wrapped multi-line content grows the area.
pub fn calculate_input_height(app: &App, area_width: u16) -> u16 {
    let lines: Vec<&str> = app.input.lines().iter().map(|s| s.as_str()).collect();
    let is_empty = lines.is_empty() || (lines.len() == 1 && lines[0].is_empty());
    if is_empty {
        return MIN_INPUT_HEIGHT;
    }
    let text_width = area_width.saturating_sub(2);
    if text_width == 0 {
        return MIN_INPUT_HEIGHT;
    }
    let text = lines.join("\n");
    let visual_lines = Paragraph::new(text.as_str())
        .wrap(Wrap { trim: false })
        .line_count(text_width) as u16;
    let height = visual_lines + 2;
    height.min(MAX_INPUT_HEIGHT).max(MIN_INPUT_HEIGHT)
}

/// Render the input textarea into the given area.
pub fn render_input(f: &mut Frame, app: &mut App, area: Rect) {
    update_input_style(app);
    f.render_widget(&app.input, area);
}
