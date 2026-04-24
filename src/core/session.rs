use colored::*;
use rig::completion::Message;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::config::Config;
use super::token_usage::TokenUsage;

/// Default session file name (looked up in the current directory).
pub const DEFAULT_SESSION_FILE: &str = ".session.json";

/// Serializable representation of a conversation session.
///
/// Contains everything needed to restore the agent to its previous state
/// after a restart: the full chat history, cumulative token usage, and
/// the last reasoning content (so `think` still works after resume).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// The conversation history (user + assistant messages).
    pub chat_history: Vec<Message>,
    /// Cumulative token usage for the session.
    pub token_usage: TokenUsage,
    /// The last reasoning content from DeepSeek Reasoner.
    pub last_reasoning: String,
    /// Unix timestamp (seconds since epoch) of when the session was saved.
    pub saved_at: u64,
}

impl SessionData {
    /// Creates a new `SessionData` from the current session state.
    pub fn new(
        chat_history: Vec<Message>,
        token_usage: TokenUsage,
        last_reasoning: String,
    ) -> Self {
        Self {
            chat_history,
            token_usage,
            last_reasoning,
            saved_at: unix_epoch_secs(),
        }
    }

    /// Saves the session data to a JSON file at the given path.
    pub fn save_to_file(&self, path: &str) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write {}: {}", path, e))
    }

    /// Loads session data from a JSON file at the given path.
    /// Returns `None` if the file does not exist.
    /// Returns an error if the file exists but cannot be parsed.
    pub fn load_from_file(path: &str) -> Option<Result<Self, String>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None, // File doesn't exist — no session to resume
        };
        let result = serde_json::from_str(&content)
            .map_err(|e| format!("parse {}: {}", path, e));
        Some(result)
    }

    /// Deletes the session file. Returns Ok(()) if the file was deleted
    /// or didn't exist. Returns an error if deletion failed.
    pub fn delete_file(path: &str) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete {}: {}", path, e)),
        }
    }

    /// Returns the session file path from the config, or the default.
    pub fn session_path(config: &Config) -> &str {
        config.session.save_file.as_deref().unwrap_or(DEFAULT_SESSION_FILE)
    }
}

/// Returns the current time as seconds since the Unix epoch.
/// Uses `std::time::SystemTime` — no external dependency needed, portable.
fn unix_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Formats a unix timestamp into a human-readable local datetime string.
/// Uses the system `date` command (GNU: `date -d @SECS`; BSD: `date -r SECS`).
/// Falls back to showing raw seconds if the command is unavailable.
pub fn format_timestamp(secs: u64) -> String {
    // Try GNU date syntax first (Linux), then BSD (macOS)
    let gnu = std::process::Command::new("date")
        .arg(format!("-d@{}", secs))
        .arg("+%Y-%m-%d %H:%M:%S")
        .output();
    if let Ok(out) = gnu
        && out.status.success()
    {
        return String::from_utf8_lossy(&out.stdout).trim().to_string();
    }
    let bsd = std::process::Command::new("date")
        .arg(format!("-r{}", secs))
        .arg("+%Y-%m-%d %H:%M:%S")
        .output();
    if let Ok(out) = bsd
        && out.status.success()
    {
        return String::from_utf8_lossy(&out.stdout).trim().to_string();
    }
    format!("epoch:{}", secs)
}

/// Prints a summary of a resumed session.
pub fn print_resume_summary(data: &SessionData) {
    let turns = data
        .chat_history
        .iter()
        .filter(|m| matches!(m, Message::User { .. }))
        .count();
    let when = format_timestamp(data.saved_at);
    println!(
        "  {} {}",
        "📂".bright_cyan(),
        format!(
            "resumed session from {} — {} turns, {} tokens used",
            when,
            turns,
            data.token_usage.total_tokens()
        )
        .dimmed()
    );
}

/// Prints a confirmation message when the session is saved.
pub fn print_saved_confirmation(path: &str, data: &SessionData) {
    let turns = data
        .chat_history
        .iter()
        .filter(|m| matches!(m, Message::User { .. }))
        .count();
    println!(
        "  {} {}",
        "💾".bright_green(),
        format!(
            "session saved to {} ({} turns, {} tokens)",
            path,
            turns,
            data.token_usage.total_tokens()
        )
        .dimmed()
    );
}
