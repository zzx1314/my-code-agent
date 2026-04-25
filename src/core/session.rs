use colored::*;
use rig::completion::Message;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::token_usage::TokenUsage;

pub const SESSION_DIR: &str = ".sessions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub chat_history: Vec<Message>,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub saved_at: u64,
    pub name: Option<String>,
}

impl SessionData {
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
            name: None,
        }
    }

    pub fn with_name(
        chat_history: Vec<Message>,
        token_usage: TokenUsage,
        last_reasoning: String,
        name: String,
    ) -> Self {
        Self {
            chat_history,
            token_usage,
            last_reasoning,
            saved_at: unix_epoch_secs(),
            name: Some(name),
        }
    }

    pub fn session_file_path(name: &str) -> String {
        format!("{}/{}.json", SESSION_DIR, name)
    }

    pub fn session_dir_path() -> String {
        SESSION_DIR.to_string()
    }

    pub fn save_to_file(&self, path: &str) -> Result<(), String> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write {}: {}", path, e))
    }

    pub fn save_with_name(&self, name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        self.save_to_file(&path)
    }

    pub fn load_from_file(path: &str) -> Option<Result<Self, String>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None,
        };
        let result = serde_json::from_str(&content)
            .map_err(|e| format!("parse {}: {}", path, e));
        Some(result)
    }

    pub fn load_by_name(name: &str) -> Option<Result<Self, String>> {
        let path = Self::session_file_path(name);
        Self::load_from_file(&path)
    }

    pub fn delete_file(path: &str) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete {}: {}", path, e)),
        }
    }

    pub fn delete_by_name(name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        Self::delete_file(&path)
    }

    pub fn list_sessions() -> Vec<SessionInfo> {
        let mut sessions = Vec::new();
        
        let dir_path = std::path::Path::new(SESSION_DIR);
        if !dir_path.is_dir() {
            return sessions;
        }
        
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Some(name) = path.file_stem() {
                        let name_str = name.to_string_lossy().to_string();
                        if let Some(data) = Self::load_from_file(&path.to_string_lossy()) {
                            if let Ok(data) = data {
                                sessions.push(SessionInfo {
                                    name: name_str,
                                    turns: data.chat_history.iter()
                                        .filter(|m| matches!(m, Message::User { .. }))
                                        .count(),
                                    saved_at: data.saved_at,
                                    tokens: data.token_usage.total_tokens(),
                                });
                            }
                        }
                    }
                }
            }
        }
        
        sessions.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
        sessions
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub turns: usize,
    pub saved_at: u64,
    pub tokens: u64,
}

fn unix_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn format_timestamp(secs: u64) -> String {
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