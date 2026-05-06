use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// Maximum number of undo entries to keep in history.
const MAX_HISTORY: usize = 100;

/// The history file is stored in the current working directory.
const HISTORY_FILE: &str = ".undo_history.json";

/// Global session ID for the current session. Set once at startup.
static CURRENT_SESSION_ID: Mutex<String> = Mutex::new(String::new());

/// Represents a single file change history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    /// The path of the file that was changed.
    pub file_path: PathBuf,
    /// The file content before the change (None if the file didn't exist).
    pub old_content: Option<String>,
    /// The file content after the change (None if the file was deleted).
    pub new_content: Option<String>,
    /// Unix timestamp of the change.
    pub timestamp: u64,
    /// Human-readable description of the operation.
    pub operation: String,
    /// The session ID that made this change.
    #[serde(default)]
    pub session_id: String,
}

/// Set the current session ID. Should be called once at startup.
pub fn set_session_id(id: String) {
    if let Ok(mut current) = CURRENT_SESSION_ID.lock() {
        *current = id;
    }
}

/// Get the current session ID.
pub fn get_current_session_id() -> String {
    CURRENT_SESSION_ID
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Get the path to the history file.
fn history_file_path() -> PathBuf {
    PathBuf::from(HISTORY_FILE)
}

/// Load the undo history from disk.
fn load_history() -> Result<Vec<UndoEntry>> {
    let path = history_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let history: Vec<UndoEntry> = serde_json::from_str(&content)?;
    Ok(history)
}

/// Save the undo history to disk.
fn save_history(history: &[UndoEntry]) -> Result<()> {
    let content = serde_json::to_string_pretty(history)?;
    std::fs::write(history_file_path(), content)?;
    Ok(())
}

/// Get the current Unix timestamp.
fn now_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Record a file change to the undo history.
///
/// Call this **before** actually writing/deleting the file, passing the current
/// content as `old_content` and the planned new content as `new_content`.
///
/// The session ID is automatically read from the global `CURRENT_SESSION_ID`.
pub fn record_change(
    file_path: &str,
    old_content: Option<String>,
    new_content: Option<String>,
    operation: &str,
) -> Result<()> {
    let mut history = load_history().unwrap_or_default();
    let session_id = get_current_session_id();

    history.push(UndoEntry {
        file_path: PathBuf::from(file_path),
        old_content,
        new_content,
        timestamp: now_timestamp(),
        operation: operation.to_string(),
        session_id,
    });

    // Keep only the last MAX_HISTORY entries.
    if history.len() > MAX_HISTORY {
        let drain = history.len() - MAX_HISTORY;
        history.drain(..drain);
    }

    save_history(&history)
}

/// Pop the last `count` entries from the history (most recent first) and return them.
/// Also saves the remaining history back to disk.
pub fn pop_last_entries(count: usize) -> Result<Vec<UndoEntry>> {
    let mut history = load_history().unwrap_or_default();

    if history.is_empty() {
        return Ok(Vec::new());
    }

    let split_at = history.len().saturating_sub(count);
    let popped: Vec<UndoEntry> = history.drain(split_at..).collect();
    save_history(&history)?;

    // Return in reverse order (most recent first) for undo.
    Ok(popped.into_iter().rev().collect())
}

/// Pop all entries belonging to the current session from the history.
/// Returns them in reverse chronological order (most recent first).
/// Entries from other sessions are preserved.
pub fn pop_current_session_entries() -> Result<Vec<UndoEntry>> {
    let history = load_history().unwrap_or_default();
    let current_id = get_current_session_id();

    if history.is_empty() || current_id.is_empty() {
        return Ok(Vec::new());
    }

    // Separate current session entries from others
    let mut remaining = Vec::new();
    let mut popped = Vec::new();

    for entry in history {
        if entry.session_id == current_id {
            popped.push(entry);
        } else {
            remaining.push(entry);
        }
    }

    save_history(&remaining)?;

    // Return in reverse order (most recent first) for undo.
    Ok(popped.into_iter().rev().collect())
}

/// Clear all entries belonging to the current session from the history
/// without returning them (used on session exit).
pub fn clear_current_session_entries() -> Result<usize> {
    let history = load_history().unwrap_or_default();
    let current_id = get_current_session_id();

    if history.is_empty() || current_id.is_empty() {
        return Ok(0);
    }

    let original_len = history.len();
    let remaining: Vec<UndoEntry> = history
        .into_iter()
        .filter(|entry| entry.session_id != current_id)
        .collect();

    let cleared = original_len - remaining.len();
    save_history(&remaining)?;

    Ok(cleared)
}

/// Return the number of entries currently in the history.
pub fn history_len() -> Result<usize> {
    let history = load_history().unwrap_or_default();
    Ok(history.len())
}

/// Return the number of entries belonging to the current session.
pub fn current_session_history_len() -> Result<usize> {
    let history = load_history().unwrap_or_default();
    let current_id = get_current_session_id();
    if current_id.is_empty() {
        return Ok(0);
    }
    Ok(history.iter().filter(|e| e.session_id == current_id).count())
}
