use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, ChatEntry};
use crate::ui::render::{render_full, render_streaming_markdown};
use unicode_width::UnicodeWidthStr;

/// Threshold for collapsing content: sections with more lines than this are collapsed.
const COLLAPSE_THRESHOLD: usize = 8;

/// Render the chat history area including streaming content and reasoning.
///
/// Codex-style: reasoning is shown inline with dim/italic style and `• ` prefix,
/// NOT as a fixed bottom block. During streaming, the reasoning content is shown
/// inline in the scrollable chat area. The status bar shows the extracted bold
/// header from reasoning for a compact status display.
pub fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    // Clear per-frame caches at the start of each render pass
    app.git_diff_cache.clear();
    if app.show_banner {
        render_banner(f, app, area);
        return;
    }

    let width = Some(area.width as usize);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();

    let has_inline_reasoning = !app.is_streaming && app.show_inline_reasoning && !app.last_reasoning.is_empty();

    if has_inline_reasoning {
        // Render history with reasoning placed before the last assistant message (Codex-style inline)
        render_chat_with_reasoning(&mut lines, app, width);
    } else {
        render_chat_messages(&mut lines, app, width);
    }

    // During streaming, render content in chronological order:
    // pre-text thinking → text → post-text thinking → text → ...
    // This interleaves thinking segments with their associated text chunks
    // using `text_segment_boundaries` recorded when each post-text thinking
    // segment starts.
    //
    // All app data is cloned BEFORE rendering to avoid borrow conflicts:
    // render_reasoning_inline and render_streaming_reasoning_inline take
    // &mut app (for collapse toggles), while boundaries/text/segments are
    // immutable references that would conflict.
    if app.is_streaming {
        let area_width = width.unwrap_or(80) as u16;

        // Clone all streaming data from app first
        let boundaries: Vec<usize> = app.text_segment_boundaries.clone();
        let text: String = app.streaming_text.clone();
        let archived_segments: Vec<String> = app.completed_post_text_segments.clone();
        let post_text: String = app.post_text_reasoning.clone();
        let last_reasoning: String = app.last_reasoning.clone();
        let active_reasoning: String = app.streaming_reasoning.clone();
        let streaming_todos: Option<String> = app.streaming_todos.clone();

        // 1. Pre-text completed reasoning (from before any text appeared)
        if !last_reasoning.is_empty() {
            render_reasoning_inline(&mut lines, &last_reasoning, app, "stream_last_reasoning", area_width);
        }

        // 2. Interleave text chunks with post-text thinking segments using
        //    the recorded text segment boundaries.
        let mut prev: usize = 0;

        // Archived segments map to boundaries[0..n-1]
        for i in 0..archived_segments.len() {
            if i < boundaries.len() {
                let b = boundaries[i];
                if b > prev && b <= text.len() {
                    let md_lines = render_streaming_markdown(&text[prev..b], width);
                    lines.extend(md_lines);
                    prev = b;
                }
            }
            // Render the archived thinking segment that follows this text chunk
            let section_id = format!("stream_post_text_reasoning_{}", i);
            render_reasoning_inline(&mut lines, &archived_segments[i], app, &section_id, area_width);
        }

        // Current post-text reasoning (follows the next text chunk)
        if !post_text.is_empty() {
            // The boundary for the current post-text segment is at
            // boundaries[archived_segments.len()] (if it exists).
            let b = boundaries.get(archived_segments.len()).copied().unwrap_or(text.len());
            if b > prev && b <= text.len() {
                let md_lines = render_streaming_markdown(&text[prev..b], width);
                lines.extend(md_lines);
                prev = b;
            }
            render_reasoning_inline(&mut lines, &post_text, app, "stream_post_text_reasoning", area_width);
        }

        // Remaining streaming text (not yet associated with any thinking segment)
        if prev < text.len() {
            let md_lines = render_streaming_markdown(&text[prev..], width);
            lines.extend(md_lines);
        }

        // Active reasoning (currently streaming segment)
        if !active_reasoning.is_empty() {
            render_streaming_reasoning_inline(&mut lines, &active_reasoning, app, "stream_reasoning", area_width);
        }

        // Streaming todos — rendered below text/reasoning when present
        if let Some(ref todos_md) = streaming_todos {
            if !todos_md.is_empty() {
                let rendered = render_full(todos_md, width);
                if !rendered.is_empty() {
                    lines.push(Line::default());
                    lines.extend(rendered);
                }
            }
        }

    }

    // Render review reasoning (transient — not added to chat history)
    render_review_reasoning(&mut lines, app, width, 8);

    // Render aggregated file change summary (when not streaming)
    render_file_change_summary(&mut lines, app, width);

    // Render status messages at the bottom
    render_status_messages(&mut lines, app, area);

    render_paragraph_with_scroll(f, app, lines, area);
}

