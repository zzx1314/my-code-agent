use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap, Block, Borders, List, ListItem, ListState, Clear},
};
use tui_textarea::TextArea;
use tui_markdown::from_str;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::ui::terminal;

const MIN_INPUT_HEIGHT: u16 = 4;
const MAX_INPUT_HEIGHT: u16 = 14;

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
            if line_idx == cursor_row { cursor_char_idx } else { usize::MAX },
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
            .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) ")
    );
    new_ta.set_cursor_line_style(ratatui::style::Style::default());
    new_ta.move_cursor(tui_textarea::CursorMove::Jump(new_cursor_row as u16, new_cursor_col as u16));
    app.input = new_ta;
}

fn calculate_input_height(app: &App, area_width: u16) -> u16 {
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

pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let text_width = (area.width as usize).saturating_sub(2);

    apply_input_wrap(app, text_width);

    let input_height = calculate_input_height(app, area.width);

    let has_reasoning = app.show_reasoning && (!app.last_reasoning.is_empty() || !app.streaming_reasoning.is_empty());

    let chunks = if has_reasoning {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(app.config.agent.thinking_display_height),
                Constraint::Min(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ])
            .split(area)
    };

    if has_reasoning {
        render_reasoning_area(f, app, chunks[0]);
    }

    let chat_chunk_index = if has_reasoning { 1 } else { 0 };
    let input_chunk_index = if has_reasoning { 2 } else { 1 };
    let status_chunk_index = if has_reasoning { 3 } else { 2 };

    render_chat_area(f, app, chunks[chat_chunk_index]);

    f.render_widget(&app.input, chunks[input_chunk_index]);

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

fn render_reasoning_area(f: &mut Frame, app: &mut App, area: Rect) {
    let reasoning_text = if app.is_streaming && !app.streaming_reasoning.is_empty() {
        format!("⏳ Thinking...\n\n{}", app.streaming_reasoning)
    } else if !app.last_reasoning.is_empty() {
        format!("✓ Thinking complete\n\n{}", app.last_reasoning)
    } else {
        "⏳ Thinking...".to_string()
    };

    // Calculate inner width accounting for borders (Borders::ALL = 2 chars)
    let inner_width = area.width.saturating_sub(2);

    // Use Paragraph::line_count to get actual visual lines after wrapping
    let temp_paragraph = Paragraph::new(reasoning_text.clone())
        .wrap(Wrap { trim: true });
    app.reasoning_total_lines = temp_paragraph.line_count(inner_width) as u16;

    if app.reasoning_auto_scroll || app.is_streaming {
        let visible_lines = app.config.agent.thinking_display_height.saturating_sub(2);
        if app.reasoning_total_lines > visible_lines {
            app.reasoning_scroll = app.reasoning_total_lines - visible_lines;
        } else {
            app.reasoning_scroll = 0;
        }
    }

    let reasoning_paragraph = Paragraph::new(reasoning_text)
        .scroll((app.reasoning_scroll, 0))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 🤔 Reasoning ")
                .border_style(Style::default().fg(Color::Yellow))
        )
        .style(Style::default().fg(Color::DarkGray));

    f.render_widget(reasoning_paragraph, area);
}

fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
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

    for (role, content) in &app.chat_history {
        match role.as_str() {
            "user" => {
                lines.push(Line::from(vec![
                    Span::styled("You: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(content.clone()),
                ]));
                lines.push(Line::default());
            }
            "assistant" => {
                let md = from_str(content);
                lines.extend(md.lines);
                lines.push(Line::default());
            }
            _ => {
                lines.push(Line::from(format!("{}: {}", role, content)));
                lines.push(Line::default());
            }
        }
    }

    if app.is_streaming {
        if !app.streaming_text.is_empty() || app.current_tool_call.is_some() {
            lines.push(Line::from(vec![
                Span::styled("Assistant: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]));
            if !app.streaming_text.is_empty() {
                let md = from_str(&app.streaming_text);
                lines.extend(md.lines);
            }
            // Show running indicator only when a tool is actively executing
            // (the tool call name is already rendered in streaming_text above)
            if let Some(ref tool_name) = app.current_tool_call {
                lines.push(Line::from(vec![
                    Span::styled(format!("  ⏳ {}...", tool_name), Style::default().fg(Color::DarkGray)),
                ]));
            }
        } else if app.streaming_reasoning.is_empty() && app.last_reasoning.is_empty() {
            lines.push(Line::from(
                Span::styled("⏳ Generating response...", Style::default().fg(Color::Yellow))
            ));
        }
    }

    if !app.status_messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        )));
        for msg in &app.status_messages {
            lines.push(Line::from(msg.as_str()));
        }
    }

    if !app.streaming_status_messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        )));
        for msg in &app.streaming_status_messages {
            lines.push(Line::from(msg.as_str()));
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

fn render_status_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = Vec::new();

    spans.push(Span::styled(
        format!("Model: {}", app.config.llm.model.as_deref().unwrap_or("unknown")),
        Style::default().fg(Color::DarkGray)
    ));

    if let Some(ref turn_line) = app.turn_usage_line {
        spans.push(Span::styled(
            format!(" | {}", turn_line),
            Style::default().fg(Color::DarkGray)
        ));
        // Show per-turn cache hit rate if available
        if let Some(cache_line) = crate::core::context_cache::global_cache().format_turn_cache_line() {
            spans.push(Span::styled(
                format!(" | {}", cache_line),
                Style::default().fg(Color::DarkGray)
            ));
        }
    }

    if app.is_streaming {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | Streaming{}", dots),
            Style::default().fg(Color::Yellow)
        ));
    } else if app.shell_mode {
        spans.push(Span::styled(
            " | 🐚 Shell",
            Style::default().fg(Color::Cyan)
        ));
    } else {
        spans.push(Span::styled(
            " | Ready",
            Style::default().fg(Color::Green)
        ));
    }

    let status_bar = Paragraph::new(Line::from(spans));
    f.render_widget(status_bar, area);
}

