pub mod messages;
pub mod reasoning;
pub mod results;

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::App;
use crate::ui::render::{render_full, render_streaming_markdown};

use messages::render_chat_messages;
use messages::render_chat_with_reasoning;
use messages::render_file_change_summary;
use messages::render_review_reasoning;
use messages::render_status_messages;
use reasoning::render_reasoning_inline;
use reasoning::render_streaming_reasoning_inline;

/// Threshold for collapsing content: sections with more lines than this are collapsed.
const COLLAPSE_THRESHOLD: usize = 8;

/// Render the chat history area including streaming content and reasoning.
pub fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    // Clear per-frame caches at the start of each render pass
    app.git_diff_cache.clear();
    if app.show_banner {
        messages::render_banner(f, app, area);
        return;
    }

    let width = Some(area.width as usize);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();

    let has_inline_reasoning =
        !app.is_streaming && app.show_inline_reasoning && !app.last_reasoning.is_empty();

    if has_inline_reasoning {
        render_chat_with_reasoning(&mut lines, app, width);
    } else {
        render_chat_messages(&mut lines, app, width);
    }

    if app.is_streaming {
        lines.push(Line::default());

        let area_width = width.unwrap_or(80) as u16;

        let boundaries: Vec<usize> = app.text_segment_boundaries.clone();
        let text: String = app.streaming_text.clone();
        let archived_segments: Vec<String> = app.completed_post_text_segments.clone();
        let post_text: String = app.post_text_reasoning.clone();
        let last_reasoning: String = app.last_reasoning.clone();
        let active_reasoning: String = app.streaming_reasoning.clone();
        let streaming_todos: Option<String> = app.streaming_todos.clone();

        if !last_reasoning.is_empty() {
            render_reasoning_inline(
                &mut lines,
                &last_reasoning,
                app,
                "stream_last_reasoning",
                area_width,
            );
        }

        let mut prev: usize = 0;

        for i in 0..archived_segments.len() {
            if i < boundaries.len() {
                let b = boundaries[i];
                if b > prev && b <= text.len() {
                    let md_lines = render_streaming_markdown(&text[prev..b], width);
                    lines.extend(md_lines);
                    prev = b;
                }
            }
            let section_id = format!("stream_post_text_reasoning_{}", i);
            lines.push(Line::default());
            render_reasoning_inline(
                &mut lines,
                &archived_segments[i],
                app,
                &section_id,
                area_width,
            );
        }

        if !post_text.is_empty() {
            let b = boundaries
                .get(archived_segments.len())
                .copied()
                .unwrap_or(text.len());
            if b > prev && b <= text.len() {
                let md_lines = render_streaming_markdown(&text[prev..b], width);
                lines.extend(md_lines);
                prev = b;
            }
            lines.push(Line::default());
            render_reasoning_inline(
                &mut lines,
                &post_text,
                app,
                "stream_post_text_reasoning",
                area_width,
            );
        }

        if prev < text.len() {
            let md_lines = render_streaming_markdown(&text[prev..], width);
            lines.extend(md_lines);
        }

        if !active_reasoning.is_empty() {
            let has_streaming_content =
                !last_reasoning.is_empty() || prev > 0 || !post_text.is_empty();
            if has_streaming_content {
                lines.push(Line::default());
            }
            render_streaming_reasoning_inline(
                &mut lines,
                &active_reasoning,
                app,
                "stream_reasoning",
                area_width,
            );
        }

        if let Some(ref todos_md) = streaming_todos {
            if !todos_md.is_empty() {
                let rendered = render_full(todos_md, width);
                if !rendered.is_empty() {
                    lines.push(Line::default());
                    lines.extend(rendered);
                    lines.push(Line::default());
                }
            }
        }
    }

    render_review_reasoning(&mut lines, app, width, 8);
    render_file_change_summary(&mut lines, app, width);
    render_status_messages(&mut lines, app, area);

    render_paragraph_with_scroll(f, app, lines, area);
}

fn render_paragraph_with_scroll(
    f: &mut Frame,
    app: &mut App,
    lines: Vec<ratatui::text::Line>,
    area: Rect,
) {
    let actual_lines = if area.width > 0 {
        lines
            .iter()
            .map(|l| {
                let w = l.width() as u16;
                if w == 0 {
                    1
                } else {
                    (w + area.width - 1) / area.width
                }
            })
            .sum::<u16>()
    } else {
        lines.len() as u16
    };

    app.total_lines = actual_lines;
    app.chat_area_height = area.height;

    let max_scroll = actual_lines.saturating_sub(area.height);

    if app.auto_scroll {
        app.scroll = max_scroll;
    } else {
        app.scroll = app.scroll.min(max_scroll);
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);
}

/// Word-wrap text to fit within `max_width` characters, splitting at word boundaries.
/// Preserves explicit newlines. Returns a flat list of wrapped lines.
pub fn word_wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for source_line in text.split('\n') {
        if source_line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut remaining = source_line;
        while !remaining.is_empty() {
            if remaining.len() <= max_width {
                result.push(remaining.to_string());
                break;
            }
            let mut break_at = remaining[..max_width].rfind(' ').unwrap_or(max_width);
            if break_at == 0 {
                break_at = max_width;
            }
            result.push(remaining[..break_at].to_string());
            remaining = remaining[break_at..].trim_start();
        }
    }
    result
}