fn render_paragraph_with_scroll(f: &mut Frame, app: &mut App, lines: Vec<ratatui::text::Line>, area: Rect) {
    // Estimate visual lines after word-wrap without cloning the entire lines vec.
    // This avoids the expensive allocation of cloning all styled spans.
    let actual_lines = if area.width > 0 {
        lines.iter().map(|l| {
            let w = l.width() as u16;
            if w == 0 { 1 } else { (w + area.width - 1) / area.width }
        }).sum::<u16>()
    } else {
        lines.len() as u16
    };

    app.total_lines = actual_lines;
    app.chat_area_height = area.height;

    let max_scroll = actual_lines.saturating_sub(area.height);

    if app.auto_scroll {
        // When auto-scrolling, always stay at the bottom of content.
        // This prevents visual jumping when total_lines fluctuates (e.g.
        // due to word-wrap reflow of collapsed reasoning sections).
        app.scroll = max_scroll;
    } else {
        // Manual scrolling: clamp to valid range so scroll doesn't go
        // past the end of content after layout transitions (streaming→done,
        // collapse toggle).
        app.scroll = app.scroll.min(max_scroll);
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);
}

/// Render the startup banner — bordered info panel wrapping tightly around content.
fn render_banner(f: &mut Frame, app: &mut App, area: Rect) {
    let model = app
        .config
        .llm
        .model
        .as_deref()
        .unwrap_or("unknown");
    let dir = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();
    let title_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::LightYellow).add_modifier(Modifier::DIM);
    let value_style = Style::default().fg(Color::Cyan);

    let lines = vec![
        Line::from(vec![
            Span::styled(" >_ My Code Agent", title_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" model:     ", dim),
            Span::styled(model, value_style),
            Span::styled("   ", dim),
            Span::styled("/model to change", dim),
        ]),
        Line::from(vec![
            Span::styled(" directory: ", dim),
            Span::styled(dir, value_style),
        ]),
    ];

    // Border fits content width + 2 for left/right borders + 2 for inner padding.
    let content_width = lines.iter().map(|l| l.width()).max().unwrap_or(0);
    let box_width = (content_width + 4).min(area.width as usize);

    // Inner text width available after borders (2) + padding (2).
    let text_width = box_width.saturating_sub(4).max(1);

    // Calculate total height accounting for line wrapping when the terminal
    // is narrower than the content (e.g. long directory paths).
    let total_text_rows: u16 = lines
        .iter()
        .map(|l| {
            let w = l.width();
            if w == 0 {
                1u16
            } else {
                ((w as u16) + (text_width as u16) - 1) / (text_width as u16).max(1)
            }
        })
        .sum();

    // Height = wrapped content lines + 2 border rows (top + bottom)
    let box_height = (total_text_rows + 2).min(area.height);

    let box_area = Rect {
        x: area.x,
        y: area.y,
        width: box_width as u16,
        height: box_height,
    };

    app.total_lines = box_area.height;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    f.render_widget(paragraph, box_area);
}


/// Render chat with reasoning placed before the last assistant message.
/// Uses the new inline reasoning style (Codex-inspired: `• ` prefix, dim/italic).
fn render_chat_with_reasoning(lines: &mut Vec<ratatui::text::Line<'static>>, app: &mut App, max_width: Option<usize>) {
    let last_assistant_idx = app
        .chat_history
        .iter()
        .rposition(|entry| entry.role == "assistant");
    let split_idx = last_assistant_idx.unwrap_or(app.chat_history.len());

    let show_tool_calls_in_history = app.config.agent.show_tool_calls_in_history;

    // Clone entries before the last assistant to avoid borrow conflict with &mut App
    let before: Vec<(usize, ChatEntry)> = app.chat_history[..split_idx].iter().enumerate()
        .map(|(i, e)| (i, e.clone()))
        .collect();
    for (i, entry) in &before {
        render_message(lines, entry, *i, app, max_width, show_tool_calls_in_history, app.config.agent.show_tool_details);
    }

    // Render the last assistant message inline — its reasoning_content will be
    // rendered by render_message via the new inline reasoning helper.
    if let Some(idx) = last_assistant_idx {
        let entry = app.chat_history[idx].clone();
        render_message(lines, &entry, idx, app, max_width, show_tool_calls_in_history, app.config.agent.show_tool_details);
    }
}

/// Render all chat messages in order.
fn render_chat_messages(lines: &mut Vec<ratatui::text::Line<'static>>, app: &mut App, max_width: Option<usize>) {
    let show_tool_calls_in_history = app.config.agent.show_tool_calls_in_history;
    // Clone entries to avoid borrow conflict with &mut App
    let entries: Vec<(usize, ChatEntry)> = app.chat_history.iter().enumerate()
        .map(|(i, e)| (i, e.clone()))
        .collect();
    for (i, entry) in &entries {
        render_message(lines, entry, *i, app, max_width, show_tool_calls_in_history, app.config.agent.show_tool_details);
    }
}

