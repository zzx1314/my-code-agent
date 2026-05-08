use crate::app::App;

/// Generate help text
pub(super) fn handle(app: &mut App) -> bool {
    let help_text = generate_help_text();
    app.chat_history
        .push(("user".to_string(), "/help".to_string()));
    app.chat_history.push(("assistant".to_string(), help_text));
    app.show_banner = false;
    app.auto_scroll = true;
    app.scroll = u16::MAX;
    true
}

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
| `/connect` | Select LLM provider (deepseek / openrouter) |
| `/model` | Select model from dropdown menu |
| `/think` | Show last reasoning/thinking content |
| `/init` | Initialize or update project knowledge file |
| `/undo` | Undo all file changes made in this session (restore to session start) |
| `/plan <task>` | Enter plan mode — analyze and create an implementation plan without executing |
| `/shell` | Toggle shell mode (all input executed as shell commands) |

## Input Features

- **`@filepath`** - Attach a file inline (e.g., `@src/main.rs`)
  - Use `@path:N` to read from line N (e.g., `@src/main.rs:50`)
  - Large files (>500 lines or 50KB) are truncated with a notice

- **`!command`** - Execute a shell command directly (e.g., `!ls -la`)
- **`/shell`** - Enter persistent shell mode (type `exit` or `/shell` to leave)
- **Alt+Enter** - Insert newline in input
- **Enter** - Send message
- **Esc** / **Ctrl+C** - Interrupt response | **Esc** twice / **Ctrl+C** twice - Quit
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
