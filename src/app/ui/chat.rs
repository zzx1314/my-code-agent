use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::App;
use crate::ui::render::{render_full, render_streaming_markdown};
use crate::ui::terminal;

/// Render the chat history area including streaming content and reasoning.
pub fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    if app.show_banner {
        render_banner(f, app, area);
        return;
    }

    let mut lines: Vec<ratatui::text::Line> = Vec::new();

    let has_reasoning =
        app.config.agent.thinking_display != "hidden" && !app.last_reasoning.is_empty();

    let width = Some(area.width as usize);

    if !app.is_streaming && has_reasoning && app.show_inline_reasoning {
        render_chat_with_reasoning(&mut lines, app, width);
    } else {
        render_chat_messages(&mut lines, app, width);
        render_streaming_reasoning(&mut lines, app);
    }

    render_streaming_content(&mut lines, app, width);
    render_status_messages(&mut lines, app, area);

    let actual_lines = Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(area.width) as u16;

    app.total_lines = actual_lines;
    app.chat_area_height = area.height;

    if app.auto_scroll {
        app.scroll = actual_lines.saturating_sub(area.height);
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
        .rposition(|(role, _)| role == "assistant");
    let split_idx = last_assistant_idx.unwrap_or(app.chat_history.len());

    // Messages before the last assistant message
    for (role, content) in &app.chat_history[..split_idx] {
        render_message(lines, role, content, max_width);
    }

    // Reasoning block
    let max_height = app.config.agent.thinking_display_height;
    render_reasoning_block(lines, &app.last_reasoning, max_height);

    // The last assistant message
    if let Some(idx) = last_assistant_idx {
        let (_role, content) = &app.chat_history[idx];
        let md = render_full(content, max_width);
        lines.extend(md);
        lines.push(Line::default());
    }
}

/// Render all chat messages in order.
fn render_chat_messages(lines: &mut Vec<ratatui::text::Line>, app: &App, max_width: Option<usize>) {
    for (role, content) in &app.chat_history {
        render_message(lines, role, content, max_width);
    }
}

/// Render a single message with role-based styling.
fn render_message(lines: &mut Vec<ratatui::text::Line>, role: &str, content: &str, max_width: Option<usize>) {
    match role {
        "user" => {
            lines.push(Line::from(vec![
                Span::styled(
                    "You: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(content.to_string()),
            ]));
            lines.push(Line::default());
        }
        "assistant" => {
            let md = render_full(content, max_width);
            lines.extend(md);
            lines.push(Line::default());
        }
        _ => {
            lines.push(Line::from(format!("{}: {}", role, content)));
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

/// Render reasoning during streaming if applicable.
fn render_streaming_reasoning(lines: &mut Vec<ratatui::text::Line>, app: &App) {
    let show_reasoning = app.config.agent.thinking_display != "hidden"
        && app.is_streaming
        && (!app.streaming_reasoning.is_empty() || !app.last_reasoning.is_empty());

    if show_reasoning {
        let text = if !app.streaming_reasoning.is_empty() {
            &app.streaming_reasoning
        } else {
            &app.last_reasoning
        };
        let max_height = app.config.agent.thinking_display_height;
        render_reasoning_block(lines, text, max_height);
    }
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
        if let Some(ref tool_name) = app.current_tool_call {
            lines.push(Line::from(vec![Span::styled(
                format!("  ⏳ {}...", tool_name),
                Style::default().fg(Color::DarkGray),
            )]));
        }
    } else if app.streaming_reasoning.is_empty() {
        lines.push(Line::from(Span::styled(
            "⏳ Generating response...",
            Style::default().fg(Color::Yellow),
        )));
    }
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
