use crossterm::{
    event::{self, KeyCode, KeyModifiers, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Write as _;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::app::App;
use crate::app::conversion::convert_app_to_rig;
use crate::core::context::expand_file_refs;
use crate::core::context_manager::ContextManager;
use crate::core::streaming::{stream_response, StreamResult, StreamEvent};

/// Handle key events
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, context_manager: &mut ContextManager) {
    // 如果补全菜单正在显示，优先处理补全相关按键
    if app.show_completion {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                // 向下选择补全项
                if !app.completion_items.is_empty() {
                    app.completion_selected = (app.completion_selected + 1) % app.completion_items.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                // 向上选择补全项
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                // 确认补全
                apply_completion(app);
                return;
            }
            KeyCode::Esc => {
                // 取消补全
                hide_completion(app);
                return;
            }
            KeyCode::Char(c) => {
                // 输入字符，更新补全查询
                if c == '/' && app.completion_type != Some('/') {
                    // 切换到命令补全
                    hide_completion(app);
                    trigger_completion(app, '/');
                    return;
                } else if c == '@' && app.completion_type != Some('@') {
                    // 切换到文件补全
                    hide_completion(app);
                    trigger_completion(app, '@');
                    return;
                }
                // 其他字符，让输入框处理，然后更新补全
            }
            KeyCode::Backspace => {
                // 退格键，检查是否需要隐藏补全
            }
            _ => {}
        }
    }

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
            if app.show_completion {
                hide_completion(app);
            } else if !app.is_streaming {
                app.should_exit = true;
            }
        }
        (KeyCode::Enter, modifiers) => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                // Shift+Enter: insert newline in textarea
                app.input.input(key);
            } else {
                // Plain Enter or Ctrl+Enter: send
                if app.show_completion {
                    apply_completion(app);
                } else {
                    handle_enter_key(app, context_manager);
                }
            }
        }
        (KeyCode::PageUp, _) => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        (KeyCode::PageDown, _) => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            app.auto_scroll = false;
        }
        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                // 向上选择补全项
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
            } else {
                app.scroll = app.scroll.saturating_sub(3);
                app.auto_scroll = false;
            }
        }
        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                // 向下选择补全项
                if !app.completion_items.is_empty() {
                    app.completion_selected = (app.completion_selected + 1) % app.completion_items.len();
                }
            } else {
                let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
                app.scroll = (app.scroll + 3).min(max_scroll);
                if app.scroll >= max_scroll {
                    app.auto_scroll = true;
                }
            }
        }
        (KeyCode::Char(c), _) => {
            // 检查是否触发补全
            if c == '/' || c == '@' {
                app.input.input(key);
                trigger_completion(app, c);
            } else {
                app.input.input(key);
                // 如果补全菜单正在显示，更新过滤
                if app.show_completion {
                    update_completion_query(app);
                }
            }
        }
        (KeyCode::Backspace, _) => {
            app.input.input(key);
            // 检查是否需要隐藏或更新补全
            if app.show_completion {
                let cursor_pos = get_cursor_position(app);
                // 如果光标前的字符不是 '/' 或 '@'，或者光标在触发位置之前，隐藏补全
                if cursor_pos == 0 || (cursor_pos <= app.completion_trigger_pos) {
                    hide_completion(app);
                } else {
                    update_completion_query(app);
                }
            }
        }
        _ => {
            app.input.input(key);
        }
    }
}