/// Render a collapsible block of `Line`s. If the number of content lines exceeds
/// `COLLAPSE_THRESHOLD` and the section is in the collapsed set, only the first
/// `COLLAPSE_THRESHOLD` lines are shown with a toggle to expand. If expanded, all
/// lines are shown with a toggle to collapse.
///
/// `area_width` is the terminal width used to compute visual line positions (after
/// word-wrap). Toggle positions are stored as visual line indices so that mouse clicks
/// (which also operate in visual line space) hit the correct toggle even when lines wrap.
fn render_collapsible_block<'a>(
    lines: &mut Vec<ratatui::text::Line<'a>>,
    app: &mut App,
    section_id: &str,
    content: Vec<ratatui::text::Line<'a>>,
    area_width: u16,
) {
    let total = content.len();
    let collapsed = !app.collapsed_sections.contains(section_id);

    /// Compute how many visual lines a `Line` occupies after word-wrap at `width`.
    fn visual_lines(line: &ratatui::text::Line<'_>, width: u16) -> u16 {
        let line_width = line.width() as u16;
        if line_width == 0 || width == 0 {
            1 // empty lines still occupy one row
        } else {
            (line_width + width - 1) / width
        }
    }

    // The current visual line position (after word-wrap) in the `lines` buffer.
    let mut vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

    if total > COLLAPSE_THRESHOLD {
        if collapsed {
            // Show first COLLAPSE_THRESHOLD lines
            for line in content.into_iter().take(COLLAPSE_THRESHOLD) {
                vis_pos += visual_lines(&line, area_width);
                lines.push(line);
            }
            // vis_pos is now the visual line index of the toggle text.
            // Store content line count so the mouse handler can use a
            // dynamic tolerance — word-wrap discrepancies compound with
            // more lines, so larger sections need a wider search radius.
            app.collapsed_toggles.push((vis_pos, section_id.to_string(), total));
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("  [+ {} more lines - click to expand]", total - COLLAPSE_THRESHOLD),
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
        } else {
            // Show all lines
            for line in content {
                vis_pos += visual_lines(&line, area_width);
                lines.push(line);
            }
            // vis_pos is now the visual line index of the toggle text.
            // Store content line count for dynamic tolerance calculation.
            app.collapsed_toggles.push((vis_pos, section_id.to_string(), total));
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    "  [-] click to collapse",
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
        }
    } else {
        // Small enough, show all without toggle
        for line in content {
            lines.push(line);
        }
    }
}

