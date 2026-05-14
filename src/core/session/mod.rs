use crate::core::paths;
use crate::core::context::token_usage::TokenUsage;
use crate::core::types::Message;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Directory name used to store session files.
pub const SESSION_DIR: &str = ".sessions";
/// Default file name for the auto-saved session.
pub const DEFAULT_SESSION_FILE: &str = ".session.json";

/// Full persisted data for a single chat session.
///
/// Serialized to/from JSON for saving and loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// The complete message history for the session.
    pub chat_history: Vec<Message>,
    /// Cumulative token usage across the session.
    pub token_usage: TokenUsage,
    /// The last reasoning output (e.g. from the assistant).
    pub last_reasoning: String,
    /// Unix timestamp (seconds) when the session was last saved.
    pub saved_at: u64,
    /// Optional human-readable name for the session.
    pub name: Option<String>,
}

/// Lightweight metadata about a saved session (no full content loaded).
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// File stem (name without extension) of the session.
    pub name: String,
    /// Number of user turns in the session.
    pub turns: usize,
    /// Unix timestamp of when the session was saved.
    pub saved_at: u64,
    /// Total tokens consumed by the session.
    pub tokens: u64,
}

/// Result of searching across all sessions for a keyword.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The name of the session that matched.
    pub session_name: String,
    /// When the session was saved.
    pub saved_at: u64,
    /// Individual message matches within the session.
    pub matches: Vec<MessageMatch>,
}

/// A single message match found during a session search.
#[derive(Debug, Clone)]
pub struct MessageMatch {
    /// Role of the matched message (e.g. "user", "assistant").
    pub role: String,
    /// Truncated snippet of content surrounding the keyword match.
    pub content_snippet: String,
    /// Index of the message in the chat history.
    pub line_number: usize,
}

impl SessionData {
    /// Create a new `SessionData` with the given chat history, token usage,
    /// and last reasoning. Automatically sets `saved_at` to the current time.
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

    /// Create a new `SessionData` with an explicit session name.
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

    /// Build the filesystem path for a named session file
    /// (e.g. `<app_dir>/.sessions/<name>.json`).
    pub fn session_file_path(name: &str) -> String {
        paths::app_file(&format!("{}/{}.json", SESSION_DIR, name))
            .to_string_lossy()
            .to_string()
    }

    /// Build the filesystem path for the session directory
    /// (e.g. `<app_dir>/.sessions`).
    pub fn session_dir_path() -> String {
        paths::app_file(SESSION_DIR).to_string_lossy().to_string()
    }

    /// Build the path of the default session file.
    ///
    /// If `save_file` is `Some` and non-empty, it is resolved via `paths::app_file`.
    /// Otherwise falls back to `DEFAULT_SESSION_FILE`.
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

    /// Serialize `self` to pretty-printed JSON and write it to `path`.
    /// Creates parent directories if they don't exist.
    pub fn save_to_file(&self, path: &str) -> Result<(), String> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write {}: {}", path, e))
    }

    /// Load a `SessionData` from a JSON file at `path`.
    ///
    /// Returns `None` if the file doesn't exist or can't be read.
    /// Returns `Some(Err(...))` if the file exists but is malformed.
    pub fn load_from_file(path: &str) -> Option<Result<Self, String>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None,
        };
        let result =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {}", path, e));
        Some(result)
    }

    /// Delete a file at `path`. Returns `Ok(())` if the file doesn't exist.
    pub fn delete_file(path: &str) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete {}: {}", path, e)),
        }
    }

    /// Save the session data to a file named `<name>.json` in the session directory.
    pub fn save_with_name(&self, name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        self.save_to_file(&path)
    }

    /// Load a session by its name from the session directory.
    pub fn load_by_name(name: &str) -> Option<Result<Self, String>> {
        let path = Self::session_file_path(name);
        Self::load_from_file(&path)
    }

    /// Delete a named session file from the session directory.
    pub fn delete_by_name(name: &str) -> Result<(), String> {
        let path = Self::session_file_path(name);
        Self::delete_file(&path)
    }

    /// Save the session data to the default session file.
    pub fn save_default(&self, save_file: Option<&str>) -> Result<(), String> {
        let path = Self::default_session_file_path(save_file);
        self.save_to_file(&path)
    }

    /// Load the session data from the default session file.
    pub fn load_default(save_file: Option<&str>) -> Option<Result<Self, String>> {
        let path = Self::default_session_file_path(save_file);
        Self::load_from_file(&path)
    }

    /// Delete the default session file, if it exists.
    pub fn delete_default(save_file: Option<&str>) -> Result<(), String> {
        let path = Self::default_session_file_path(save_file);
        if Path::new(&path).exists() {
            Self::delete_file(&path)
        } else {
            Ok(())
        }
    }

    /// List all saved sessions as lightweight [`SessionInfo`] entries,
    /// sorted by most recently saved first.
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

    /// Remove the oldest sessions so that at most `max_count` remain.
    /// Returns the number of sessions removed.
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

    /// Search through all messages in this session for a keyword
    /// (case-insensitive) and return matching snippets.
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

/// Generate a session name based on the current timestamp
/// (e.g. `session_2025_01_15_10_30_00`).
pub fn generate_session_name() -> String {
    let secs = unix_epoch_secs();
    let timestamp = format_timestamp(secs);
    format!("session_{}", timestamp.replace(&['-', ':', ' '][..], "_"))
}

/// Get the current Unix timestamp in seconds.
fn unix_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format a Unix timestamp as `"YYYY-MM-DD HH:MM:SS"`.
///
/// Tries GNU `date -d@<secs>` first, then BSD `date -r<secs>`,
/// and falls back to `"epoch:<secs>"` if neither works.
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

/// Build a human-readable confirmation string describing a saved session
/// (includes path, turn count, and token count).
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

/// Print a saved confirmation message to stdout (delegates to [`format_saved_confirmation`]).
pub fn print_saved_confirmation(path: &str, data: &SessionData) {
    println!("  {}", format_saved_confirmation(path, data));
}

/// Search all session files in the session directory for a keyword
/// (case-insensitive) and return results sorted by most recent first.
///
/// This is a convenience wrapper that scans every `.json` file in
/// [`SESSION_DIR`], loads it, and collects matching [`MessageMatch`]es.
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

/// Extract a snippet of text surrounding a keyword match.
///
/// Finds the keyword (case-insensitive) within `content` and returns
/// up to `context_size` characters centered around it, with `"..."`
/// ellipsis markers if the snippet is truncated at either end.
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
