use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, ChatEntry};
use crate::ui::render::{render_full, render_streaming_markdown};
use crate::ui::terminal;

/// Threshold for collapsing content: sections with more lines than this are collapsed.
const COLLAPSE_THRESHOLD: usize = 8;

/// Render the chat history area including streaming content and reasoning.
pub fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    if app.show_banner {
        render_banner(f, app, area);
        return;
    }

    let has_reasoning = app.config.agent.thinking_display != "hidden"
        && (app.is_reasoning_active || !app.last_reasoning.is_empty());

    let width = Some(area.width as usize);

    // ── Review reasoning is rendered in ALL branches ────────────────────
    // Review reasoning (`app.review_reasoning`) is transient thinking content
    // from the review agent's LLM calls. It must be visible regardless of the
    // main agent's reasoning state, so we always call `render_review_reasoning`.

    if !app.is_streaming && has_reasoning && app.show_inline_reasoning {
        let mut lines: Vec<ratatui::text::Line> = Vec::new();
        render_chat_with_reasoning(&mut lines, app, width);
        render_review_reasoning(&mut lines, app, width, 8);
        render_status_messages(&mut lines, app, area);
        render_paragraph_with_scroll(f, app, lines, area);
    } else if has_reasoning {
        // Split layout: scrolling content on top, fixed reasoning block at bottom
        let max_height = app.config.agent.thinking_display_height;
        let total_reserve = max_height + 2; // header + content + trailing empty
        let (content_area, reasoning_area) = if area.height > total_reserve {
            let areas = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(1),
                    ratatui::layout::Constraint::Length(total_reserve),
                ])
                .split(area);
            (areas[0], areas[1])
        } else {
            // Not enough space, use full area for reasoning
            let areas = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(0),
                    ratatui::layout::Constraint::Min(1),
                ])
                .split(area);
            (areas[0], areas[1])
        };

        // Top: history + streaming content + review reasoning
        let mut content_lines: Vec<ratatui::text::Line> = Vec::new();
        render_chat_messages(&mut content_lines, app, width);
        render_review_reasoning(&mut content_lines, app, width, 8);
        render_streaming_content(&mut content_lines, app, width);
        render_status_messages(&mut content_lines, app, area);
        render_paragraph_with_scroll(f, app, content_lines, content_area);

        // Bottom: fixed reasoning block
        let mut reasoning_lines: Vec<ratatui::text::Line> = Vec::new();
        let reasoning_text = if !app.streaming_reasoning.is_empty() {
            &app.streaming_reasoning
        } else {
            &app.last_reasoning
        };
        render_reasoning_block(&mut reasoning_lines, reasoning_text, max_height);
        let paragraph = Paragraph::new(reasoning_lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(paragraph, reasoning_area);
    } else {
        let mut lines: Vec<ratatui::text::Line> = Vec::new();
        render_chat_messages(&mut lines, app, width);
        // Render review reasoning (transient — not added to chat history)
        render_review_reasoning(&mut lines, app, width, 8);
        render_streaming_content(&mut lines, app, width);
        render_status_messages(&mut lines, app, area);
        render_paragraph_with_scroll(f, app, lines, area);
    }
}

fn render_paragraph_with_scroll(f: &mut Frame, app: &mut App, lines: Vec<ratatui::text::Line>, area: Rect) {
    let actual_lines = Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(area.width) as u16;

    app.total_lines = actual_lines;
    app.chat_area_height = area.height;

    let max_scroll = actual_lines.saturating_sub(area.height);

    if app.auto_scroll {
        // Use max() to prevent scroll from decreasing (monotonic),
        // which avoids visual jumping when line_count fluctuates due to word-wrap reflow.
        app.scroll = app.scroll.max(max_scroll);
    }

    // Clamp scroll to valid range. Without this, layout transitions
    // (streaming→done, collapse toggle) can leave scroll pointing past
    // the end of the new smaller content, resulting in a blank screen.
    app.scroll = app.scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .scroll((app.scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);
}

/// Render the startup banner.
fn render_banner(f: &mut Frame, app: &mut App, area: Rect) {
    let banner = terminal::make_startup_text();
    app.total_lines = banner.height() as u16;
    let paragraph = Paragraph::new(banner)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);
}

