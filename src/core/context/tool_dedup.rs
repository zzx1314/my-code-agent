use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Type definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Key for file_outline dedup: just the file path
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct OutlineKey {
    path: PathBuf,
}

/// Stores metadata and cached outline content for a previous outline call.
#[derive(Debug, Clone)]
struct OutlineRecord {
    /// File modification time at the time of the read.
    mtime: SystemTime,
    /// Total lines in the file.
    total_lines: usize,
    /// Cached outline string returned on short-circuit (avoids re-parsing).
    cached_outline: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Global singleton
// ─────────────────────────────────────────────────────────────────────────────

static GLOBAL_TOOL_DEDUP: OnceLock<Arc<Mutex<ToolCallDedup>>> = OnceLock::new();

/// Get the global tool dedup instance.
pub fn get_global_tool_dedup() -> Arc<Mutex<ToolCallDedup>> {
    GLOBAL_TOOL_DEDUP
        .get_or_init(|| Arc::new(Mutex::new(ToolCallDedup::new())))
        .clone()
}

// ─────────────────────────────────────────────────────────────────────────────
// ToolCallDedup implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks previous `file_outline` calls so we can return cached results
/// instead of re-parsing the file. Only used for `file_outline` — `file_read`
/// is NOT deduplicated (doing so would risk the model hallucinating content).
pub struct ToolCallDedup {
    outline_records: HashMap<OutlineKey, OutlineRecord>,
}

impl ToolCallDedup {
    pub fn new() -> Self {
        Self {
            outline_records: HashMap::new(),
        }
    }

    /// Check if a file_outline with the same path has been done before
    /// and the file hasn't been modified since.
    ///
    /// Unlike `check_file_read`, this always returns the cached outline on hit
    /// (no hit_count escalation) since the outline is small and cached directly.
    /// The model always gets useful data instead of a `[DEDUP]` message.
    pub fn check_file_outline(&mut self, path: &str) -> DedupAction {
        let path_buf = PathBuf::from(path);
        let key = OutlineKey { path: path_buf };

        if let Some(record) = self.outline_records.get(&key) {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(current_mtime) = metadata.modified() {
                    if current_mtime == record.mtime {
                        // File unchanged — return cached outline directly.
                        // No hit_count throttle needed since we store the real outline,
                        // not a ["DEDUP"] message. The model always gets useful data.
                        return DedupAction::ShortCircuit(DedupInfo {
                            path: path.to_string(),
                            total_lines: record.total_lines,
                            cached_outline: record.cached_outline.clone(),
                        });
                    }
                }
            }
        }

        DedupAction::Allow
    }

    /// Record a completed file_outline so future identical calls can be short-circuited.
    /// The `outline` string is cached and returned directly on subsequent dedup hits.
    pub fn record_file_outline(&mut self, path: &str, total_lines: usize, outline: &str) {
        let path_buf = PathBuf::from(path);

        let mtime = std::fs::metadata(&path_buf)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let key = OutlineKey { path: path_buf };

        self.outline_records.insert(
            key,
            OutlineRecord {
                mtime,
                total_lines,
                cached_outline: outline.to_string(),
            },
        );
    }

    /// Invalidate all records for a specific path (e.g., after file_write or file_update).
    pub fn invalidate_path(&mut self, path: &str) {
        let path_buf = PathBuf::from(path);
        self.outline_records.retain(|key, _| key.path != path_buf);
    }

    /// Invalidate all dedup records.
    pub fn invalidate_all(&mut self) {
        self.outline_records.clear();
    }

    /// Reset all dedup state. Call this on new session.
    pub fn reset(&mut self) {
        self.outline_records.clear();
    }

    /// Number of cached dedup entries.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.outline_records.len()
    }
}

impl Default for ToolCallDedup {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DedupAction / DedupInfo — controls what FileOutline returns on duplicate calls
// ─────────────────────────────────────────────────────────────────────────────

/// Action to take when a duplicate call is detected.
#[derive(Debug)]
pub enum DedupAction {
    /// Proceed with the full operation (no duplicate, or stale cache).
    Allow,
    /// Return cached data instead of re-executing.
    ShortCircuit(DedupInfo),
}

/// Minimal metadata for a short-circuit response from `file_outline`.
#[derive(Debug, Clone)]
pub struct DedupInfo {
    pub path: String,
    pub total_lines: usize,
    /// Cached outline content returned directly to the model.
    pub cached_outline: String,
}

impl DedupInfo {
    /// Format a short message suitable as a tool result.
    pub fn format_message(&self) -> String {
        format!(
            "[DEDUP] File \"{}\" ({} total lines) was already outlined above. \
             No need to re-outline. Use the outline from the conversation history. \
             If the content is no longer in context (was pruned), call file_outline again.",
            self.path, self.total_lines,
        )
    }
}
