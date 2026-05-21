use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

/// Minimum input area height (1 content line + 1 top pad + 1 bottom pad).
const MIN_INPUT_HEIGHT: u16 = 3;
/// Maximum input area height.
const MAX_INPUT_HEIGHT: u16 = 12;
const MAX_QUEUE_DISPLAY_LINES: usize = 4;


/// Update the input textarea's visual style based on the current app state.
///
/// Codex-style: no borders, just a subtle cursor-line highlight.
/// The cursor line style always includes the input background color so that
/// `cell.set_style()` in the textarea renderer preserves the background.
fn update_input_style(app: &mut App) {
    let bg = app.input_bg_color;

    let cursor_line_style = if app.is_streaming {
        Style::default().bg(bg)
    } else if app.shell_mode {
        Style::default().bg(Color::Rgb(40, 0, 60))
    } else {
        Style::default().bg(bg)
    };

    app.input.set_cursor_line_style(cursor_line_style);
    app.input.set_cursor_style(Style::default());
}

/// Wrapping is now handled natively by `TextArea`'s display logic.
/// This function is a no‑op — kept for compatibility.
pub fn apply_input_wrap(_app: &mut App, _text_width: usize) {
    // TextArea wraps internally during rendering; no manual wrap needed.
}

/// Calculate the dynamic height for the input area based on content and available width.
///
/// Returns a value clamped between `MIN_INPUT_HEIGHT` (3) and `MAX_INPUT_HEIGHT` (12).
/// An empty input returns the minimum height; wrapped multi-line content grows the area.
/// The input area has `top_pad=1` + `bot_pad=1`, so we add 2 to the wrapped line count
/// so that all content lines are visible without internal scrolling.
pub fn calculate_input_height(app: &App, area_width: u16) -> u16 {
    if app.input.is_empty() {
        return MIN_INPUT_HEIGHT;
    }
    let content_lines = app.input.desired_height(area_width);
    let height = content_lines + 2; // +2 for top_pad(1) + bot_pad(1)
    height.min(MAX_INPUT_HEIGHT).max(MIN_INPUT_HEIGHT)
}

/// Calculate the height needed for the queue display above the input.
/// Shows up to `MAX_QUEUE_DISPLAY_LINES` items, capped at a reasonable visual height.
pub fn calculate_queue_height(app: &App) -> u16 {
    if app.message_queue.is_empty() {
        return 0;
    }
    let display_count = app.message_queue.len().min(MAX_QUEUE_DISPLAY_LINES);
    // 2 lines for top/bottom border + separator + optional overflow indicator
    (display_count as u16) + 2
}

/// Spinner characters for the queue animation when streaming.
const QUEUE_SPINNER: &[char] = &['▰', '▱', '◉', '○'];

/// Color palette for the queue badge numbering — cycles through these per item.
const QUEUE_BADGE_COLORS: [Color; 4] = [
    Color::Rgb(255, 160, 50),   // warm orange
    Color::Rgb(255, 100, 100),  // coral red
    Color::Rgb(100, 200, 255),  // sky blue
    Color::Rgb(180, 130, 255),  // lavender
];

/// Render a single queue line with styled badge and message preview.
fn render_queue_line(index: usize, msg: &str) -> ratatui::text::Line<'static> {
    let badge_color = QUEUE_BADGE_COLORS[index % QUEUE_BADGE_COLORS.len()];

    // Truncate long messages for display
    let display_text = if msg.len() > 60 {
        let truncated: String = msg.chars().take(57).collect();
        format!("{}…", truncated)
    } else {
        msg.to_string()
    };

    ratatui::text::Line::from(vec![
        // Badge number
        ratatui::text::Span::styled(
            format!("  {:>2} ", index + 1),
            Style::default()
                .fg(badge_color)
                .bg(Color::Rgb(20, 15, 25))
                .add_modifier(Modifier::BOLD),
        ),
        // Separator dot
        ratatui::text::Span::styled(
            " ▶ ",
            Style::default().fg(Color::DarkGray),
        ),
        // Message text — bright white for emphasis
        ratatui::text::Span::styled(
            display_text,
            Style::default()
                .fg(Color::Rgb(240, 240, 255))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Render the queued messages above the input box.
pub fn render_queue_display(f: &mut Frame, app: &App, area: Rect) {
    if app.message_queue.is_empty() {
        return;
    }

    let queue_count = app.message_queue.len();
    let display_count = queue_count.min(MAX_QUEUE_DISPLAY_LINES);

    // Animated spinner character when streaming
    let spinner = if app.is_streaming {
        let frame = (app.marquee_frame as usize / 3) % QUEUE_SPINNER.len();
        QUEUE_SPINNER[frame]
    } else {
        '◉'
    };

    let title = format!(" {} Queued ({}) ", spinner, queue_count);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    for (i, msg) in app.message_queue.iter().take(display_count).enumerate() {
        lines.push(render_queue_line(i, msg));
    }

    if queue_count > MAX_QUEUE_DISPLAY_LINES {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            format!("  … and {} more in queue", queue_count - MAX_QUEUE_DISPLAY_LINES),
            Style::default()
                .fg(Color::Rgb(150, 150, 180))
                .add_modifier(Modifier::ITALIC),
        )));
    }

    // Vibrant gradient border: top/left in warm orange, bottom/right in purple
    let border_color = if app.is_streaming {
        Color::Rgb(255, 130, 40)  // bright orange when streaming
    } else {
        Color::Rgb(230, 100, 60)  // warm amber when idle with queue
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(255, 200, 80))
                .add_modifier(Modifier::BOLD),
        ))
        .border_type(ratatui::widgets::BorderType::Thick);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(Color::Rgb(25, 20, 30)));

    f.render_widget(paragraph, area);
}

/// Render the input textarea with a frosted‑glass background and position
/// the native terminal cursor.
pub fn render_input(f: &mut Frame, app: &mut App, area: Rect) {
    update_input_style(app);

    let bg = app.input_bg_color;
    let bg_paragraph = Paragraph::new("")
        .style(Style::default().bg(bg));
    f.render_widget(bg_paragraph, area);

    f.render_widget(&app.input, area);

    // Render the prompt prefix (›) on the first text line (Codex-style).
    // Rendered after the textarea so it's not overwritten by cursor-line fill.
    let prompt = Span::styled("›", Style::default().add_modifier(Modifier::BOLD));
    let prefix_area = Rect {
        x: area.x,
        y: area.y + 1, // top_pad = 1
        width: 1,
        height: 1,
    };
    f.render_widget(Paragraph::new(Line::from(prompt)), prefix_area);

    // Position the native terminal cursor.
    if let Some((x, y)) = app.input.cursor_pos(area) {
        let max_x = area.x + area.width.saturating_sub(1);
        let max_y = area.y + area.height.saturating_sub(1);
        if x <= max_x && y <= max_y {
            f.set_cursor_position((x, y));
        }
    }
}
