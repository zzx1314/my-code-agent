use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Type definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Key for deduplication: (path, offset, limit)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ReadKey {
    path: PathBuf,
    offset: usize,
    limit: usize,
}

/// Stores metadata about a previous read (no content — we only need the mtime for staleness checks).
#[derive(Debug, Clone)]
struct ReadRecord {
    /// File modification time at the time of the read.
    mtime: SystemTime,
    /// Total lines in the file at the time of the read.
    total_lines: usize,
    /// Start line index (0-indexed) of the returned content.
    start: usize,
    /// End line index (exclusive, 0-indexed) of the returned content.
    end: usize,
    /// Number of times this same read has been short-circuited.
    /// On the second+ hit the model likely lost context, so we fall through
    /// to a full re-read.
    hit_count: u32,
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

pub struct ToolCallDedup {
    records: HashMap<ReadKey, ReadRecord>,
}

impl ToolCallDedup {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Check if a file_read with the same parameters has been done before
    /// and the file hasn't been modified since.
    ///
    /// Returns `DedupAction` indicating whether to short-circuit or proceed.
    pub fn check_file_read(
        &mut self,
        path: &str,
        offset: usize,
        limit: usize,
    ) -> DedupAction {
        let path_buf = PathBuf::from(path);
        let key = ReadKey {
            path: path_buf,
            offset,
            limit,
        };

        if let Some(record) = self.records.get_mut(&key) {
            // Verify file hasn't been modified
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(current_mtime) = metadata.modified() {
                    if current_mtime == record.mtime {
                        record.hit_count += 1;
                        // First dedup hit: return short message (saves tokens).
                        // Second+ hit: model may have lost context → allow full re-read.
                        if record.hit_count <= 1 {
                            return DedupAction::ShortCircuit(DedupInfo {
                                path: path.to_string(),
                                total_lines: record.total_lines,
                                start: record.start,
                                end: record.end,
                            });
                        } else {
                            // Allow the read to proceed (context was likely pruned)
                            return DedupAction::Allow;
                        }
                    }
                }
            }
        }

        DedupAction::Allow
    }

    /// Record a completed file_read so future identical calls can be short-circuited.
    pub fn record_file_read(
        &mut self,
        path: &str,
        offset: usize,
        limit: usize,
        total_lines: usize,
        start: usize,
        end: usize,
    ) {
        let path_buf = PathBuf::from(path);

        let mtime = std::fs::metadata(&path_buf)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let key = ReadKey {
            path: path_buf,
            offset,
            limit,
        };

        self.records.insert(
            key,
            ReadRecord {
                mtime,
                total_lines,
                start,
                end,
                hit_count: 0,
            },
        );
    }

    /// Invalidate all records for a specific path (e.g., after file_write or file_update).
    pub fn invalidate_path(&mut self, path: &str) {
        let path_buf = PathBuf::from(path);
        self.records.retain(|key, _| key.path != path_buf);
    }

    /// Reset all dedup state. Call this on new session.
    pub fn reset(&mut self) {
        self.records.clear();
    }

    /// Number of cached dedup entries.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.records.len()
    }
}

impl Default for ToolCallDedup {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DedupAction / DedupInfo — controls what FileRead returns on duplicate calls
// ─────────────────────────────────────────────────────────────────────────────

/// Action to take when a duplicate file_read is detected.
#[derive(Debug)]
pub enum DedupAction {
    /// Proceed with the full read (no duplicate, or stale cache).
    Allow,
    /// Return a short message instead of the full content.
    ShortCircuit(DedupInfo),
}

/// Minimal metadata for a short-circuit response.
#[derive(Debug, Clone)]
pub struct DedupInfo {
    pub path: String,
    pub total_lines: usize,
    pub start: usize,
    pub end: usize,
}

impl DedupInfo {
    /// Format a short message suitable as a tool result.
    pub fn format_message(&self) -> String {
        format!(
            "[DEDUP] File \"{}\" (lines {}-{}, total {} lines) was already read and is in the conversation history above. \
             No need to re-read. If you need a different range, use different offset/limit values. \
             If the content is no longer in context (was pruned), call file_read again to get a fresh copy.",
            self.path, self.start + 1, self.end, self.total_lines,
        )
    }
}
