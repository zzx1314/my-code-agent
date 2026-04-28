use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap, Block, Borders},
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
    let mut chat_text = String::new();

    // Show startup banner if no messages yet
    if app.show_banner {
        // Banner 单独渲染，不走 markdown，避免 ANSI 码乱码和 ASCII art 变形
        let banner = terminal::make_startup_text();
        app.total_lines = banner.lines.len() as u16;
        let paragraph = Paragraph::new(banner)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(paragraph, area);
    } else {
        for (role, content) in &app.chat_history {
            match role.as_str() {
                "user" => {
                    chat_text.push_str(&format!("**You**: {}\n\n", content));
                }
                "assistant" => {
                    chat_text.push_str(content);
                    chat_text.push_str("\n\n---\n\n");
                }
                _ => {
                    chat_text.push_str(&format!("**{}**: {}\n\n", role, content));
                }
            }
        }

        if app.is_streaming {
            if !app.streaming_text.is_empty() {
                chat_text.push_str("**Assistant**: ");
                chat_text.push_str(&app.streaming_text);
                chat_text.push('\n');
            } else if app.streaming_reasoning.is_empty() && app.last_reasoning.is_empty() {
                chat_text.push_str("*⏳ Generating response...*\n\n");
            }
        }

        if !app.status_messages.is_empty() {
            chat_text.push_str("---\n");
            for msg in &app.status_messages {
                chat_text.push_str(msg);
                chat_text.push('\n');
            }
        }

        let markdown = from_str(&chat_text);
        app.total_lines = markdown.lines.len() as u16;

        if app.auto_scroll {
            let chat_area_height = area.height;
            if app.total_lines > chat_area_height {
                app.scroll = app.total_lines - chat_area_height;
            } else {
                app.scroll = 0;
            }
        }

        let paragraph = Paragraph::new(markdown)
            .scroll((app.scroll, 0))
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(paragraph, area);
    }
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
        spans.push(Span::styled(
            " | Streaming...",
            Style::default().fg(Color::Yellow)
        ));
    } else {
        spans.push(Span::styled(
            " | Ready",
            Style::default().fg(Color::Green)
        ));
    }
    
    // 计算左侧状态信息的宽度
    let left_width: usize = spans.iter().map(|s| s.content.len()).sum();
    
    // 右侧：跑马灯动画（如果正在流式输出）
    if app.is_streaming {
        let total_width = area.width as usize;
        if total_width > left_width + 5 {
            let marquee_width = total_width - left_width - 1; // 跑马灯可用宽度
            if marquee_width > 0 {
                let marquee_text = "⏳ Processing... ";

                // 构建缓冲区：前导空格 + 文本 + 尾部空格，实现循环滚动
                let mut buffer = String::new();
                buffer.push_str(&" ".repeat(marquee_width));  // 前导空格，让文本从右侧进入
                buffer.push_str(marquee_text);
                buffer.push_str(&" ".repeat(marquee_width));  // 尾部空格

                // 根据帧数计算起始位置（每2帧移动1个字符，速度适中）
                let speed = 1u64;
                let mut start = ((app.marquee_frame / 2) * speed) as usize % (buffer.len() - marquee_width);
                // Snap to valid UTF-8 boundary to avoid slicing inside multi-byte chars (e.g., ⏳)
                start = buffer.floor_char_boundary(start);
                let end = (start + marquee_width).min(buffer.len());
                let end = buffer.floor_char_boundary(end);
                let display = &buffer[start..end];

                spans.push(Span::styled(
                    display.to_string(),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                ));
            }
        }
    }
    
    let status_bar = Paragraph::new(Line::from(spans));
    f.render_widget(status_bar, area);
}