/// Render chat with reasoning placed before the last assistant message.
fn render_chat_with_reasoning(lines: &mut Vec<ratatui::text::Line>, app: &mut App, max_width: Option<usize>) {
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

    // Reasoning block
    let max_height = app.config.agent.thinking_display_height;
    render_reasoning_block(lines, &app.last_reasoning, max_height);

    // The last assistant message
    if let Some(idx) = last_assistant_idx {
        let content = &app.chat_history[idx].content;
        let md = render_full(content, max_width);
        lines.extend(md);
        lines.push(Line::default());
    }
}

/// Render all chat messages in order.
fn render_chat_messages(lines: &mut Vec<ratatui::text::Line>, app: &mut App, max_width: Option<usize>) {
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
fn render_message(lines: &mut Vec<ratatui::text::Line>, entry: &ChatEntry, entry_idx: usize, app: &mut App, max_width: Option<usize>, show_tool_calls: bool, show_tool_details: bool) {
    let area_width = max_width.unwrap_or(80) as u16;
    match entry.role.as_str() {
        "user" => {
            lines.push(Line::from(vec![
                Span::styled(
                    "You: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(entry.content.to_string()),
            ]));
            lines.push(Line::default());
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
            // Display normal content
            if !entry.content.is_empty() {
                let md = render_full(&entry.content, max_width);
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

/// Render reasoning block with blockquote style, always exactly max_height lines tall.
/// Fixed height prevents the content below from jumping as reasoning streams in.
/// Render review reasoning block — transient thinking content shown during code review.
/// Uses the same blockquote style as the main reasoning block but with a shorter fixed height
/// since review phases complete quickly and reasoning is shown within the scrollable chat area.
fn render_review_reasoning(lines: &mut Vec<ratatui::text::Line>, app: &App, _max_width: Option<usize>, max_height: u16) {
    if !app.is_reviewing || app.review_reasoning.is_empty() {
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

    let reasoning_lines: Vec<&str> = app.review_reasoning.lines().collect();
    let total = reasoning_lines.len();
    let max_display = content_budget as usize;

    if total > max_display {
        let skipped = total - max_display;
        // "hidden" message line also counts toward the budget
        let effective_display = max_display.saturating_sub(1).max(1);
        lines.push(Line::from(Span::styled(
            format!("│ … {} lines hidden (showing last {}) …", skipped, effective_display),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
        for line in &reasoning_lines[total - effective_display..] {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
    } else {
        let mut content_lines_added: u16 = 0;
        for line in &reasoning_lines {
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

fn render_reasoning_block(lines: &mut Vec<ratatui::text::Line>, reasoning: &str, max_height: u16) {
    // Reserve lines for: header ("💭 Thinking:") and trailing empty line
    let header_reserve: u16 = 2; // header + trailing empty
    let content_budget = max_height.saturating_sub(header_reserve).max(1);
    lines.push(Line::from(Span::styled(
        "💭 Thinking:",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    let reasoning_lines: Vec<&str> = reasoning.lines().collect();
    let total = reasoning_lines.len();
    let max_display = content_budget as usize;

    if total > max_display {
        let skipped = total - max_display;
        // "hidden" message line also counts toward the budget
        let effective_display = max_display.saturating_sub(1);
        lines.push(Line::from(Span::styled(
            format!("│ … {} lines hidden (showing last {}) …", skipped, effective_display),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
        for line in &reasoning_lines[total - effective_display..] {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
    } else {
        let mut content_lines_added: u16 = 0;
        for line in reasoning_lines {
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

/// Render streaming content (text and tool calls).
fn render_streaming_content(lines: &mut Vec<ratatui::text::Line>, app: &mut App, max_width: Option<usize>) {
    let area_width = max_width.unwrap_or(80) as u16;
    if !app.is_streaming {
        return;
    }

    // Render persistent todos FIRST so they stay visible even during
    // inter-turn waiting periods (between tool execution and next text).
    // streaming_todos is set when a write_todos tool result arrives and
    // cleared when new streaming text arrives or streaming ends.
    // Skip if streaming_tool_result is still present (it will render the
    // same content via .take() on this same frame).
    if app.streaming_tool_result.is_none() {
        if let Some(ref todos) = app.streaming_todos {
            try_render_todos(lines, todos, max_width);
            lines.push(Line::default());
        }
    }

    if !app.streaming_text.is_empty() || app.current_tool_call.is_some() || app.streaming_tool_result.is_some() {
        lines.push(Line::from(vec![Span::styled(
            "Assistant: ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]));
        if !app.streaming_text.is_empty() {
            let md_lines = render_streaming_markdown(&app.streaming_text, max_width);
            lines.extend(md_lines);
        }
        // Display current executing tool call with detailed info (if config allows)
        if app.config.agent.show_tool_calls {
            if let Some(ref tool_call) = app.current_tool_call {
                lines.push(Line::from(vec![
                    Span::styled(
                        "⚙️ ",
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        tool_call.name.clone(),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                ]));
                // Try to parse arguments as JSON to show command nicely
                if app.config.agent.show_tool_details {
                    match serde_json::from_str::<serde_json::Value>(&tool_call.arguments) {
                        Ok(val) if !val.is_null() => {
                            if let Some(cmd) = val.get("command").and_then(|c| c.as_str()) {
                                lines.push(Line::from(format!("  {}", cmd)));
                            } else {
                                lines.push(Line::from(format!("  {}", val)));
                            }
                        }
                        _ => {
                            // Show raw arguments if not yet valid JSON (still streaming)
                            if !tool_call.arguments.is_empty() {
                                lines.push(Line::from(format!("  {}", tool_call.arguments)));
                            }
                        }
                    }
                }
            }
        }

        // Display completed tool result with truncated content display.
        // Take the content to avoid borrowing conflicts with &mut App calls
        // and avoid cloning the potentially large content string every frame.
        let streaming_content = app.streaming_tool_result.take().map(|(_name, content)| content);
        if let Some(ref content) = streaming_content {
            // Todos results are ALWAYS shown — they contain planning progress.
            // Check this BEFORE the show_tool_details guard so todos are visible
            // regardless of tool display settings.
            if try_render_todos(lines, content, max_width).is_some() {
                // Already rendered the todos, nothing more to do.
            }
            // Only render other tool results when show_tool_calls and show_tool_details
            // are both enabled. Without this guard, the specialized renderers below
            // (file_tool_result, shell_exec, file_outline) would render their content
            // even when the user has configured show_tool_calls = false.
            else if app.config.agent.show_tool_calls && app.config.agent.show_tool_details {
                // Try rendering as file tool result (git diff) first
                // Use a special high index for streaming section IDs — only one
                // streaming tool result exists at a time, so section IDs won't clash.
                if try_render_file_tool_result(lines, content, usize::MAX, app, false, area_width).is_none()
                    && try_render_shell_exec_result(lines, content, usize::MAX, app, true, area_width).is_none()
                    && try_render_file_outline(lines, content, usize::MAX, app, area_width).is_none()
                {
                    // Lightweight rendering: only show first few lines with a note
                    let total_lines = content.lines().count();
                    let max_preview = 5;
                    lines.push(Line::from(vec![
                        Span::styled(
                            "🔧 Tool Result:",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    for line in content.lines().take(max_preview) {
                        lines.push(Line::from(line.to_string()));
                    }
                    if total_lines > max_preview {
                        lines.push(Line::from(Span::styled(
                            format!("  ... {} more lines (tool result shown briefly)", total_lines - max_preview),
                            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                        )));
                    }
                }
            }
        }
    } else if !app.streaming_status.is_empty() {
        // Show a status message during inter-turn waiting periods
        // (e.g. "⏳ Waiting for model response..." after tool execution)
        lines.push(Line::from(Span::styled(
            app.streaming_status.clone(),
            Style::default().fg(Color::Yellow),
        )));
    } else if app.streaming_reasoning.is_empty() {
        lines.push(Line::from(Span::styled(
            "⏳ Generating response...",
            Style::default().fg(Color::Yellow),
        )));
    } else {
        // Reasoning is active but no text/tool call yet — avoid blank content area
        lines.push(Line::from(Span::styled(
            "💭 Thinking...",
            Style::default().fg(Color::Yellow),
        )));
    }
}

/// and render it with a collapsible git diff display.
/// Returns Some(()) if the content contained a git_diff field.
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
    let git_diff = value.get("git_diff")?.as_str()?;

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
            _ => "File Operation",
        }
    } else {
        "File Operation"
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
        lines.push(Line::from(Span::styled(
            "  ─── git diff ───",
            Style::default().fg(Color::Green),
        )));

        // Build styled diff lines (inspired by codebuff's DiffViewer)
        // Filter out hunk headers (@@) for a cleaner view - matches codebuff's behavior
        let diff_lines: Vec<Line> = git_diff
            .lines()
            .filter(|line| !line.starts_with("@@"))
            .map(|line| {
                let line = line.trim_end();
                // Empty lines should have a space for rendering
                let display_line = if line.is_empty() { " " } else { line };

                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    // Added lines - green
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    // Removed lines - red
                    Style::default().fg(Color::Red)
                } else if line.starts_with("+++") || line.starts_with("---") {
                    // File header lines - gray bold
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else if line.starts_with("diff ")
                    || line.starts_with("index ")
                    || line.starts_with("rename ")
                    || line.starts_with("similarity ")
                    || line.starts_with("new file ")
                    || line.starts_with("deleted file ")
                {
                    // Metadata lines - gray
                    Style::default().fg(Color::DarkGray)
                } else if line.starts_with('\\') {
                    // "No newline at end of file" - gray
                    Style::default().fg(Color::DarkGray)
                } else {
                    // Context lines - default
                    Style::default()
                };

                Line::from(Span::styled(format!("  {}", display_line), style))
            })
            .collect();

        let section_id = format!("gd_{}", entry_idx);
        render_collapsible_block(lines, app, &section_id, diff_lines, area_width);
    }

    Some(())
}

/// Try to parse tool content as a shell exec result and render it with collapsible
/// stdout/stderr blocks.
/// Returns Some(()) if the content was successfully rendered as a shell exec result.
fn try_render_shell_exec_result(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    entry_idx: usize,
    app: &mut App,
    show_tool_details: bool,
    area_width: u16,
) -> Option<()> {
    // When details are hidden, don't render shell results at all
    if !show_tool_details {
        return None;
    }

    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let cmd = value.get("command")?.as_str()?;

    // Header
    lines.push(Line::from(vec![
        Span::styled("⚙️ ", Style::default().fg(Color::Yellow)),
        Span::styled(
            "Shell Exec",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(format!("  Command: {}", cmd)));

    if let Some(exit_code) = value.get("exit_code") {
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

    if let Some(timed_out) = value.get("timed_out").and_then(|t| t.as_bool()) {
        if timed_out {
            lines.push(Line::from(Span::styled(
                "  ⚠ Timed out",
                Style::default().fg(Color::Red),
            )));
        }
    }

    // Show stdout/stderr
    if let Some(stdout) = value.get("stdout").and_then(|s| s.as_str()) {
        if !stdout.is_empty() {
            lines.push(Line::from(Span::styled(
                "  ─── stdout ───",
                Style::default().fg(Color::DarkGray),
            )));
            let stdout_lines: Vec<Line> = stdout
                .lines()
                .map(|l| Line::from(format!("  {}", l)))
                .collect();
            let section_id = format!("so_{}", entry_idx);
            render_collapsible_block(lines, app, &section_id, stdout_lines, area_width);
        }
    }

    if let Some(stderr) = value.get("stderr").and_then(|s| s.as_str()) {
        if !stderr.is_empty() {
            lines.push(Line::from(Span::styled(
                "  ─── stderr ───",
                Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
            )));
            let stderr_lines: Vec<Line> = stderr
                .lines()
                .map(|l| Line::from(format!("  {}", l)))
                .collect();
            let section_id = format!("se_{}", entry_idx);
            render_collapsible_block(lines, app, &section_id, stderr_lines, area_width);
        }
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
fn render_status_messages(lines: &mut Vec<ratatui::text::Line>, app: &App, area: Rect) {
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