/// Handle Enter key press (send message)
fn handle_enter_key(app: &mut App, context_manager: &mut ContextManager) {
    let input_text = app.input.lines().join("\n").trim().to_string();
    if !input_text.is_empty() && !app.is_streaming {
        // Check if it's a command (starts with /)
        if input_text.starts_with('/') {
            // Handle commands locally without sending to LLM
            if handle_command(app, &input_text) {
                // Clear input after command is handled
                app.input = {
                    let mut ta = TextArea::default();
                    ta.set_block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
                    );
                    ta.set_cursor_line_style(ratatui::style::Style::default());
                    ta
                };
                return; // Command was handled, don't send to LLM
            }
        }

        app.show_banner = false; // Hide startup banner
        app.chat_history.push(("user".to_string(), input_text.clone()));
        app.input = {
            let mut ta = TextArea::default();
            ta.set_block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
            );
            ta.set_cursor_line_style(ratatui::style::Style::default());
            ta
        };
        app.is_streaming = true;
        app.auto_scroll = true;
        app.reasoning_auto_scroll = true;
        app.reasoning_scroll = 0;
        app.streaming_text.clear();
        app.streaming_reasoning.clear();
        app.current_response.clear();
        app.status_messages.clear();
        app.turn_usage_line = None;

        let expanded = expand_file_refs(&input_text, &app.config);
        
        let mut rig_chat_history = convert_app_to_rig(&app.chat_history);
        let agent_clone = app.agent.clone();
        let config_clone = app.config.clone();
        let mut token_usage_clone = app.token_usage.clone();
        let interrupt_rx = app.interrupt_tx.subscribe();
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

/// Handle mouse events
pub fn handle_mouse_event(mouse: event::MouseEvent, app: &mut App) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            // 滚到底部时重新启用 auto_scroll
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {} // 其他鼠标事件忽略，不影响文本选中
    }
}

/// Process streaming events
pub fn process_streaming_events(app: &mut App) {
    // Poll streaming text events for live display
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(StreamEvent::Text(delta)) => {
                    app.streaming_text.push_str(&delta);
                }
                Ok(StreamEvent::ToolCall(name)) => {
                    app.streaming_text.push_str(&format!("\n⟳ [`{}`]\n", name));
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
}

/// Check for completed stream result
pub fn check_stream_result(app: &mut App) {
    if let Some(ref mut rx) = app.response_rx {
        if let Ok(result) = rx.try_recv() {
            process_stream_result(app, result);
            app.response_rx = None;
        }
    }
}

/// Process stream result
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
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
    }
    app.status_messages.clear();
}

