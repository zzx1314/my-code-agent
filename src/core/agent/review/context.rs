//! Context extraction helpers for code review.
//!
//! Utilities for extracting relevant context from conversation history
//! and formatting it for the review LLM.

use std::path::Path;

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

/// Extract the main agent's responses following fix prompts.
/// For each fix prompt (user message), finds the next assistant message
/// (the main agent's response) and includes it as "Previous Iteration
/// Feedback" — so the review agent knows which issues were accepted,
/// rejected, or partially fixed.
pub fn extract_previous_iteration_feedback(
    history: &[crate::core::types::Message],
) -> String {
    let mut responses = Vec::new();

    for i in 0..history.len() {
        if history[i].role != "user" || !is_fix_prompt(&history[i].content) {
            continue;
        }

        // Found a fix prompt — look for the next assistant message
        // (the main agent's response to the review)
        for j in (i + 1)..history.len() {
            if history[j].role == "assistant" {
                let content = history[j].content.trim();
                if !content.is_empty() && content.len() > 10 {
                    responses.push(content.to_string());
                }
                break;
            }
        }
    }

    // Deduplicate by content (same response may appear in multiple iterations)
    responses.dedup();

    let mut result = String::new();
    if responses.len() == 1 {
        result.push_str(&truncate_content(&responses[0], 600));
    } else {
        for (i, response) in responses.iter().enumerate() {
            if i > 0 {
                result.push_str("\n---\n");
            }
            result.push_str(&format!("**Iteration {}:** ", i + 1));
            result.push_str(&truncate_content(response, 400));
        }
    }

    result
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

/// Try to read a file from disk and return its structural outline.
/// Returns None if the file can't be read or is not a supported language.
pub fn get_file_outline(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let parsed = crate::core::parser::ParsedFile::parse_with_path(content, file_path)?;
    Some(parsed.get_outline_string())
}
