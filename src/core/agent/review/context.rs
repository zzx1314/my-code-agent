//! Context extraction helpers for code review.
//!
//! Utilities for extracting relevant context from conversation history
//! and formatting it for the review LLM.


/// Clean review content by filtering out auto-review fix prompt lines.
pub(crate) fn clean_review_content(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            !line.contains("fix the issues found in the code review")
                && !line.contains("Auto-Review Iteration")
                && !line.contains("Fix Required")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Detect if a message content is an auto-review fix prompt.
/// Matches the same patterns as `is_auto_fix_prompt` in result.rs
/// (but lives here to avoid circular dependencies).
/// Also matches the broader patterns used by `clean_review_content`
/// to ensure consistency between what gets filtered and what's
/// recognized as a fix prompt.
pub fn is_fix_prompt(content: &str) -> bool {
    content.contains("Code Review - Iteration")
        || content.contains("fix the issues found in the code review")
        || content.contains("Auto-Review Iteration")
        || content.contains("Fix Required")
}



/// Find the largest byte index ≤ `max` that is a valid UTF-8 char boundary.
pub fn char_boundary_at_or_before(s: &str, max: usize) -> usize {
    s.char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Safely truncate a string to at most `max_bytes` bytes, appending "..." if truncated.
/// Never panics on multi-byte UTF-8 characters.
pub fn truncate_content(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let boundary = char_boundary_at_or_before(content, max_bytes);
    let mut s = content[..boundary].to_string();
    s.push_str("...");
    s
}