/// Render a single message with role-based styling.
fn render_message(lines: &mut Vec<ratatui::text::Line<'static>>, entry: &ChatEntry, entry_idx: usize, app: &mut App, max_width: Option<usize>, show_tool_calls: bool, show_tool_details: bool) {
    let area_width = max_width.unwrap_or(80) as u16;
    match entry.role.as_str() {            "user" => {
            // User message display with full-row background:
            // - Full-width background color across the entire terminal
            // - Top margin spacer (1 line) with background
            // - "› " prefix (bold, dim) on first line
            // - "  " continuation indent on wrapped/subsequent lines
            // - Each content line is padded with spaces to fill the full terminal width
            // - Bottom margin spacer (1 line) with background
            let user_bg = app.user_message_bg;
            let body_style = Style::default()
                .fg(Color::Rgb(220, 220, 240))
                .bg(user_bg);
            let prefix_style = Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::DIM)
                .bg(user_bg);
            let full_bg = Style::default().bg(user_bg);
            let area_w = area_width as usize;
            // Top margin spacer — full-width with background
            lines.push(Line::from(vec![Span::styled(" ".repeat(area_w), full_bg)]));
            if !entry.content.is_empty() {
                let wrap_width = area_width.saturating_sub(3).max(8) as usize;
                let message = entry.content.trim_end_matches(['\r', '\n']);
                let wrapped = word_wrap_text(message, wrap_width);
                for (i, line_text) in wrapped.iter().enumerate() {
                    let prefix = if i == 0 { "› " } else { "  " };
                    let padding = area_w.saturating_sub(prefix.width() + line_text.width());
                    let padded_body = format!("{}{}", line_text, " ".repeat(padding));
                    lines.push(Line::from(vec![
                        Span::styled(prefix.to_string(), prefix_style),
                        Span::styled(padded_body, body_style),
                    ]));
                }
            }
            // Bottom margin spacer — full-width with background
            lines.push(Line::from(vec![Span::styled(" ".repeat(area_w), full_bg)]));
        }
        "assistant" => {
            // Display tool calls (e.g. shell_exec) if present and config allows
            if show_tool_calls {
                if let Some(ref tool_calls) = entry.tool_calls {
                    for tc in tool_calls {
                        let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(serde_json::Value::Null);
                        lines.push(Line::from(vec![
                            Span::styled(
                                "⚙️ ",
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::styled(
                                tc.function.name.clone(),
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                            if show_tool_details {
                                if let Some(cmd) = args.get("command").and_then(|c| c.as_str()) {
                                    lines.push(Line::from(format!("  {}", cmd)));
                                } else {
                                    lines.push(Line::from(format!("  {}", args)));
                                }
                            }
                    }
                    if !entry.content.is_empty() {
                        lines.push(Line::default());
                    }
                }
            }
            // Display reasoning inline (Codex style) if present
            if let Some(ref reasoning) = entry.reasoning_content {
                let section_id = format!("reason_{}", entry_idx);
                render_reasoning_inline(lines, reasoning, app, &section_id, area_width);
            }
            // Display normal content (cached to avoid re-parsing markdown every frame)
            // Cache key includes max_width to handle terminal resizing correctly.
            if !entry.content.is_empty() {
                let cache_key = format!("{}|{}", entry.content, max_width.map_or(0, |w| w as isize));
                let md = if let Some(cached) = app.rendered_cache.get(&cache_key) {
                    cached.clone()
                } else {
                    let rendered = render_full(&entry.content, max_width);
                    app.rendered_cache.insert(cache_key, rendered.clone());
                    rendered
                };
                lines.extend(md);
            }
            if (show_tool_calls && entry.tool_calls.is_some()) || !entry.content.is_empty() {
                lines.push(Line::default());
            }
        }
"tool" => {
            // File tool results (file_write, file_update, file_delete) with git_diff
            // are ALWAYS shown — they contain substantive code changes.
            if try_render_file_tool_result(lines, &entry.content, entry_idx, app, true, area_width).is_some() {
                lines.push(Line::default());
                return;
            }

            // Todos results are ALWAYS shown — they contain planning progress.
            if try_render_todos(lines, &entry.content, max_width).is_some() {
                lines.push(Line::default());
                return;
            }

            // Other tool results are only shown when show_tool_calls is enabled
            if show_tool_calls && show_tool_details {
                // Parse the tool result (ShellExecOutput JSON) for nice display
                if let Ok(output) = serde_json::from_str::<serde_json::Value>(&entry.content) {
                    if let Some(cmd) = output.get("command").and_then(|c| c.as_str()) {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "⚙️ ",
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::styled(
                                "Shell Exec",
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                        lines.push(Line::from(format!("  Command: {}", cmd)));
                        if let Some(exit_code) = output.get("exit_code") {
                            let color = if exit_code.as_i64() == Some(0) {
                                Color::Green
                            } else {
                                Color::Red
                            };
                            lines.push(Line::from(vec![
                                Span::styled("  Exit Code: ", Style::default()),
                                Span::styled(format!("{}", exit_code), Style::default().fg(color)),
                            ]));
                        }
                        if let Some(timed_out) = output.get("timed_out").and_then(|t| t.as_bool()) {
                            if timed_out {
                                lines.push(Line::from(Span::styled(
                                    "  ⚠ Timed out",
                                    Style::default().fg(Color::Red),
                                )));
                            }
                        }
                        if let Some(stdout) = output.get("stdout").and_then(|s| s.as_str()) {
                            if !stdout.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    "  ─── stdout ───",
                                    Style::default().fg(Color::DarkGray),
                                )));
                                // Collapsible stdout
                                let stdout_lines: Vec<Line> = stdout.lines()
                                    .map(|l| Line::from(format!("  {}", l)))
                                    .collect();
                                let section_id = format!("so_{}", entry_idx);
                                render_collapsible_block(lines, app, &section_id, stdout_lines, area_width);
                            }
                        }
                        if let Some(stderr) = output.get("stderr").and_then(|s| s.as_str()) {
                            if !stderr.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    "  ─── stderr ───",
                                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                                )));
                                // Collapsible stderr
                                let stderr_lines: Vec<Line> = stderr.lines()
                                    .map(|l| Line::from(format!("  {}", l)))
                                    .collect();
                                let section_id = format!("se_{}", entry_idx);
                                render_collapsible_block(lines, app, &section_id, stderr_lines, area_width);
                            }
                        }
                        lines.push(Line::default());
                        return;
                    }
                }
                // Check if it's a file_outline result
                if try_render_file_outline(lines, &entry.content, entry_idx, app, area_width).is_some() {
                    lines.push(Line::default());
                    return;
                }


                // Fallback: show raw content for non-shell tool results
                if !entry.content.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "🔧 Tool Result:",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    lines.push(Line::from(entry.content.to_string()));
                    lines.push(Line::default());
                }
            }
        }
        _ => {
            lines.push(Line::from(format!("{}: {}", entry.role, entry.content)));
            lines.push(Line::default());
        }
    }
}


