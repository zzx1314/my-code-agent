pub mod messages;
pub mod reasoning;
pub mod results;

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

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
        // Track whether we've added the initial separator from the chat
        // history. Once added, subsequent reasoning segments don't need
        // another blank line (the segment loop handles inter-segment spacing).
        let mut has_stream_separator = false;

        let area_width = width.unwrap_or(80) as u16;

        let boundaries: Vec<usize> = app.text_segment_boundaries.clone();
        let text: String = app.streaming_text.clone();
        let pre_text_segments: Vec<String> = app.completed_pre_text_segments.clone();
        let post_text_segments: Vec<String> = app.completed_post_text_segments.clone();
        let post_text: String = app.post_text_reasoning.clone();
        let last_reasoning: String = app.last_reasoning.clone();
        let active_reasoning: String = app.streaming_reasoning.clone();
        let streaming_todos: Option<String> = app.streaming_todos.clone();

        if !last_reasoning.is_empty() {
            // Separator before the first streaming reasoning block matches the
            // blank line that render_chat_with_reasoning adds before the last
            // assistant entry. If content already preceded us (has_stream_separator
            // is true), we still need a blank line to separate from it.
            if !has_stream_separator {
                lines.push(Line::default());
                has_stream_separator = true;
            } else {
                lines.push(Line::default());
            }
            render_reasoning_inline(
                &mut lines,
                &last_reasoning,
                app,
                "stream_last_reasoning",
                area_width,
            );
        }

        // ── Render archived pre-text reasoning segments ──
        // These are completed reasoning blocks that appeared before any text
        // content was emitted. Each segment gets its own "💭 Thinking..."
        // header so distinct thought blocks remain visually separated.
        for (i, segment) in pre_text_segments.iter().enumerate() {
            if !has_stream_separator {
                lines.push(Line::default());
                has_stream_separator = true;
            } else if i > 0 {
                lines.push(Line::default());
            }
            let section_id = format!("stream_pre_text_reasoning_{}", i);
            render_reasoning_inline(&mut lines, segment, app, &section_id, area_width);
        }

        let mut prev: usize = 0;

        for i in 0..post_text_segments.len() {
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
                &post_text_segments[i],
                app,
                &section_id,
                area_width,
            );
        }

        if !post_text.is_empty() {
            let b = boundaries
                .get(post_text_segments.len())
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
            lines.push(Line::default());
            let md_lines = render_streaming_markdown(&text[prev..], width);
            lines.extend(md_lines);
        }

        if !active_reasoning.is_empty() {
            lines.push(Line::default());
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
    // Build the paragraph first so we can query ratatui's exact line count,
    // which uses the same WordWrapper as rendering — eliminating mismatch.
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));

    let actual_lines = if area.width > 0 {
        paragraph.line_count(area.width) as u16
    } else {
        0
    };

    app.total_lines = actual_lines;
    app.chat_area_height = area.height;

    let max_scroll = actual_lines.saturating_sub(area.height);

    if app.auto_scroll {
        app.scroll = max_scroll;
    } else {
        app.scroll = app.scroll.min(max_scroll);
    }

    let paragraph = paragraph.scroll((app.scroll, 0));
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
            // Use display width (columns) instead of byte length.
            // CJK characters are 3 bytes but 2 columns; emoji can be 4+ bytes
            // but 2 columns. Using len() causes premature wrapping for non-ASCII text.
            if remaining.width() <= max_width {
                result.push(remaining.to_string());
                break;
            }
            // Find the byte position where display width exceeds max_width
            let mut byte_pos = 0;
            let mut w = 0;
            for ch in remaining.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if w + cw > max_width {
                    break;
                }
                w += cw;
                byte_pos += ch.len_utf8();
            }
            if byte_pos == 0 {
                // Single character wider than max_width — emit it anyway to avoid infinite loop
                let ch = remaining.chars().next().unwrap();
                byte_pos = ch.len_utf8();
            }
            // Try to break at the last space within the width limit
            let chunk = &remaining[..byte_pos];
            let mut break_at = chunk.rfind(' ').unwrap_or(byte_pos);
            if break_at == 0 {
                break_at = byte_pos;
            }
            result.push(remaining[..break_at].to_string());
            remaining = remaining[break_at..].trim_start();
        }
    }
    result
}
