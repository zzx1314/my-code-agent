use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode, disable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap, Block, Borders},
};
use tui_textarea::TextArea;
use tui_markdown::from_str;
use tokio::sync::mpsc;
use std::time::Duration;
use std::sync::Arc;

use my_code_agent::core::config::Config;
use my_code_agent::core::context::expand_file_refs;
use my_code_agent::core::context_manager::ContextManager;
use my_code_agent::core::preamble::build_agent;
use my_code_agent::core::session::SessionData;
use my_code_agent::core::streaming::{stream_response, StreamResult, StreamEvent};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::tools::create_mcp_tools;
use my_code_agent::ui::render::ReasoningTracker;

use rig::completion::Message as RigMessage;
use rig::message::UserContent;
use rig::completion::AssistantContent;

fn text_from_user_content(content: &UserContent) -> String {
    match content {
        UserContent::Text(s) => s.text.clone(),
        _ => String::new(),
    }
}

fn text_from_assistant_content(content: &AssistantContent) -> String {
    match content {
        AssistantContent::Text(s) => s.text.clone(),
        _ => String::new(),
    }
}

fn convert_rig_to_app(msg: RigMessage) -> (String, String) {
    match msg {
        RigMessage::User { content } => {
            let text = text_from_user_content(&content.first());
            ("user".to_string(), text)
        }
        RigMessage::Assistant { content, .. } => {
            let text = text_from_assistant_content(&content.first());
            ("assistant".to_string(), text)
        }
        _ => ("unknown".to_string(), String::new()),
    }
}

fn convert_app_to_rig(chat_history: &[(String, String)]) -> Vec<RigMessage> {
    chat_history
        .iter()
        .map(|(role, content)| match role.as_str() {
            "user" => RigMessage::user(content),
            "assistant" => RigMessage::assistant(content),
            _ => RigMessage::user(""),
        })
        .collect()
}

struct App {
    chat_history: Vec<(String, String)>,
    current_response: String,
    input: TextArea<'static>,
    scroll: u16,
    total_lines: u16,
    token_usage: TokenUsage,
    _session_name: Option<String>,
    last_reasoning: String,
    config: Config,
    should_exit: bool,
    is_streaming: bool,
    response_rx: Option<mpsc::Receiver<StreamResult>>,
    streaming_events_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    streaming_text: String,
    streaming_reasoning: String,
    _interrupt_tx: tokio::sync::broadcast::Sender<()>,
    _reasoning_tracker: ReasoningTracker,
    status_messages: Vec<String>,
    turn_usage_line: Option<String>,
    /// 是否显示思考区域
    show_reasoning: bool,
    /// 思考区域的滚动位置
    reasoning_scroll: u16,
    /// 思考区域的总行数
    reasoning_total_lines: u16,
}

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // 判断是否有思考内容需要显示
    let has_reasoning = app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming);
    
    let chunks = if has_reasoning {
        // 四区域布局：思考区域 + 聊天区域 + 输入区域 + 状态栏
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12),  // 思考区域（固定12行，包括边框）
                Constraint::Min(1),      // 聊天区域
                Constraint::Length(5),   // 输入区域
                Constraint::Length(1),   // 状态栏
            ])
            .split(area)
    } else {
        // 三区域布局（无思考区域）
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(5),
                Constraint::Length(1),
            ])
            .split(area)
    };

    // 渲染思考区域
    if has_reasoning {
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
        
        f.render_widget(reasoning_paragraph, chunks[0]);
    }

    // 渲染聊天区域
    let chat_chunk_index = if has_reasoning { 1 } else { 0 };
    let input_chunk_index = if has_reasoning { 2 } else { 1 };
    let status_chunk_index = if has_reasoning { 3 } else { 2 };

    let mut chat_text = String::new();

    for (role, content) in &app.chat_history {
        let role_display = match role.as_str() {
            "user" => "**User**",
            "assistant" => "**Assistant**",
            _ => "**Unknown**",
        };
        chat_text.push_str(&format!("{}: {}\n\n", role_display, content));
    }

    if app.is_streaming {
        // During streaming: show plain streaming text
        if !app.streaming_text.is_empty() {
            chat_text.push_str("**Assistant**: ");
            chat_text.push_str(&app.streaming_text);
            chat_text.push('\n');
        } else if app.streaming_reasoning.is_empty() && app.last_reasoning.is_empty() {
            chat_text.push_str("*⏳ Generating response...*\n\n");
        }
    }

    // Status messages (tool calls, warnings, plan progress)
    if !app.status_messages.is_empty() {
        chat_text.push_str("---\n");
        for msg in &app.status_messages {
            chat_text.push_str(&msg);
            chat_text.push('\n');
        }
    }

    let markdown = from_str(&chat_text);
    let line_count = markdown.lines.len() as u16;
    app.total_lines = line_count;
    let lines: Vec<ratatui::text::Line> = markdown.lines;
    let text = ratatui::text::Text::from(lines);
    let paragraph = Paragraph::new(text)
        .scroll((app.scroll, 0))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, chunks[chat_chunk_index]);
    
    f.render_widget(&app.input, chunks[input_chunk_index]);

    let scroll_info = if app.total_lines > 0 {
        let pct = (app.scroll as u64 * 100 / app.total_lines as u64).min(100);
        format!(" | Scroll: {}/{} ({}%)", app.scroll, app.total_lines, pct)
    } else {
        String::new()
    };
    let mut status = format!(
        "Model: {} | Tokens: {}{}",
        app.config.llm.model.as_deref().unwrap_or("unknown"),
        app.token_usage.total_tokens(),
        scroll_info,
    );
    if let Some(ref turn_line) = app.turn_usage_line {
        status.push_str(&format!(" | {}", turn_line));
    }
    if app.is_streaming {
        status.push_str(" | Streaming...");
    } else {
        status.push_str(" | Ready");
    }
    
    // 添加思考区域控制提示
    if !app.last_reasoning.is_empty() || app.is_streaming {
        status.push_str(" | Ctrl+R: toggle reasoning");
    }
    
    let status_bar = Paragraph::new(status)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(status_bar, chunks[status_chunk_index]);
}

