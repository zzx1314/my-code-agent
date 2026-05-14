use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, ChatEntry};
use crate::ui::render::{render_full, render_streaming_markdown};
use crate::ui::terminal;

/// Render the chat history area including streaming content and reasoning.
pub fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    if app.show_banner {
        render_banner(f, app, area);
        return;
    }

    let has_reasoning = app.config.agent.thinking_display != "hidden"
        && (app.is_streaming || !app.last_reasoning.is_empty());

    let width = Some(area.width as usize);

    if !app.is_streaming && has_reasoning && app.show_inline_reasoning {
        let mut lines: Vec<ratatui::text::Line> = Vec::new();
        render_chat_with_reasoning(&mut lines, app, width);
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

        // Top: history + streaming content
        let mut content_lines: Vec<ratatui::text::Line> = Vec::new();
        render_chat_messages(&mut content_lines, app, width);
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

    if app.auto_scroll {
        // Use max() to prevent scroll from decreasing (monotonic),
        // which avoids visual jumping when line_count fluctuates due to word-wrap reflow.
        let new_scroll = actual_lines.saturating_sub(area.height);
        app.scroll = app.scroll.max(new_scroll);
    }

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

    // Messages before the last assistant message
    for entry in &app.chat_history[..split_idx] {
        render_message(lines, entry, max_width, show_tool_calls_in_history);
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
fn render_chat_messages(lines: &mut Vec<ratatui::text::Line>, app: &App, max_width: Option<usize>) {
    let show_tool_calls_in_history = app.config.agent.show_tool_calls_in_history;
    for entry in &app.chat_history {
        render_message(lines, entry, max_width, show_tool_calls_in_history);
    }
}

/// Render a single message with role-based styling.
fn render_message(lines: &mut Vec<ratatui::text::Line>, entry: &ChatEntry, max_width: Option<usize>, show_tool_calls: bool) {
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
                        if let Some(cmd) = args.get("command").and_then(|c| c.as_str()) {
                            lines.push(Line::from(format!("  {}", cmd)));
                        } else {
                            lines.push(Line::from(format!("  {}", args)));
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
            // Tool results are only shown when show_tool_calls is enabled
            if show_tool_calls {
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
                                for line in stdout.lines() {
                                    lines.push(Line::from(format!("  {}", line)));
                                }
                            }
                        }
                        if let Some(stderr) = output.get("stderr").and_then(|s| s.as_str()) {
                            if !stderr.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    "  ─── stderr ───",
                                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
                                )));
                                for line in stderr.lines() {
                                    lines.push(Line::from(format!("  {}", line)));
                                }
                            }
                        }
                        lines.push(Line::default());
                        return;
                    }
                }
                // Check if it's a file_outline result
                if try_render_file_outline(lines, &entry.content).is_some() {
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
fn render_streaming_content(lines: &mut Vec<ratatui::text::Line>, app: &App, max_width: Option<usize>) {
    if !app.is_streaming {
        return;
    }

    if !app.streaming_text.is_empty() || app.current_tool_call.is_some() {
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
    } else if app.streaming_reasoning.is_empty() {
        lines.push(Line::from(Span::styled(
            "⏳ Generating response...",
            Style::default().fg(Color::Yellow),
        )));
    }
}

/// Try to parse tool content as a file_outline result and render it with colored spans.
/// Returns Some(()) if the content was successfully rendered as a file outline.
fn try_render_file_outline(lines: &mut Vec<ratatui::text::Line>, content: &str) -> Option<()> {
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

    for line in outline.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        // Total line: "Total: N lines"
        if let Some(total) = line.strip_prefix("Total: ") {
            lines.push(Line::from(vec![
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

            lines.push(Line::from(spans));
        } else {
            // Fallback for lines that don't match the tree format
            lines.push(Line::from(format!("  {}", line)));
        }
    }

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
