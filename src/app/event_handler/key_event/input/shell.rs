use super::reset_input;
use crate::app::App;

/// Handle shell mode command execution
pub fn handle_shell_command(app: &mut App, input_text: &str) {
    let cmd = if app.shell_mode {
        input_text.to_string()
    } else {
        input_text
            .strip_prefix('!')
            .unwrap_or(input_text)
            .trim()
            .to_string()
    };

    // Handle shell mode exit commands
    if cmd == "exit" || cmd == "!exit" {
        app.shell_mode = false;
        app.chat_history
            .push(("user".to_string(), input_text.to_string()));
        app.chat_history.push((
            "assistant".to_string(),
            "🐚 Shell mode deactivated.".to_string(),
        ));
        reset_input(app);
        return;
    }

    if cmd.is_empty() {
        return;
    }

    // Handle cd command specially — subprocess cd doesn't affect the parent process
    let cmd_trimmed = cmd.trim();
    let is_cd =
        cmd_trimmed == "cd" || cmd_trimmed.starts_with("cd ") || cmd_trimmed.starts_with("cd\t");
    if is_cd {
        let target = if cmd_trimmed == "cd" {
            std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
        } else {
            cmd_trimmed[2..].trim().to_string()
        };
        // Handle ~ expansion
        let target = if target.starts_with('~') {
            if let Ok(home) = std::env::var("HOME") {
                target.replacen('~', &home, 1)
            } else {
                target
            }
        } else {
            target
        };
        app.chat_history
            .push(("user".to_string(), format!("$ {}", cmd)));
        match std::env::set_current_dir(&target) {
            Ok(()) => {
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "?".to_string());
                app.chat_history.push((
                    "assistant".to_string(),
                    format!("Changed directory to {}", cwd),
                ));
            }
            Err(e) => {
                app.chat_history
                    .push(("assistant".to_string(), format!("❌ cd: {}: {}", target, e)));
            }
        }
        reset_input(app);
        return;
    }

    app.chat_history.push((
        "user".to_string(),
        if app.shell_mode {
            format!("$ {}", cmd)
        } else {
            input_text.to_string()
        },
    ));

    // Execute shell command
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(&format!("stderr:\n{}", stderr));
            }
            if !o.status.success() {
                result.push_str(&format!("\n(exit code: {})", o.status.code().unwrap_or(-1)));
            }
            if result.is_empty() {
                result = "(no output)".to_string();
            }
            app.chat_history.push(("assistant".to_string(), result));
        }
        Err(e) => {
            app.chat_history.push((
                "assistant".to_string(),
                format!("❌ Failed to execute command: {}", e),
            ));
        }
    }

    reset_input(app);
    app.show_inline_reasoning = false;
}