fn render_completion_menu(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.completion_items.is_empty() {
        return;
    }

    let max_visible_items = 10;
    let menu_height = (app.completion_items.len().min(max_visible_items) as u16) + 2;
    let menu_width = 50u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app.completion_items
        .iter()
        .map(|item| {
            let style = if item == &app.completion_items[app.completion_selected] {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(item.as_str()).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.completion_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(match app.completion_type {
                    Some('/') => " Commands ",
                    Some('@') => " Files ",
                    _ => " Completions ",
                })
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black))
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render a confirmation dialog overlay for dangerous operations.
fn render_confirmation_dialog(f: &mut Frame, app: &mut App) {
    let Some(pending) = &app.pending_confirmation else {
        return;
    };

    let area = f.area();

    // Dialog dimensions
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let detail_lines = pending.detail.lines().count() as u16;
    let dialog_height = (detail_lines + 6).min(area.height.saturating_sub(4));

    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_rect = Rect {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
    };

    // Dim the background
    let overlay = Rect { x: 0, y: 0, width: area.width, height: area.height };
    f.render_widget(Clear, overlay);

    // Dialog block
    let block = Block::default()
        .title(" ⚠️  Confirmation Required ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black).fg(Color::White));

    let inner = block.inner(dialog_rect);
    f.render_widget(block, dialog_rect);

    // Render the reason (bold, yellow)
    let reason_text = format!("\n{}\n", &pending.reason);
    let reason_paragraph = Paragraph::new(reason_text)
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(reason_paragraph, Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 2,
    });

    // Render the detail
    let detail_paragraph = Paragraph::new(pending.detail.as_str())
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));
    f.render_widget(detail_paragraph, Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(5),
    });

    // Render prompt at the bottom
    let prompt_text = "  [Y] Yes   [N] No   [Esc] Cancel";
    let prompt = Paragraph::new(prompt_text)
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(prompt, Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 1,
    });
}

fn render_model_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.model_options.is_empty() {
        return;
    }

    let max_visible_items = 10;
    let menu_height = (app.model_options.len().min(max_visible_items) as u16) + 2;
    let menu_width = 60u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app.model_options
        .iter()
        .enumerate()
        .map(|(idx, model)| {
            let prefix = if idx == app.model_selected { "▶ " } else { "  " };
            let style = if idx == app.model_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            ListItem::new(format!("{}{}", prefix, model)).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.model_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Model (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black))
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, menu_rect, &mut state);
}

fn render_provider_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.provider_options.is_empty() {
        return;
    }

    let menu_height = (app.provider_options.len() as u16) + 2;
    let menu_width = 30u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app.provider_options
        .iter()
        .enumerate()
        .map(|(idx, provider)| {
            let prefix = if idx == app.provider_selected { "▶ " } else { "  " };
            let style = if idx == app.provider_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            ListItem::new(format!("{}{}", prefix, provider)).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.provider_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Provider (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Green))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, menu_rect, &mut state);
}

fn render_session_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.session_options.is_empty() {
        return;
    }

    let menu_height = (app.session_options.len() as u16) + 2;
    let menu_width = 50u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app.session_options
        .iter()
        .enumerate()
        .map(|(idx, session)| {
            let prefix = if idx == app.session_selected { "▶ " } else { "  " };
            let style = if idx == app.session_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let display_text = format!("{}{} ({} turns, {} tokens)", 
                prefix, session.name, session.turns, session.tokens);
            ListItem::new(display_text).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.session_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Session (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, menu_rect, &mut state);
}

