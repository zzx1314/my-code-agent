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
        let banner = terminal::make_startup_text();
        app.total_lines = banner.height() as u16;
        let paragraph = Paragraph::new(banner)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(paragraph, area);
        return;
    }

    let mut lines: Vec<ratatui::text::Line> = Vec::new();

    // Render chat history with correct reasoning placement
    let has_reasoning =
        app.config.agent.thinking_display != "hidden" && !app.last_reasoning.is_empty();

    if !app.is_streaming && has_reasoning && app.show_inline_reasoning {
        // Non-streaming with reasoning: render reasoning BEFORE the last assistant message
        let last_assistant_idx = app
            .chat_history
            .iter()
            .rposition(|(role, _)| role == "assistant");
        let split_idx = last_assistant_idx.unwrap_or(app.chat_history.len());

        // Render messages before the last assistant message
        for (role, content) in &app.chat_history[..split_idx] {
            match role.as_str() {
                "user" => {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "You: ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(content.clone()),
                    ]));
                    lines.push(Line::default());
                }
                "assistant" => {
                    let md = render_full(content);
                    lines.extend(md);
                    lines.push(Line::default());
                }
                _ => {
                    lines.push(Line::from(format!("{}: {}", role, content)));
                    lines.push(Line::default());
                }
            }
        }

        // Render reasoning with blockquote style
        lines.push(Line::from(Span::styled(
            "💭 Thinking:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for line in app.last_reasoning.lines() {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(line, Style::default().fg(Color::DarkGray)),
            ]));
        }
        lines.push(Line::default());

        // Render the last assistant message (if any)
        if let Some(idx) = last_assistant_idx {
            let (_role, content) = &app.chat_history[idx];
            let md = render_full(content);
            lines.extend(md);
            lines.push(Line::default());
        }
    } else {
        // Streaming or no reasoning: render all messages as before
        for (role, content) in &app.chat_history {
            match role.as_str() {
                "user" => {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "You: ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(content.clone()),
                    ]));
                    lines.push(Line::default());
                }
                "assistant" => {
                    let md = render_full(content);
                    lines.extend(md);
                    lines.push(Line::default());
                }
                _ => {
                    lines.push(Line::from(format!("{}: {}", role, content)));
                    lines.push(Line::default());
                }
            }
        }

        // Show reasoning during streaming with blockquote style
        let show_reasoning_anywhere = app.config.agent.thinking_display != "hidden"
            && app.is_streaming
            && (!app.streaming_reasoning.is_empty() || !app.last_reasoning.is_empty());
        if show_reasoning_anywhere {
            let reasoning_text = if !app.streaming_reasoning.is_empty() {
                Some(app.streaming_reasoning.as_str())
            } else {
                Some(app.last_reasoning.as_str())
            };
            if let Some(text) = reasoning_text {
                lines.push(Line::from(Span::styled(
                    "💭 Thinking:",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                for line in text.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(line, Style::default().fg(Color::DarkGray)),
                    ]));
                }
                lines.push(Line::default());
            }
        }
    }

    if app.is_streaming {
        // Always show "Assistant:" label when there's content or tool calls
        if !app.streaming_text.is_empty() || app.current_tool_call.is_some() {
            lines.push(Line::from(vec![Span::styled(
                "Assistant: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]));
            if !app.streaming_text.is_empty() {
                let md_lines = render_streaming_markdown(&app.streaming_text);
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

    if !app.status_messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        )));
        for msg in &app.status_messages {
            for line in msg.lines() {
                lines.push(Line::from(line));
            }
        }
    }

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