fn process_stream_result(app: &mut App, result: StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.streaming_events_rx = None;

    if !result.full_response.is_empty() {
        app.chat_history.push(("assistant".to_string(), result.full_response));
    }

    app.last_reasoning = result.last_reasoning;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    // Auto-scroll to bottom after receiving response (show last ~10 visible lines)
    let max_scroll = app.total_lines.saturating_sub(1);
    app.scroll = app.total_lines.saturating_sub(10).min(max_scroll);

    if result.should_exit {
        app.should_exit = true;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let config = Config::load();

    let mut app_chat_history: Vec<(String, String)> = Vec::new();
    let mut token_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

    if let Some(load_result) = SessionData::load_default(config.session.save_file.as_deref()) {
        if let Ok(data) = load_result {
            app_chat_history = data.chat_history.into_iter().map(convert_rig_to_app).collect();
            token_usage = data.token_usage;
            last_reasoning = data.last_reasoning;
            let turns = app_chat_history.iter().filter(|(r, _)| r == "user").count();
            eprintln!("[info] Resumed session ({} turns, {} tokens)", turns, token_usage.total_tokens());
        }
    }

    let mcp_tools = create_mcp_tools(&config).await;
    let agent = Arc::new(build_agent(&config, mcp_tools));

    let context_manager = ContextManager::new(&config);

    let (interrupt_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut input_area = TextArea::default();
    input_area.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
    );
    input_area.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));

    let mut app = App {
        chat_history: app_chat_history,
        current_response: String::new(),
        input: input_area,
        scroll: 0,
        total_lines: 0,
        token_usage,
        _session_name: None,
        last_reasoning,
        config: config.clone(),
        should_exit: false,
        is_streaming: false,
        response_rx: None,
        streaming_events_rx: None,
        streaming_text: String::new(),
        streaming_reasoning: String::new(),
        _interrupt_tx: interrupt_tx.clone(),
        _reasoning_tracker: ReasoningTracker::new(),
        status_messages: Vec::new(),
        turn_usage_line: None,
        show_reasoning: true,  // 默认显示思考区域
        reasoning_scroll: 0,
        reasoning_total_lines: 0,
    };

    // Ctrl+C handler sends interrupt on broadcast channel
    let interrupt_tx_ctrlc = interrupt_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx_ctrlc.send(()).ok();
        }
    });

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        // Check for completed stream result
        if let Some(ref mut rx) = app.response_rx {
            if let Ok(result) = rx.try_recv() {
                process_stream_result(&mut app, result);
                app.response_rx = None;
            }
        }

        // Poll streaming text events for live display
        if let Some(ref mut rx) = app.streaming_events_rx {
            loop {
                match rx.try_recv() {
                    Ok(StreamEvent::Text(delta)) => {
                        app.streaming_text.push_str(&delta);
                    }
                    Ok(StreamEvent::ToolCall(name)) => {
                        app.streaming_text.push_str(&format!("\n⟳ [{}]\n", name));
                    }
                    Ok(StreamEvent::ReasoningActive(active)) => {
                        if !active {
                            // 思考结束，保存内容到 last_reasoning
                            if !app.streaming_reasoning.is_empty() {
                                app.last_reasoning = app.streaming_reasoning.clone();
                                app.streaming_reasoning.clear();
                            }
                        }
                    }
                    Ok(StreamEvent::ReasoningDelta(delta)) => {
                        app.streaming_reasoning.push_str(&delta);
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        app.streaming_events_rx = None;
                        break;
                    }
                }
            }
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+C when streaming: handled by the broadcast interrupt
                            if !app.is_streaming {
                                app.should_exit = true;
                            }
                        }
                        (KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+R: toggle reasoning display
                            app.show_reasoning = !app.show_reasoning;
                        }
                        (KeyCode::Esc, _) => {
                            if !app.is_streaming {
                                app.should_exit = true;
                            }
                        }
                        (KeyCode::Enter, modifiers) => {
                            if modifiers.contains(KeyModifiers::SHIFT) {
                                // Shift+Enter: insert newline in textarea
                                app.input.input(key);
                            } else {
                                // Plain Enter or Ctrl+Enter: send
                                let input_text = app.input.lines().join("\n").trim().to_string();
                                if !input_text.is_empty() && !app.is_streaming {
                                    app.chat_history.push(("user".to_string(), input_text.clone()));
                                    app.input = {
                                        let mut ta = TextArea::default();
                                        ta.set_block(
                                            Block::default()
                                                .borders(Borders::ALL)
                                                .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
                                        );
                                        ta.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
                                        ta
                                    };
                                    app.is_streaming = true;
                                    app.streaming_text.clear();
                                    app.streaming_reasoning.clear();
                                    app.current_response.clear();
                                    app.status_messages.clear();
                                    app.turn_usage_line = None;

                                    let expanded = expand_file_refs(&input_text, &app.config);
                                    
                                    let mut rig_chat_history = convert_app_to_rig(&app.chat_history);
                                    let agent_clone = agent.clone();
                                    let config_clone = app.config.clone();
                                    let mut token_usage_clone = app.token_usage.clone();
                                    let interrupt_rx = interrupt_tx.subscribe();
                                    let (response_tx, response_rx) = mpsc::channel::<StreamResult>(1);
                                    let (event_tx, event_rx) = mpsc::unbounded_channel::<StreamEvent>();

                                    let mut ctx_mgr = context_manager.clone();

                                    app.response_rx = Some(response_rx);
                                    app.streaming_events_rx = Some(event_rx);

                                    tokio::spawn(async move {
                                        let mut interrupt_rx = interrupt_rx;

                                        let result = stream_response(
                                            &agent_clone,
                                            &expanded.expanded,
                                            &mut rig_chat_history,
                                            &mut token_usage_clone,
                                            &mut interrupt_rx,
                                            &mut ctx_mgr,
                                            &config_clone.agent,
                                            Some(event_tx),
                                        ).await;

                                        response_tx.send(result).await.ok();
                                    });
                                }
                            }
                        }
                        (KeyCode::PageUp, _) => {
                            if app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming) {
                                app.reasoning_scroll = app.reasoning_scroll.saturating_sub(3);
                            } else {
                                app.scroll = app.scroll.saturating_sub(3);
                            }
                        }
                        (KeyCode::PageDown, _) => {
                            if app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming) {
                                let max_scroll = app.reasoning_total_lines.saturating_sub(1);
                                app.reasoning_scroll = (app.reasoning_scroll + 3).min(max_scroll);
                            } else {
                                let max_scroll = app.total_lines.saturating_sub(1);
                                app.scroll = (app.scroll + 3).min(max_scroll);
                            }
                        }
                        (KeyCode::Up, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            if app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming) {
                                app.reasoning_scroll = app.reasoning_scroll.saturating_sub(3);
                            } else {
                                app.scroll = app.scroll.saturating_sub(3);
                            }
                        }
                        (KeyCode::Down, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            if app.show_reasoning && (!app.last_reasoning.is_empty() || app.is_streaming) {
                                let max_scroll = app.reasoning_total_lines.saturating_sub(1);
                                app.reasoning_scroll = (app.reasoning_scroll + 3).min(max_scroll);
                            } else {
                                let max_scroll = app.total_lines.saturating_sub(1);
                                app.scroll = (app.scroll + 3).min(max_scroll);
                            }
                        }
                        _ => {
                            app.input.input(key);
                        }
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll = app.scroll.saturating_sub(3);
                    }
                    MouseEventKind::ScrollDown => {
                        let max_scroll = app.total_lines.saturating_sub(1);
                        app.scroll = (app.scroll + 3).min(max_scroll);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if app.should_exit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;

    if !app.chat_history.is_empty() {
        let data = SessionData::new(
            app.chat_history.into_iter().map(|(r, c)| match r.as_str() {
                "user" => RigMessage::user(c),
                "assistant" => RigMessage::assistant(c),
                _ => RigMessage::user(c),
            }).collect(),
            app.token_usage.clone(),
            app.last_reasoning.clone(),
        );
        if let Err(e) = data.save_default(config.session.save_file.as_deref()) {
            eprintln!("Failed to save session: {}", e);
        }
    }

    Ok(())
}
