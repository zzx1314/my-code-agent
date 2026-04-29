use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap, Block, Borders, List, ListItem, ListState, Clear},
};
use tui_markdown::from_str;

use crate::app::App;
use crate::ui::terminal;

/// Render the UI
pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // 判断是否有思考内容需要显示
    let has_reasoning = app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming);

    // 布局：状态栏中包含跑马灯
    let chunks = if has_reasoning {
        // 四区域布局：思考区域 + 聊天区域 + 输入区域 + 状态栏（含跑马灯）
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(app.config.agent.thinking_display_height),  // 思考区域高度（可配置）
                Constraint::Min(1),      // 聊天区域
                Constraint::Length(5),   // 输入区域
                Constraint::Length(1),   // 状态栏（包含跑马灯）
            ])
            .split(area)
    } else {
        // 三区域布局（无思考区域）：聊天区域 + 输入区域 + 状态栏（含跑马灯）
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),      // 聊天区域
                Constraint::Length(5),   // 输入区域
                Constraint::Length(1),   // 状态栏（包含跑马灯）
            ])
            .split(area)
    };

    // 渲染思考区域
    if has_reasoning {
        render_reasoning_area(f, app, chunks[0]);
    }

    // 渲染聊天区域
    let chat_chunk_index = if has_reasoning { 1 } else { 0 };
    let input_chunk_index = if has_reasoning { 2 } else { 1 };
    let status_chunk_index = if has_reasoning { 3 } else { 2 };

    render_chat_area(f, app, chunks[chat_chunk_index]);

    f.render_widget(&app.input, chunks[input_chunk_index]);

    // 渲染补全菜单（浮窗在输入框上方）
    if app.show_completion {
        render_completion_menu(f, app, chunks[input_chunk_index]);
    }

    // 渲染模型选择器（浮窗在输入框上方）
    if app.show_model_picker {
        render_model_picker(f, app, chunks[input_chunk_index]);
    }

    // 渲染状态栏（左侧状态信息 + 右侧跑马灯）
    render_status_bar(f, app, chunks[status_chunk_index]);
}

/// Render the reasoning area
fn render_reasoning_area(f: &mut Frame, app: &mut App, area: Rect) {
    let reasoning_text = if app.is_streaming && !app.streaming_reasoning.is_empty() {
        // 流式输出中，显示实时思考内容
        format!("⏳ Thinking...\n\n{}", app.streaming_reasoning)
    } else if !app.last_reasoning.is_empty() {
        // 思考结束，显示完整思考内容
        format!("✓ Thinking complete\n\n{}", app.last_reasoning)
    } else {
        "⏳ Thinking...".to_string()
    };

    let lines: Vec<ratatui::text::Line> = reasoning_text
        .lines()
        .map(|l| ratatui::text::Line::from(l.to_string()))
        .collect();
    
    app.reasoning_total_lines = lines.len() as u16;

    // Auto-scroll reasoning: during streaming always follow, else honor flag
    if app.reasoning_auto_scroll || app.is_streaming {
        // reasoning area: height - 2 borders = content lines
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

/// Render the chat area
fn render_chat_area(f: &mut Frame, app: &mut App, area: Rect) {
    if app.show_banner {
        let banner = terminal::make_startup_text();
        app.total_lines = banner.lines.len() as u16;
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
        if !app.streaming_text.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Assistant: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]));
            let md = from_str(&app.streaming_text);
            lines.extend(md.lines);
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

    // 用列宽计算折行后实际渲染行数，同时适配宽屏/窄屏/中文
    let wrap_width = area.width.max(1) as usize;
    let rendered_lines: u16 = lines.iter().map(|line| {
        let col_width: usize = line.spans.iter()
            .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        if col_width == 0 {
            1 // 空行也占1行
        } else {
            ((col_width + wrap_width - 1) / wrap_width) as u16
        }
    }).sum();

    app.total_lines = rendered_lines;
    app.chat_area_height = area.height;

    if app.auto_scroll {
        if app.total_lines > area.height {
            app.scroll = app.total_lines - area.height;
        } else {
            app.scroll = 0;
        }
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.scroll, 0))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, area);
}

/// Render the status bar with model info and marquee animation
fn render_status_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = Vec::new();
    
    // 左侧：状态信息
    spans.push(Span::styled(
        format!("Model: {}", app.config.llm.model.as_deref().unwrap_or("unknown")),
        Style::default().fg(Color::DarkGray)
    ));
    
    if let Some(ref turn_line) = app.turn_usage_line {
        spans.push(Span::styled(
            format!(" | {}", turn_line),
            Style::default().fg(Color::DarkGray)
        ));
    }
    
    if app.is_streaming {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | Streaming{}", dots),
            Style::default().fg(Color::Yellow)
        ));
    } else {
        spans.push(Span::styled(
            " | Ready",
            Style::default().fg(Color::Green)
        ));
    }
    
    // 计算左侧状态信息的宽度
    
    let status_bar = Paragraph::new(Line::from(spans));
    f.render_widget(status_bar, area);
}

/// Render the completion menu as a floating popup above the input area
fn render_completion_menu(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.completion_items.is_empty() {
        return;
    }

    // 计算补全菜单的尺寸和位置
    let max_visible_items = 10; // 最多显示10项
    let menu_height = (app.completion_items.len().min(max_visible_items) as u16) + 2; // +2 for borders
    let menu_width = 50u16.min(input_area.width); // 最大宽度50，不超过输入框宽度

    // 定位在输入框上方，如果空间不够则放在下方
    let menu_y = if input_area.y >= menu_height {
        // 空间足够，放在上方
        input_area.y - menu_height
    } else {
        // 空间不够，放在下方
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    // 清除背景（创建浮窗效果）
    f.render_widget(Clear, menu_rect);

    // 创建补全项列表
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

    // 创建列表状态，设置选中项
    let mut state = ListState::default();
    state.select(Some(app.completion_selected));

    // 创建列表组件
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

    // 渲染列表
    f.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render the model picker as a floating popup above the input area
fn render_model_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.model_options.is_empty() {
        return;
    }

    // 计算模型选择器的尺寸和位置
    let max_visible_items = 10; // 最多显示10项
    let menu_height = (app.model_options.len().min(max_visible_items) as u16) + 2; // +2 for borders
    let menu_width = 60u16.min(input_area.width); // 最大宽度60，显示完整模型名

    // 定位在输入框上方，如果空间不够则放在下方
    let menu_y = if input_area.y >= menu_height {
        // 空间足够，放在上方
        input_area.y - menu_height
    } else {
        // 空间不够，放在下方
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    // 清除背景（创建浮窗效果）
    f.render_widget(Clear, menu_rect);

    // 创建模型列表项
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

    // 创建列表状态，设置选中项
    let mut state = ListState::default();
    state.select(Some(app.model_selected));

    // 创建列表组件
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Model (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black))
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    // 渲染列表
    f.render_stateful_widget(list, menu_rect, &mut state);
}
