use glob::glob;

use crate::app::App;

/// Trigger the completion menu
pub fn trigger_completion(app: &mut App, trigger_char: char) {
    app.show_completion = true;
    app.completion_type = Some(trigger_char);
    app.completion_selected = 0;
    app.completion_trigger_pos = get_cursor_position(app);
    app.completion_query = String::new();

    // Fetch and cache completion items
    let items = get_completion_items(trigger_char);
    app.completion_all_items = items.clone();
    app.completion_items = items;
}

/// Hide the completion menu
pub fn hide_completion(app: &mut App) {
    app.show_completion = false;
    app.completion_items.clear();
    app.completion_all_items.clear();
    app.completion_selected = 0;
    app.completion_type = None;
    app.completion_query.clear();
    app.completion_trigger_pos = 0;
}

/// Convert a character index to a byte index (handles multi-byte characters)
fn char_idx_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// Apply the selected completion
pub fn apply_completion(app: &mut App) {
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

    // Get the current input text
    let mut lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();
    let cursor = app.input.cursor();

    let mut added_trailing_space = false;

    // Find the current line
    if cursor.0 < lines.len() {
        let line = &mut lines[cursor.0];
        let pos = char_idx_to_byte(line, cursor.1);

        // Find the position of the trigger character (search backwards from the cursor position)
        let trigger_pos = line[..pos]
            .rfind(trigger_char)
            .unwrap_or(pos.saturating_sub(1));

        // Replace content from the trigger position to the cursor position
        // Auto-add a trailing space after '@' file path completions
        let extra = if trigger_char == '@' && !line[pos..].starts_with(' ') { added_trailing_space = true; " " } else { "" };
        let new_line = format!("{}{}{}{}", &line[..trigger_pos], selected, extra, &line[pos..]);
        lines[cursor.0] = new_line;
    }

    let new_text = lines.join("\n");
    let mut new_input = tui_textarea::TextArea::from(new_text.lines());
    new_input.set_block(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
    );
    new_input.set_cursor_line_style(ratatui::style::Style::default());
    app.input = new_input;

    // Set cursor position to the end of the completion
    let completion_len = selected.len();
    let cursor = app.input.cursor();
    let new_cursor_col = app.completion_trigger_pos + completion_len + if added_trailing_space { 1 } else { 0 };
    app.input.move_cursor(tui_textarea::CursorMove::Jump(
        cursor.0 as u16,
        new_cursor_col as u16,
    ));

    hide_completion(app);
}

/// Update the completion query string (used for filtering)
pub fn update_completion_query(app: &mut App) {
    let cursor_pos = get_cursor_position(app);
    if cursor_pos <= app.completion_trigger_pos {
        hide_completion(app);
        return;
    }

    // Get text from trigger position to cursor position as the query string
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

    // Filter completion items from cached full list
    if app.completion_query.is_empty() {
        app.completion_items = app.completion_all_items.clone();
    } else {
        app.completion_items = app.completion_all_items
            .iter()
            .filter(|item| {
                item.to_lowercase()
                    .contains(&app.completion_query.to_lowercase())
            })
            .cloned()
            .collect();
    }
    app.completion_selected = 0;
}

/// Get the current cursor position (character offset)
pub fn get_cursor_position(app: &App) -> usize {
    let cursor = app.input.cursor();
    cursor.1
}

/// Get the list of completion items
fn get_completion_items(trigger_char: char) -> Vec<String> {
    match trigger_char {
        '/' => {
            // Command completion
            vec![
                "/help".to_string(),
                "/quit".to_string(),
                "/clear".to_string(),
                "/save".to_string(),
                "/load".to_string(),
                "/status".to_string(),
                "/tokens".to_string(),
                "/think".to_string(),
                "/connect".to_string(),
                "/model".to_string(),
                "/init".to_string(),
                "/review".to_string(),
                "/undo".to_string(),
                "/plan".to_string(),
                "/shell".to_string(),
                "/compact".to_string(),
            ]
        }
        '@' => {
            // File completion - use glob to get files in the current directory
            let mut files = Vec::new();

            // Directories to skip in file completion
            let skip_dirs = ["target", "node_modules", "dist", "build", ".git"];

            // Get all files in the current directory (recursive depth 2)
            if let Ok(entries) = glob("**/*") {
                for entry in entries.flatten() {
                    if let Some(path_str) = entry.to_str() {
                        // Skip hidden files and directories
                        if path_str.starts_with('.') || path_str.contains("/.") {
                            continue;
                        }
                        // Skip common build/dependency directories
                        let should_skip = skip_dirs.iter().any(|dir| {
                            path_str == *dir
                                || path_str.starts_with(&format!("{}/", dir))
                                || path_str.contains(&format!("/{}/", dir))
                        });
                        if should_skip {
                            continue;
                        }
                        files.push(format!("@{}", path_str));
                    }
                }
            }

            // If no files found, add some examples
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
