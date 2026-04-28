use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
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
use my_code_agent::core::streaming::{stream_response, StreamResult};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::tools::create_mcp_tools;
use my_code_agent::ui::render::{ReasoningTracker, get_reasoning_summary};

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
    _interrupt_tx: tokio::sync::broadcast::Sender<()>,
    _reasoning_tracker: ReasoningTracker,
    status_messages: Vec<String>,
    turn_usage_line: Option<String>,
}

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let mut chat_text = String::new();

    if !app.last_reasoning.is_empty() {
        chat_text.push_str(&format!("{}\n\n", get_reasoning_summary(&app.last_reasoning)));
    }

    for (role, content) in &app.chat_history {
        let role_display = match role.as_str() {
            "user" => "**User**",
            "assistant" => "**Assistant**",
            _ => "**Unknown**",
        };
        chat_text.push_str(&format!("{}: {}\n\n", role_display, content));
    }

    if app.is_streaming {
        if !app.current_response.is_empty() {
            chat_text.push_str(&format!("**Assistant**: {}", app.current_response));
        } else {
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
    f.render_widget(paragraph, chunks[0]);
    f.render_widget(&app.input, chunks[1]);

    let mut status = format!(
        "Model: {} | Tokens: {}",
        app.config.llm.model.as_deref().unwrap_or("unknown"),
        app.token_usage.total_tokens(),
    );
    if let Some(ref turn_line) = app.turn_usage_line {
        status.push_str(&format!(" | {}", turn_line));
    }
    if app.is_streaming {
        status.push_str(" | Streaming...");
    } else {
        status.push_str(" | Ready");
    }
    let status_bar = Paragraph::new(status)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(status_bar, chunks[2]);
}

fn process_stream_result(app: &mut App, result: StreamResult) {
    app.is_streaming = false;
    app.current_response = result.full_response.clone();

    if !result.full_response.is_empty() {
        app.chat_history.push(("assistant".to_string(), result.full_response));
    }

    app.last_reasoning = result.last_reasoning;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    // Auto-scroll to bottom after receiving response
    app.scroll = app.total_lines.saturating_sub(10);

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
    execute!(stdout, EnterAlternateScreen)?;
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
        _interrupt_tx: interrupt_tx.clone(),
        _reasoning_tracker: ReasoningTracker::new(),
        status_messages: Vec::new(),
        turn_usage_line: None,
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

                                    let mut ctx_mgr = context_manager.clone();

                                    app.response_rx = Some(response_rx);

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
                                        ).await;

                                        response_tx.send(result).await.ok();
                                    });
                                }
                            }
                        }
                        (KeyCode::PageUp, _) => {
                            app.scroll = app.scroll.saturating_sub(5);
                        }
                        (KeyCode::PageDown, _) => {
                            app.scroll = app.scroll.saturating_add(5);
                        }
                        _ => {
                            app.input.input(key);
                        }
                    }
                }
                Event::Mouse(_) => {}
                _ => {}
            }
        }

        if app.should_exit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

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