/// Render review reasoning block — transient thinking content shown during code review.
/// Uses a blockquote style with `│ ` prefix and a fixed height so the content below
/// doesn't jump as reasoning streams in. Each line is pre-word-wrapped to
/// `area_width - 2` (for the `"│ "` prefix) to prevent Paragraph re-wrapping and
/// the resulting visual height fluctuations that cause flickering.
fn render_review_reasoning(lines: &mut Vec<ratatui::text::Line<'static>>, app: &App, max_width: Option<usize>, max_height: u16) {
    if !app.is_reviewing {
        return;
    }

    let area_width = max_width.unwrap_or(80) as usize;
    // Wrap width = terminal width - 2 for the "│ " prefix, so each final line
    // fits in exactly 1 visual line without Paragraph re-wrapping.
    let wrap_width = area_width.saturating_sub(2).max(8);

    if app.review_reasoning.is_empty() {
        // Pad with empty lines to maintain fixed height even when no content
        if max_height > 0 {
            let header_reserve: u16 = 2; // header + trailing empty
            let content_budget = max_height.saturating_sub(header_reserve).max(1);
            lines.push(Line::from(Span::styled(
                "💭 Review Analysis:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            for _ in 0..content_budget {
                lines.push(Line::from(Span::styled(
                    "│",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::default());
        }
        return;
    }

    // Reserve lines for: header ("💭 Review Analysis:") and trailing empty line
    let header_reserve: u16 = 2; // header + trailing empty
    let content_budget = max_height.saturating_sub(header_reserve).max(1);

    lines.push(Line::from(Span::styled(
        "💭 Review Analysis:",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));

    // Pre-word-wrap each reasoning line to wrap_width so that after adding
    // the "│ " prefix, each content line is exactly 1 visual line (no re-wrap).
    let reasoning_wrapped: Vec<String> = app.review_reasoning.lines()
        .flat_map(|line| {
            if line.is_empty() {
                vec![String::new()]
            } else {
                word_wrap_text(line, wrap_width)
            }
        })
        .collect();

    let total = reasoning_wrapped.len();
    let max_display = content_budget as usize;

    if total > max_display {
        let skipped = total - max_display;
        // "hidden" message line also counts toward the budget
        let effective_display = max_display.saturating_sub(1).max(1);
        lines.push(Line::from(Span::styled(
            format!("│ … {} lines hidden (showing last {}) …", skipped, effective_display),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
        for line in &reasoning_wrapped[total - effective_display..] {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
    } else {
        let mut content_lines_added: u16 = 0;
        for line in &reasoning_wrapped {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
            content_lines_added += 1;
        }
        // Pad with empty placeholder lines to keep a fixed height
        while content_lines_added < content_budget {
            lines.push(Line::from(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            )));
            content_lines_added += 1;
        }
    }

    lines.push(Line::default());
}

/// Render reasoning content inline in the chat flow, inspired by Codex's
/// `ReasoningSummaryCell`. Reasoning is rendered as markdown with a dim/italic
/// style and a bullet-point prefix (`• ` first line, `  ` subsequent lines).
/// When the reasoning has more than COLLAPSE_THRESHOLD lines, it is collapsible
/// via the existing `render_collapsible_block`.
///
/// This returns the number of visual lines added (for the caller's positioning).
/// Build styled reasoning lines (Codex-inspired: `• ` prefix, dim/italic, markdown).
/// Returns `None` when reasoning is empty, so callers can skip.
fn build_reasoning_lines(
    reasoning: &str,
    area_width: u16,
) -> Option<Vec<Line<'static>>> {
    if reasoning.trim().is_empty() {
        return None;
    }

    // Use area_width - 4 so the "  • " / "    " prefix (4 chars) doesn't
    // cause the Paragraph widget to re-wrap the line beyond area_width.
    // This keeps each content line at exactly 1 visual line, preventing
    // total_lines from fluctuating when the collapsed reasoning content
    // changes between frames during streaming.
    let wrap_width = (area_width as usize).saturating_sub(4).max(10);
    let rendered = crate::ui::render::render_full(reasoning, Some(wrap_width));
    if rendered.is_empty() {
        return None;
    }

    let summary_style = Style::default()
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let styled: Vec<Line<'static>> = rendered
        .into_iter()
        .map(|mut line| {
            line.spans = line
                .spans
                .into_iter()
                .map(|span| {
                    let fg = span.style.fg.unwrap_or(Color::DarkGray);
                    span.patch_style(summary_style.fg(fg))
                })
                .collect();
            line
        })
        .collect();

    if styled.is_empty() {
        return None;
    }

    // Prepend "• " prefix to first line, "  " to subsequent lines
    let prefixed: Vec<Line<'static>> = styled
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { "  • " } else { "    " };
            let mut prefixed = Line::from(Span::raw(prefix));
            prefixed.spans.extend(line.spans);
            prefixed
        })
        .collect();

    Some(prefixed)
}

/// Render reasoning lines into the output, with collapsible support.
///
/// Always inserts a blank-line separator and "💭 Thinking..." header before the
/// content so distinct reasoning segments remain visually separated even when
/// each segment is shorter than `COLLAPSE_THRESHOLD`.
fn render_reasoning_inline(
    lines: &mut Vec<Line<'static>>,
    reasoning: &str,
    app: &mut App,
    section_id: &str,
    area_width: u16,
) {
    let Some(styled) = build_reasoning_lines(reasoning, area_width) else {
        return;
    };
    let total = styled.len();
        let collapsed = !app.collapsed_sections.contains(section_id);

    /// Compute how many visual lines a `Line` occupies after word-wrap at `width`.
    fn visual_lines(line: &ratatui::text::Line<'_>, width: u16) -> u16 {
        let line_width = line.width() as u16;
        if line_width == 0 || width == 0 {
            1
        } else {
            (line_width + width - 1) / width
        }
    }

    let vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

    // Blank-line separator to visually distinguish reasoning segments
    // (applies to BOTH large and small segments).
    lines.push(Line::default());

    if total > COLLAPSE_THRESHOLD {
        // Build clickable header line
        let hidden_count = total - COLLAPSE_THRESHOLD;
        let header = if collapsed {
            Line::from(vec![
                Span::styled(
                    "  ▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "💭 Thinking... ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!("({} lines hidden)", hidden_count),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    "  ▼ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "💭 Thinking...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])
        };

        app.collapsed_toggles
            .push((vis_pos, section_id.to_string(), total));

        lines.push(header);

        if collapsed {
            // Show the LAST COLLAPSE_THRESHOLD lines (newest content visible by default).
            let start = total.saturating_sub(COLLAPSE_THRESHOLD);
            for line in styled.iter().skip(start) {
                lines.push(line.clone());
            }
        } else {
            // Show all lines when expanded
            for line in &styled {
                lines.push(line.clone());
            }
        }
    } else {
        // Small reasoning block: show a minimal header so even short segments
        // are visually separated from adjacent reasoning blocks.
        lines.push(Line::from(vec![
            Span::styled(
                "  💭 Thinking...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
        lines.extend(styled);
    }
}

/// Render an active streaming reasoning segment inline (Codex-style).
/// Shows the reasoning content with dim/italic style and `• ` prefix.
/// This handles only the currently streaming segment — completed segments
/// are rendered separately via `render_reasoning_inline` to match the
/// final per-entry output format.
fn render_streaming_reasoning_inline(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    reasoning: &str,
    app: &mut App,
    section_id: &str,
    area_width: u16,
) {
    if app.config.agent.thinking_display == "hidden" {
        return;
    }

    if reasoning.trim().is_empty() {
        return;
    }

    if let Some(styled) = build_reasoning_lines(reasoning, area_width) {
        let section_id_owned = section_id.to_string();
        let collapsed = !app.collapsed_sections.contains(&section_id_owned);
        let total = styled.len();

        // Compute current visual line position for toggle placement
        fn visual_lines(line: &ratatui::text::Line<'_>, width: u16) -> u16 {
            let line_width = line.width() as u16;
            if line_width == 0 || width == 0 { 1 } else { (line_width + width - 1) / width }
        }
        let vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

        // Blank-line separator to visually distinguish from prior reasoning segment
        lines.push(Line::default());

        if total > COLLAPSE_THRESHOLD {
            // Build clickable header line
            let hidden_count = total - COLLAPSE_THRESHOLD;
            let header = if collapsed {
                Line::from(vec![
                    Span::styled(
                        "  ▶ ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "💭 Thinking... ",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                    Span::styled(
                        format!("({} lines hidden)", hidden_count),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        "  ▼ ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "💭 Thinking...",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ])
            };

            app.collapsed_toggles.push((vis_pos, section_id.to_string(), total));

            lines.push(header);

            if collapsed {
                // Show the LAST COLLAPSE_THRESHOLD lines (newest content visible by default).
                let start = total.saturating_sub(COLLAPSE_THRESHOLD);
                for line in styled.iter().skip(start) {
                    lines.push(line.clone());
                }
            } else {
                // Show all lines when expanded
                for line in &styled {
                    lines.push(line.clone());
                }
            }
        } else {
            // Small active reasoning block: show a minimal header so even short
            // streaming segments are visually separated from adjacent blocks.
            lines.push(Line::from(vec![
                Span::styled(
                    "  💭 Thinking...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
            lines.extend(styled);
        }
    }
}

/// Word-wrap text to fit within `max_width` characters, splitting at word boundaries.
/// Preserves explicit newlines. Returns a flat list of wrapped lines.
fn word_wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for source_line in text.split('\n') {
        if source_line.is_empty() {
            result.push(String::new());
            continue;
        }
        // Wrap the line
        let mut remaining = source_line;
        while !remaining.is_empty() {
            if remaining.len() <= max_width {
                result.push(remaining.to_string());
                break;
            }
            // Find the last space within max_width to break at
            let mut break_at = remaining[..max_width].rfind(' ').unwrap_or(max_width);
            // If break_at is 0, force-break at max_width to avoid infinite loop
            if break_at == 0 {
                break_at = max_width;
            }
            result.push(remaining[..break_at].to_string());
            remaining = remaining[break_at..].trim_start();
        }
    }
    result
}

/// Parse a unified diff string and return (additions, deletions) counts.
fn count_diff_stats(diff: &str) -> (usize, usize) {
    let mut adds = 0usize;
    let mut dels = 0usize;
    for line in diff.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            adds += 1;
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            dels += 1;
        }
    }
    (adds, dels)
}

fn try_render_file_tool_result(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    entry_idx: usize,
    app: &mut App,
    show_git_diff: bool,
    area_width: u16,
) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;

    // Check if this is a file tool result by looking for a path field
    let path = value.get("path")?.as_str()?;

    // Get the current diff from git (always up-to-date, not stale from tool output)
    // Results are cached per frame in git_diff_cache to avoid spawning git
    // subprocesses for the same file multiple times in one render pass.
    // When a review baseline exists, diff against it instead of HEAD
    // to show only changes since the last review.
    let git_diff_from_tool = value.get("git_diff").and_then(|v| v.as_str()).unwrap_or("");
    let git_diff = if let Some(cached) = app.git_diff_cache.get(path) {
        cached.clone()
    } else {
        let mut git_args = vec!["diff", "--no-color"];
        if let Some(ref baseline) = app.review_baseline {
            git_args.push(baseline.as_str());
        }
        git_args.push("--");
        git_args.push(path);
        let diff = match std::process::Command::new("git")
            .args(&git_args)
            .output()
        {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout);
                if s.trim().is_empty() { git_diff_from_tool.to_string() } else { s.to_string() }
            }
            _ => git_diff_from_tool.to_string(),
        };
        app.git_diff_cache.insert(path.to_string(), diff.clone());
        diff
    };
    let git_diff = git_diff.as_str();

    // Determine the action type
    let action = if value.get("bytes_written").is_some() {
        "File Write"
    } else if value.get("replacements").is_some() {
        "File Update"
    } else if let Some(deleted_type) = value.get("deleted_type").and_then(|t| t.as_str()) {
        match deleted_type {
            "file" => "File Delete",
            "directory" => "Directory Delete",
            "snippet" => "Snippet Delete",
            _ => return None,
        }
    } else {
        // Not a file modification (e.g. file_read) — don't render a header
        return None;
    };

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            "📝 ",
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            action,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "  File: ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            path.to_string(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Show the git diff (collapsible)
    if show_git_diff && !git_diff.is_empty() {
        // Compute diff stats for display
        let (adds, dels) = count_diff_stats(git_diff);
        lines.push(Line::from(vec![
            Span::styled(
                "  ─── git diff ",
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("+{} -{}", adds, dels),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ───",
                Style::default().fg(Color::Green),
            ),
        ]));

        // Build styled diff lines with preserved @@ hunk headers
        let diff_lines: Vec<Line> = git_diff
            .lines()
            .map(|line| {
                let line = line.trim_end();
                // Empty lines should have a space for rendering
                let display_line = if line.is_empty() { " " } else { line };

                let (prefix, style) = if line.starts_with("@@") {
                    // Hunk header — bright cyan with bold
                    (
                        format!("  {}", display_line),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if line.starts_with('+') && !line.starts_with("+++") {
                    // Added lines - green
                    (format!("  {}", display_line), Style::default().fg(Color::Green))
                } else if line.starts_with('-') && !line.starts_with("---") {
                    // Removed lines - red
                    (format!("  {}", display_line), Style::default().fg(Color::Red))
                } else if line.starts_with("+++") || line.starts_with("---") {
                    // File header lines - gray bold
                    (
                        format!("  {}", display_line),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if line.starts_with("diff ")
                    || line.starts_with("index ")
                    || line.starts_with("rename ")
                    || line.starts_with("similarity ")
                    || line.starts_with("new file ")
                    || line.starts_with("deleted file ")
                {
                    // Metadata lines - gray
                    (format!("  {}", display_line), Style::default().fg(Color::DarkGray))
                } else if line.starts_with('\\') {
                    // "No newline at end of file" - gray
                    (format!("  {}", display_line), Style::default().fg(Color::DarkGray))
                } else {
                    // Context lines - default
                    (format!("  {}", display_line), Style::default())
                };

                Line::from(Span::styled(prefix, style))
            })
            .collect();

        let section_id = format!("gd_{}", entry_idx);
        render_collapsible_block(lines, app, &section_id, diff_lines, area_width);
    }

    Some(())
}

/// Try to parse tool content as a file_outline result and render it with colored spans.
/// Returns Some(()) if the content was successfully rendered as a file outline.
fn try_render_file_outline(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    entry_idx: usize,
    app: &mut App,
    area_width: u16,
) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let outline = value.get("outline")?.as_str()?;
    let path = value.get("path")?.as_str()?;

    // Header: 📋 File Outline + path
    lines.push(Line::from(vec![
        Span::styled(
            "📋 ",
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            "File Outline",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "  File: ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            path.to_string(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Build styled outline lines
    let mut outline_content: Vec<Line> = Vec::new();
    for line in outline.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        // Total line: "Total: N lines"
        if let Some(total) = line.strip_prefix("Total: ") {
            outline_content.push(Line::from(vec![
                Span::styled(
                    "  ",
                    Style::default(),
                ),
                Span::styled(
                    "── ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("Total: {}", total),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }

        // Structure line format: "├── [1-10: 10 lines] fn main"
        // or "└── [1-10: 10 lines] fn main"
        if let Some((_prefix, rest)) = line.split_once("── ") {
            let tree_char = line.chars().next().unwrap_or(' ');

            let mut spans = Vec::new();
            // Tree prefix character
            spans.push(Span::styled(
                format!("  {}", tree_char),
                Style::default().fg(Color::DarkGray),
            ));

            if let Some((range_str, rest_after_range)) = rest.split_once("] ") {
                // Range: "── [1-10: 10 lines"
                let range_part = format!("── {}", range_str);
                spans.push(Span::styled(
                    range_part,
                    Style::default().fg(Color::Blue),
                ));
                spans.push(Span::styled(
                    "] ",
                    Style::default().fg(Color::Blue),
                ));

                // Split rest into kind and name
                let after_range = rest_after_range.trim_start();
                if let Some((kind, name)) = after_range.split_once(char::is_whitespace) {
                    let name = name.trim_start();
                    spans.push(Span::styled(
                        kind.to_string(),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ));
                    if !name.is_empty() {
                        spans.push(Span::styled(
                            format!(" {}", name),
                            Style::default().fg(Color::Green),
                        ));
                    }
                } else {
                    spans.push(Span::styled(
                        after_range.to_string(),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ));
                }
            } else {
                // Fallback: show the rest as-is
                let rest_display = format!("── {}", rest);
                spans.push(Span::styled(
                    rest_display,
                    Style::default(),
                ));
            }

            outline_content.push(Line::from(spans));
        } else {
            // Fallback for lines that don't match the tree format
            outline_content.push(Line::from(format!("  {}", line)));
        }
    }

    // Render collapsible outline content
    let section_id = format!("ol_{}", entry_idx);
    render_collapsible_block(lines, app, &section_id, outline_content, area_width);

    Some(())
}

fn try_render_todos(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    max_width: Option<usize>,
) -> Option<()> {
    // Detect Markdown-formatted todos output (starts with the todos header)
    if !content.starts_with("## 📋 Todos") {
        return None;
    }

    let md = render_full(content, max_width);
    lines.extend(md);
    Some(())
}

/// Render status messages at the bottom.
fn render_status_messages(lines: &mut Vec<ratatui::text::Line<'static>>, app: &App, area: Rect) {
    if app.status_messages.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    )));
    for msg in &app.status_messages {
        for line in msg.lines() {
            lines.push(Line::from(line.to_string()));
        }
    }
}

/// Render an aggregated file change summary block at the end of a turn.
/// Inspired by Codex's change summary display: shows a compact list of
/// all files that were modified in the current turn with +X -Y stats.
/// Only renders when not streaming, and when there are actual changes.
fn render_file_change_summary(lines: &mut Vec<ratatui::text::Line<'static>>, app: &App, max_width: Option<usize>) {
    if app.is_streaming {
        return;
    }

    // Find the most recent user message boundary — scan backward from the end
    // of chat_history for the last user message.
    let turn_start = app.chat_history.iter().rposition(|e| e.role == "user");
    let start_idx = turn_start.unwrap_or(0);

    // Aggregate per-file stats from tool entries in the current turn
    let mut per_file: Vec<(String, usize, usize)> = Vec::new();
    let mut total_adds = 0usize;
    let mut total_dels = 0usize;

    for entry in app.chat_history[start_idx..].iter() {
        if entry.role != "tool" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&entry.content) {
            // Only show file-change results (file_write, file_update, file_delete)
            let has_file_op = value.get("bytes_written").is_some()
                || value.get("replacements").is_some()
                || value.get("deleted_type").is_some();
            if !has_file_op {
                continue;
            }
            if let Some(path) = value.get("path").and_then(|p| p.as_str()) {
                let diff_str = value.get("git_diff").and_then(|v| v.as_str()).unwrap_or("");
                let (adds, dels) = count_diff_stats(diff_str);
                if adds > 0 || dels > 0 {
                    total_adds += adds;
                    total_dels += dels;
                    per_file.push((path.to_string(), adds, dels));
                }
            }
        }
    }

    if per_file.is_empty() {
        return;
    }

    let area_width = max_width.unwrap_or(80) as usize;

    // Separator line
    lines.push(Line::from(Span::styled(
        "─".repeat(area_width),
        Style::default().fg(Color::DarkGray),
    )));

    // Header: "📝 N files changed: +X -Y"
    lines.push(Line::from(vec![
        Span::styled(
            "📝 ",
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("{} file{} changed: ", per_file.len(), if per_file.len() == 1 { "" } else { "s" }),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("+{} -{}", total_adds, total_dels),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Per-file entries
    for (path, adds, dels) in &per_file {
        let (add_color, del_color) = if *adds > 0 && *dels > 0 {
            (Color::Green, Color::Red)
        } else if *adds > 0 {
            (Color::Green, Color::DarkGray)
        } else {
            (Color::DarkGray, Color::Red)
        };

        lines.push(Line::from(vec![
            Span::styled(
                "  ",
                Style::default(),
            ),
            Span::styled(
                path.clone(),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                "  ",
                Style::default(),
            ),
            Span::styled(
                format!("+{}", adds),
                Style::default().fg(add_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" -{}", dels),
                Style::default().fg(del_color),
            ),
        ]));
    }

    lines.push(Line::default());
}