/// Enter alternate screen and enable raw mode
pub fn enter_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Enable alternate scroll mode (DECSET 1007): mouse wheel sends arrow keys
    // without requiring mouse capture, so text selection still works.
    let _ = write!(std::io::stdout(), "\x1b[?1007h");
    let _ = std::io::stdout().flush();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
    let _ = write!(std::io::stdout(), "\x1b[?1007l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

// ========== 补全菜单相关函数 ==========

/// 触发补全菜单
fn trigger_completion(app: &mut App, trigger_char: char) {
    app.show_completion = true;
    app.completion_type = Some(trigger_char);
    app.completion_selected = 0;
    app.completion_trigger_pos = get_cursor_position(app);
    app.completion_query = String::new();
    
    // 获取补全项
    app.completion_items = get_completion_items(trigger_char);
}

/// 隐藏补全菜单
fn hide_completion(app: &mut App) {
    app.show_completion = false;
    app.completion_items.clear();
    app.completion_selected = 0;
    app.completion_type = None;
    app.completion_query.clear();
    app.completion_trigger_pos = 0;
}

/// 将字符索引转换为字节索引（处理多字节字符）
fn char_idx_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// 应用选中的补全
fn apply_completion(app: &mut App) {
    if app.completion_items.is_empty() {
        hide_completion(app);
        return;
    }
    
    let selected = app.completion_items[app.completion_selected].clone();
    let trigger_char = match app.completion_type {
        Some(c) => c,
        None => {
            hide_completion(app);
            return;
        }
    };
    
    // 获取当前输入文本
    let mut lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();
    let cursor = app.input.cursor();
    
    // 找到当前行
    if cursor.0 < lines.len() {
        let line = &mut lines[cursor.0];
        let pos = char_idx_to_byte(line, cursor.1);
        
        // 找到触发字符的位置（从光标位置向前找）
        let trigger_pos = line[..pos].rfind(trigger_char).unwrap_or(pos.saturating_sub(1));
        
        // 替换从触发位置到光标位置的内容
        let new_line = format!("{}{}{}", &line[..trigger_pos], selected, &line[pos..]);
        lines[cursor.0] = new_line;
    }
    
    let new_text = lines.join("\n");
    let mut new_input = TextArea::from(new_text.lines());
    new_input.set_block(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
    );
    new_input.set_cursor_line_style(ratatui::style::Style::default());
    app.input = new_input;
    
    // 设置光标位置到补全项末尾
    let completion_len = selected.len();
    let cursor = app.input.cursor();
    let new_cursor_col = app.completion_trigger_pos + completion_len;
    app.input.move_cursor(tui_textarea::CursorMove::Jump(cursor.0 as u16, new_cursor_col as u16));
    
    hide_completion(app);
}

/// 更新补全查询字符串（用于过滤）
fn update_completion_query(app: &mut App) {
    let cursor_pos = get_cursor_position(app);
    if cursor_pos <= app.completion_trigger_pos {
        hide_completion(app);
        return;
    }
    
    // 获取触发位置到光标位置的文本作为查询字符串
    let lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();
    let cursor = app.input.cursor();
    
    if cursor.0 < lines.len() {
        let line = &lines[cursor.0];
        let byte_start = char_idx_to_byte(line, app.completion_trigger_pos);
        let byte_end = char_idx_to_byte(line, cursor_pos);
        if byte_start <= byte_end && byte_end <= line.len() {
            app.completion_query = line[byte_start..byte_end].to_string();
        }
    }
    
    // 过滤补全项
    if let Some(trigger_char) = app.completion_type {
        let all_items = get_completion_items(trigger_char);
        if app.completion_query.is_empty() {
            app.completion_items = all_items;
        } else {
            app.completion_items = all_items
                .into_iter()
                .filter(|item| item.to_lowercase().contains(&app.completion_query.to_lowercase()))
                .collect();
        }
        app.completion_selected = 0;
    }
}

/// 获取当前光标位置（字符偏移量）
fn get_cursor_position(app: &App) -> usize {
    let cursor = app.input.cursor();
    cursor.1
}

/// 获取补全项列表
fn get_completion_items(trigger_char: char) -> Vec<String> {
    match trigger_char {
        '/' => {
            // 命令补全
            vec![
                "/help".to_string(),
                "/quit".to_string(),
                "/clear".to_string(),
                "/save".to_string(),
                "/load".to_string(),
                "/status".to_string(),
                "/tokens".to_string(),
                "/reasoning".to_string(),
            ]
        }
        '@' => {
            // 文件补全 - 使用 glob 获取当前目录下的文件
            use glob::glob;
            let mut files = Vec::new();
            
            // 获取当前目录下的所有文件（递归深度2）
            if let Ok(entries) = glob("**/*") {
                for entry in entries.flatten() {
                    if let Some(path_str) = entry.to_str() {
                        // 跳过隐藏文件和目录
                        if !path_str.starts_with('.') && !path_str.contains("/.") {
                            files.push(format!("@{}", path_str));
                        }
                    }
                }
            }
            
            // 如果没有找到文件，添加一些示例
            if files.is_empty() {
                files.push("@src/main.rs".to_string());
                files.push("@src/lib.rs".to_string());
                files.push("@Cargo.toml".to_string());
                files.push("@README.md".to_string());
            }
            
            files.sort();
            files.dedup();
            files
        }
        _ => Vec::new(),
    }
}

/// 处理命令（以 / 开头的输入）
/// 返回 true 表示命令已处理，false 表示需要发送给 LLM
fn handle_command(app: &mut App, input: &str) -> bool {
    let command = input.trim().to_lowercase();
    
    match command.as_str() {
        "/help" => {
            let help_text = generate_help_text();
            app.chat_history.push(("user".to_string(), "/help".to_string()));
            app.chat_history.push(("assistant".to_string(), help_text));
            app.show_banner = false;
            app.auto_scroll = true;
            app.scroll = u16::MAX;
            true
        }
        "/quit" => {
            app.should_exit = true;
            true
        }
        "/clear" => {
            app.chat_history.clear();
            app.token_usage = crate::core::token_usage::TokenUsage::with_config(&app.config);
            app.show_banner = true;
            app.auto_scroll = true;
            app.scroll = 0;
            // 删除会话文件
            if app.config.session.enabled {
                if let Some(save_file) = &app.config.session.save_file {
                    let _ = std::fs::remove_file(save_file);
                }
            }
            true
        }
        "/save" => {
            // 保存会话（会通过主循环自动处理）
            app.chat_history.push(("user".to_string(), "/save".to_string()));
            app.chat_history.push(("assistant".to_string(), "Session will be saved on exit. Use /quit to exit and save.".to_string()));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/load" => {
            app.chat_history.push(("user".to_string(), "/load".to_string()));
            app.chat_history.push(("assistant".to_string(), "Session auto-resumes on startup if session.enabled is true in config.toml".to_string()));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/status" => {
            let status = format!("Session enabled: {}\nModel: {}\nProvider: {}\nTotal tokens used: {}", 
                app.config.session.enabled,
                app.config.llm.model.as_deref().unwrap_or("default"),
                app.config.llm.provider,
                app.token_usage.total_tokens());
            app.chat_history.push(("user".to_string(), "/status".to_string()));
            app.chat_history.push(("assistant".to_string(), status));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/tokens" => {
            let token_info = format!("Total tokens used: {}\nInput tokens: {}\nOutput tokens: {}", 
                app.token_usage.total_tokens(),
                app.token_usage.input_tokens(),
                app.token_usage.output_tokens());
            app.chat_history.push(("user".to_string(), "/tokens".to_string()));
            app.chat_history.push(("assistant".to_string(), token_info));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/reasoning" => {
            app.show_reasoning = !app.show_reasoning;
            let status = if app.show_reasoning { "Reasoning display enabled" } else { "Reasoning display disabled" };
            app.chat_history.push(("user".to_string(), "/reasoning".to_string()));
            app.chat_history.push(("assistant".to_string(), status.to_string()));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        _ => {
            // 未知命令，发送给 LLM 处理
            false
        }
    }
}

/// 生成帮助文本
fn generate_help_text() -> String {
    let help = r#"# My Code Agent - Command Help

## Available Commands

| Command | Description |
|---------|-------------|
| `/help` | Show this help message |
| `/quit` | Exit the application |
| `/clear` | Clear chat history and start fresh |
| `/save` | Save session (auto-saves on exit) |
| `/load` | Load/resume a saved session |
| `/status` | Show current configuration and status |
| `/tokens` | Show token usage statistics |
| `/reasoning` | Toggle reasoning display on/off |
| `/think` | Switch to deep thinking mode (if supported) |

## Input Features

- **`@filepath`** - Attach a file inline (e.g., `@src/main.rs`)
  - Use `@path:N` to read from line N (e.g., `@src/main.rs:50`)
  - Large files (>500 lines or 50KB) are truncated with a notice

- **Shift+Enter** - Insert newline in input
- **Enter** - Send message
- **Ctrl+C** - Interrupt response (once) or quit (twice)
- **Ctrl+R** - Toggle reasoning display
- **PageUp/PageDown** - Scroll chat history
- **Mouse wheel** - Scroll chat history

## Tools Available (13 total)

`file_read` · `file_write` · `file_update` · `file_delete` · `shell_exec` · `code_search` · `code_review` · `list_dir` · `glob` · `git_status` · `git_diff` · `git_log` · `git_commit`

## Tips

- Type your question or task in natural language
- Attach files using `@filepath` for context
- The AI will automatically use tools when needed
- Sessions auto-save to `.session.json` if enabled in config.toml
"#;
    help.to_string()
}
