use crate::core::paths;
use crate::core::token_usage::TokenUsage;
use crate::core::types::Message;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SESSION_DIR: &str = ".sessions";
pub const DEFAULT_SESSION_FILE: &str = ".session.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub chat_history: Vec<Message>,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub saved_at: u64,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub turns: usize,
    pub saved_at: u64,
    pub tokens: u64,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub session_name: String,
    pub saved_at: u64,
    pub matches: Vec<MessageMatch>,
}

#[derive(Debug, Clone)]
pub struct MessageMatch {
    pub role: String,
    pub content_snippet: String,
    pub line_number: usize,
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
        paths::app_file(&format!("{}/{}.json", SESSION_DIR, name))
            .to_string_lossy()
            .to_string()
    }

    pub fn session_dir_path() -> String {
        paths::app_file(SESSION_DIR).to_string_lossy().to_string()
    }

    pub fn default_session_file_path(save_file: Option<&str>) -> String {
        save_file
            .filter(|s| !s.is_empty())
            .map(|s| paths::app_file(s).to_string_lossy().to_string())
            .unwrap_or_else(|| {
                paths::app_file(DEFAULT_SESSION_FILE)
                    .to_string_lossy()
                    .to_string()
            })
    }

    pub fn save_to_file(&self, path: &str) -> Result<(), String> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write {}: {}", path, e))
    }

    pub fn load_from_file(path: &str) -> Option<Result<Self, String>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None,
        };
        let result =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {}", path, e));
        Some(result)
    }

    pub fn delete_file(path: &str) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete {}: {}", path, e)),
        }
    }

    pub fn save_with_name(&self, name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        self.save_to_file(&path)
    }

    pub fn load_by_name(name: &str) -> Option<Result<Self, String>> {
        let path = Self::session_file_path(name);
        Self::load_from_file(&path)
    }

    pub fn delete_by_name(name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        Self::delete_file(&path)
    }

    pub fn save_default(&self, save_file: Option<&str>) -> Result<(), String> {
        let path = Self::default_session_file_path(save_file);
        self.save_to_file(&path)
    }

    pub fn load_default(save_file: Option<&str>) -> Option<Result<Self, String>> {
        let path = Self::default_session_file_path(save_file);
        Self::load_from_file(&path)
    }

    pub fn delete_default(save_file: Option<&str>) -> Result<(), String> {
        let path = Self::default_session_file_path(save_file);
        if Path::new(&path).exists() {
            Self::delete_file(&path)
        } else {
            Ok(())
        }
    }

    pub fn list_sessions() -> Vec<SessionInfo> {
        let mut sessions = Vec::new();
        let dir_path = paths::app_file(SESSION_DIR);
        if !dir_path.is_dir() {
            return sessions;
        }
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Some(name) = path.file_stem() {
                        let name_str = name.to_string_lossy().to_string();
                        if let Some(data) =
                            Self::load_from_file(&path.to_string_lossy())
                        {
                            if let Ok(data) = data {
                                sessions.push(SessionInfo {
                                    name: name_str,
                                    turns: data
                                        .chat_history
                                        .iter()
                                        .filter(|m| m.role == "user")
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

    pub fn prune_old_sessions(max_count: usize) -> Result<usize, String> {
        let sessions = Self::list_sessions();
        if sessions.len() <= max_count {
            return Ok(0);
        }
        let mut removed = 0;
        for session in sessions.iter().skip(max_count) {
            let path = Self::session_file_path(&session.name);
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(path = %path, error = %e, "Failed to remove old session file");
            } else {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn search_in_session(&self, keyword: &str) -> Vec<MessageMatch> {
        let mut matches = Vec::new();
        let keyword_lower = keyword.to_lowercase();
        for (idx, message) in self.chat_history.iter().enumerate() {
            let role = &message.role;
            let content = &message.content;
            if content.to_lowercase().contains(&keyword_lower) {
                let snippet = extract_snippet(content, keyword, 100);
                matches.push(MessageMatch {
                    role: role.to_string(),
                    content_snippet: snippet,
                    line_number: idx,
                });
            }
        }
        matches
    }
}

pub fn generate_session_name() -> String {
    let secs = unix_epoch_secs();
    let timestamp = format_timestamp(secs);
    format!("session_{}", timestamp.replace(&['-', ':', ' '][..], "_"))
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

pub fn format_saved_confirmation(path: &str, data: &SessionData) -> String {
    let turns = data
        .chat_history
        .iter()
        .filter(|m| m.role == "user")
        .count();
    format!(
        "💾 session saved to {} ({} turns, {} tokens)",
        path,
        turns,
        data.token_usage.total_tokens()
    )
}

pub fn print_saved_confirmation(path: &str, data: &SessionData) {
    println!("  {}", format_saved_confirmation(path, data));
}

pub fn search_sessions(keyword: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let dir_path = Path::new(SESSION_DIR);
    if !dir_path.is_dir() {
        return results;
    }
    if let Ok(entries) = std::fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(name) = path.file_stem() {
                    let name_str = name.to_string_lossy().to_string();
                    if let Some(load_result) =
                        SessionData::load_from_file(&path.to_string_lossy())
                    {
                        if let Ok(session_data) = load_result {
                            let matches = session_data.search_in_session(keyword);
                            if !matches.is_empty() {
                                results.push(SearchResult {
                                    session_name: name_str,
                                    saved_at: session_data.saved_at,
                                    matches,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    results.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    results
}

fn extract_snippet(content: &str, keyword: &str, context_size: usize) -> String {
    let content_lower = content.to_lowercase();
    let keyword_lower = keyword.to_lowercase();
    if let Some(byte_pos) = content_lower.find(&keyword_lower) {
        let char_pos = content
            .char_indices()
            .enumerate()
            .find_map(|(char_idx, (byte_idx, _))| {
                if byte_idx == byte_pos {
                    Some(char_idx)
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let char_start = char_pos.saturating_sub(context_size / 2);
        let char_end = (char_pos + keyword.chars().count() + context_size / 2)
            .min(content.chars().count());
        let start_byte = content
            .char_indices()
            .nth(char_start)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let end_byte = content
            .char_indices()
            .nth(char_end)
            .map(|(i, _)| i)
            .unwrap_or(content.len());
        let mut snippet = String::new();
        if char_start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(&content[start_byte..end_byte]);
        if char_end < content.chars().count() {
            snippet.push_str("...");
        }
        snippet
    } else {
        content.chars().take(context_size).collect()
    }
}
