//! Deterministic code structure checks for code review.
//!
//! Checks that don't require an LLM call: file length limits.

use std::path::Path;

/// Check if a file exceeds the maximum line threshold.
pub fn check_file_too_long(path: &Path, max_lines: usize) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let line_count = content.lines().count();
    line_count > max_lines
}


