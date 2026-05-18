//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.

use anyhow::Result;

use super::client::LlmClient;
use crate::core::types::review::*;

/// Code Review Agent
///
/// Reviews code changes by sending diffs directly to the LLM.
/// Does NOT register tools — the LLM should analyze the diffs we provide
/// and return JSON, not call additional tools.
pub struct ReviewAgent {
    pub client: LlmClient,
    pub config: ReviewConfig,
    pub reasoning_field: String,
}

/// Review Request
pub struct ReviewRequest {
    pub changed_files: Vec<ChangedFile>,
    pub context: Option<String>,  // Original task description
}

/// Review response events
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Started { file_count: usize },
    FileAnalyzed { file: String, issues_found: usize },
    Progress { message: String },
    /// Emitted when a review phase completes (used for phased/multi-category review)
    PhaseCompleted {
        phase_index: usize,           // 1-based phase number
        total_phases: usize,          // total number of phases
        phase_name: String,           // e.g. "Core Correctness"
        categories: Vec<String>,      // category names checked in this phase
        issues_found: usize,          // number of issues found
        passed: bool,                 // true if no issues
        details: String,              // brief summary
    },
    /// Reasoning/thinking content from the LLM during review.
    /// Displayed on the frontend but NOT added to conversation history.
    ReasoningDelta(String),
    Completed { report: ReviewReport },
    Error { message: String },
}



impl ReviewAgent {
    pub fn new(client: LlmClient, config: ReviewConfig, reasoning_field: String) -> Self {
        Self { client, config, reasoning_field }
    }

    pub fn system_prompt(&self) -> String {
        let mut prompt = String::from(
            "You are a code review assistant. Review the code changes below and give helpful critical feedback.\n\n"
        );

        prompt.push_str("## Important: Diff Awareness\n\n");
        prompt.push_str("The diff below only shows what CHANGED. It does NOT show the entire file. ");
        prompt.push_str("Existing code outside the diff range is still present and working. ");
        prompt.push_str("Do NOT flag something as missing just because it's absent from the diff ");
        prompt.push_str("— check the user's request and assume existing code still works.\n\n");

        prompt.push_str("## Guidelines\n\n");
        prompt.push_str("- Focus on high-impact issues: bugs, security, missing requirements.\n");
        prompt.push_str("- Make sure ALL requirements in the user's request are addressed — advocate for the user.\n");
        prompt.push_str("- Where a function can be reused, suggest reuse instead of creating new ones.\n");
        prompt.push_str("- Make sure no dead code, missing imports, or accidental deletions are introduced.\n");
        prompt.push_str("- New code should match the style of existing code.\n");
        prompt.push_str("- Try to keep changes minimal — don't rewrite working code.\n");
        prompt.push_str("- Be concise: If you don't have much critical feedback, simply say it looks good.\n");
        prompt.push_str("- Do NOT nitpick style, formatting, or minor preferences.\n");
        prompt.push_str("- Do NOT flag language issues — the user's conversation language is not a review criterion.\n\n");

        prompt.push_str("## Output Format\n\n");
        prompt.push_str("You MUST output ONLY a valid JSON object:\n\n");
        prompt.push_str("```json\n");
        prompt.push_str("{\n");
        prompt.push_str("  \"issues\": [\n");
        prompt.push_str("    {\n");
        prompt.push_str("      \"file\": \"src/example.rs\",\n");
        prompt.push_str("      \"line\": 42,\n");
        prompt.push_str("      \"end_line\": 50,\n");
        prompt.push_str("      \"severity\": \"high\",\n");
        prompt.push_str("      \"category\": \"bug_risk\",\n");
        prompt.push_str("      \"title\": \"Short issue title\",\n");
        prompt.push_str("      \"description\": \"What's wrong and why\",\n");
        prompt.push_str("      \"suggestion\": \"How to fix it\",\n");
        prompt.push_str("      \"code_snippet\": \"Problematic code\",\n");
        prompt.push_str("      \"fix_example\": \"Fixed code\"\n");
        prompt.push_str("    }\n");
        prompt.push_str("  ],\n");
        prompt.push_str("  \"summary\": {\n");
        prompt.push_str("    \"overall_score\": 85,\n");
        prompt.push_str("    \"verdict\": \"approved\"\n");
        prompt.push_str("  }\n");
        prompt.push_str("}\n");
        prompt.push_str("```\n\n");

        prompt.push_str("Severity: \"critical\" | \"high\" | \"medium\" | \"low\" | \"info\"\n");
        prompt.push_str("Category: \"bug_risk\" | \"security\" | \"functional_completeness\" | \"performance\" | \"error_handling\" | \"style\" | \"maintainability\"\n");
        prompt.push_str("Verdict: \"approved\" (no blocking issues) | \"needs_revision\" (fixes required)\n");
        prompt.push_str("Score: 0-100 (higher = better)\n\n");

        prompt.push_str("If there are no issues, simply return:\n");
        prompt.push_str("{\"issues\": [], \"summary\": {\"overall_score\": 100, \"verdict\": \"approved\"}}\n");

        prompt
    }

    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        let changes_summary = self.format_changes_summary(&request.changed_files);
        let user_message = self.build_user_message(&changes_summary, &request.context);
        let (response, _reasoning) = self.call_llm(&user_message).await?;
        let issues = self.parse_issues_from_response(&response)?;
        self.build_report(&issues, &request.changed_files)
    }

    pub async fn review_with_events(
        &self,
        request: &ReviewRequest,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let file_count = request.changed_files.len();
        let _ = event_tx.send(ReviewEvent::Started { file_count });

        let _ = event_tx.send(ReviewEvent::Progress {
            message: "Reviewing code changes...".to_string(),
        });

        let changes_summary = self.format_changes_summary(&request.changed_files);
        let user_message = self.build_user_message(&changes_summary, &request.context);

        let response = self.call_llm_stream(&user_message, &event_tx).await?;
        let issues = self.parse_issues_from_response(&response)?;
        let report = self.build_report(&issues, &request.changed_files)?;

        let _ = event_tx.send(ReviewEvent::Completed {
            report: report.clone(),
        });

        Ok(report)
    }

    fn build_user_message(&self, changes_summary: &str, context: &Option<String>) -> String {
        if let Some(ctx) = context {
            format!(
                "## User Request (Requirements)\n\n{ctx}\n\n## Code Changes to Review\n\n{changes_summary}",
                ctx = ctx,
                changes_summary = changes_summary,
            )
        } else {
            format!(
                "Please review the following code changes:\n\n{changes_summary}",
                changes_summary = changes_summary,
            )
        }
    }

    async fn call_llm(&self, user_message: &str) -> Result<(String, String)> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let response = self.client.chat(&messages, &[], &self.reasoning_field).await?;
        let message = &response["choices"][0]["message"];

        let content = message["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No content in review response"))?
            .to_string();

        let reasoning = message.get("reasoning_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok((content, reasoning))
    }

    /// Call LLM with streaming, sending reasoning deltas via event_tx in real-time.
    async fn call_llm_stream(
        &self,
        user_message: &str,
        event_tx: &tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<String> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let mut chat_stream = self.client.stream_chat(&messages, &[], &self.reasoning_field).await?;
        let mut full_content = String::new();

        while let Some(chunk_result) = chat_stream.next().await {
            let chunk = chunk_result?;
            for choice in &chunk.choices {
                let delta = &choice.delta;

                if let Some(ref rt) = delta.reasoning_content {
                    if !rt.is_empty() {
                        let _ = event_tx.send(ReviewEvent::ReasoningDelta(rt.clone()));
                    }
                } else if let Some(ref rt) = delta.reasoning {
                    if !rt.is_empty() {
                        let _ = event_tx.send(ReviewEvent::ReasoningDelta(rt.clone()));
                    }
                }

                if let Some(ref text) = delta.content {
                    if !text.is_empty() {
                        full_content.push_str(text);
                    }
                }
            }
        }

        Ok(full_content)
    }

    /// Extract user's original request from conversation history for review context.
    ///
    /// Uses an improved strategy:
    /// 1. Takes the FIRST user message (original request) — the most important context
    /// 2. Includes the LAST assistant message before the review (what was implemented),
    ///    UNLESS it's a response to an auto-review fix prompt (that goes in
    ///    the "Previous Iteration Feedback" section instead)
    /// 3. Includes the most recent follow-up user message (if any substantial one exists)
    /// 4. Adds the main agent's responses from previous auto-review iterations as
    ///    "Previous Iteration Feedback" — so the review agent knows which
    ///    issues were accepted, rejected, or partially fixed
    /// 5. Caps total context at ~2000 characters
    pub fn extract_context_from_history(history: &[crate::core::types::Message]) -> String {
        let mut result = String::new();

        // Find positions of first user and last assistant messages
        let first_user_idx = history.iter().position(|m| m.role == "user");
        let last_assistant_idx = history.iter().rposition(|m| m.role == "assistant");

        // 1. Always include the first user message (original request)
        if let Some(idx) = first_user_idx {
            let content = clean_review_content(&history[idx].content);
            if !content.is_empty() {
                result.push_str("## Original Request\n");
                result.push_str(&truncate_content(&content, 1500));
                result.push_str("\n\n");
            }
        }

        // 2. Include last assistant message summary (what was implemented).
        //    Skip if it follows a fix prompt — the agent's response to the
        //    review belongs in the "Previous Iteration Feedback" section instead.
        if let Some(idx) = last_assistant_idx {
            let follows_fix_prompt = idx > 0
                && history[idx - 1].role == "user"
                && is_fix_prompt(&history[idx - 1].content);

            if !follows_fix_prompt {
                let content = clean_review_content(&history[idx].content);
                if !content.is_empty() {
                    result.push_str("## What Was Implemented\n");
                    result.push_str(&truncate_content(&content, 1000));
                    result.push_str("\n\n");
                }
            }
        }

        // 3. If there are follow-up user messages between first and last assistant,
        //    include the most recent substantial one
        if let (Some(first_idx), Some(last_idx)) = (first_user_idx, last_assistant_idx) {
            for i in (first_idx + 1..last_idx).rev() {
                if history[i].role == "user" {
                    let content = clean_review_content(&history[i].content);
                    if !content.is_empty() && content.len() > 20 {
                        result.push_str("## Follow-up Context\n");
                        result.push_str(&truncate_content(&content, 500));
                        result.push_str("\n\n");
                        break;
                    }
                }
            }
        }

        // 4. Add previous iteration feedback: the main agent's response(s) to
        //    fix prompts from previous review iterations. This tells the review
        //    agent which issues the main agent accepted/rejected/partially fixed.
        let agent_feedback = extract_previous_iteration_feedback(history);
        if !agent_feedback.is_empty() {
            result.push_str("## Previous Iteration Feedback\n");
            result.push_str("The main agent's response to the previous code review:\n");
            result.push_str(&agent_feedback);
            result.push_str("\n\n");
        }

        // 5. Cap at 2000 characters (safely at UTF-8 char boundaries)
        if result.len() > 2000 {
            let boundary = char_boundary_at_or_before(&result, 2000);
            result.truncate(boundary);
            // Try to break at a newline for cleaner appearance
            let search_end = char_boundary_at_or_before(&result, 1997.min(result.len()));
            if let Some(last_newline) = result[..search_end].rfind('\n') {
                result.truncate(last_newline + 1);
            }
        }

        if result.is_empty() {
            // Fallback: return last user message
            if let Some(msg) = history.iter().rev().find(|m| m.role == "user") {
                result = clean_review_content(&msg.content);
            }
        }

        result
    }

    pub fn format_changes_summary(&self, files: &[ChangedFile]) -> String {
        let mut summary = String::new();
        summary.push_str(&format!("## Changed Files ({})\n\n", files.len()));

        for file in files {
            let change_type_str = match file.change_type {
                ChangeType::Added => "Added",
                ChangeType::Modified => "Modified",
                ChangeType::Deleted => "Deleted",
                ChangeType::Renamed => "Renamed",
            };
            summary.push_str(&format!(
                "### {} ({})\n",
                file.path, change_type_str,
            ));
            summary.push_str(&format!(
                "- +{} lines, -{} lines\n",
                file.lines_added, file.lines_removed
            ));

            if !file.diff.is_empty() {
                summary.push_str("```diff\n");
                summary.push_str(&file.diff);
                summary.push_str("\n```\n");
            }

            summary.push_str("\n");
        }

        summary
    }



    /// Parse review response — full pipeline: extract JSON, parse issues, build report.
    /// (Kept for backward compatibility with tests.)
    pub fn parse_review_response(
        &self,
        response: &str,
        changed_files: &[ChangedFile],
    ) -> Result<ReviewReport> {
        let issues = self.parse_issues_from_response(response)?;
        self.build_report(&issues, changed_files)
    }

    /// Extract issues from a JSON review response (without building the full report).
    /// Returns the raw list of ReviewIssue structs.
    fn parse_issues_from_response(&self, response: &str) -> Result<Vec<ReviewIssue>> {
        let json_str = self.extract_json(response)?;

        // Guard: empty or whitespace-only JSON means no issues to report.
        // This handles cases where the LLM output an empty code block
        // (e.g. ```json followed immediately by ```) or other edge cases
        // that result in an empty extracted string.
        if json_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let sanitized = sanitize_json_escapes(&json_str);
        let parsed: serde_json::Value = match serde_json::from_str(&sanitized) {
            Ok(v) => v,
            Err(_) => {
                // Attempt JSON repair for truncated/malformed responses
                let repaired = repair_truncated_json(&sanitized);
                serde_json::from_str(&repaired)?
            }
        };

        let mut issues = Vec::new();

        if let Some(issues_array) = parsed.get("issues").and_then(|v| v.as_array()) {
            for issue in issues_array {
                let severity = match issue.get("severity").and_then(|v| v.as_str()) {
                    Some("critical") => Severity::Critical,
                    Some("high") => Severity::High,
                    Some("medium") => Severity::Medium,
                    Some("low") => Severity::Low,
                    _ => Severity::Info,
                };

                let category = match issue.get("category").and_then(|v| v.as_str()) {
                    Some("functional_completeness") => ReviewCategory::FunctionalCompleteness,
                    Some("security") => ReviewCategory::Security,
                    Some("performance") => ReviewCategory::Performance,
                    Some("bug_risk") => ReviewCategory::BugRisk,
                    Some("style") => ReviewCategory::Style,
                    Some("maintainability") => ReviewCategory::Maintainability,
                    Some("error_handling") => ReviewCategory::ErrorHandling,
                    Some("concurrency") => ReviewCategory::Concurrency,
                    _ => ReviewCategory::Maintainability,
                };

                issues.push(ReviewIssue {
                    file: issue.get("file").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    line: issue.get("line").and_then(|v| v.as_u64()).map(|v| v as usize),
                    end_line: issue.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize),
                    severity,
                    category,
                    title: issue.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: issue.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    suggestion: issue.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    code_snippet: issue.get("code_snippet").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    fix_example: issue.get("fix_example").and_then(|v| v.as_str()).map(|s| s.to_string()),
                });
            }
        }

        Ok(issues)
    }

    /// Build a complete ReviewReport from a list of issues and changed files.
    /// Calculates summary statistics, verdict, and auto-fixable list.
    ///
    /// This is a public wrapper used after post-processing (e.g., fingerprint
    /// deduplication) has filtered some issues. Delegates to the shared
    /// `build_report_inner` logic.
    pub fn rebuild_report(&self, issues: &[ReviewIssue], changed_files: &[ChangedFile]) -> ReviewReport {
        self.build_report_inner(issues, changed_files)
    }

    /// Build a complete ReviewReport from a list of issues and changed files.
    /// This wrapper exists for callers that use `?` (Result-returning).
    /// Delegates to the shared `build_report_inner` logic.
    fn build_report(&self, issues: &[ReviewIssue], changed_files: &[ChangedFile]) -> Result<ReviewReport> {
        Ok(self.build_report_inner(issues, changed_files))
    }

    /// Shared internal logic for building a ReviewReport from issues and changed files.
    /// Calculates summary statistics, verdict, overall score, and auto-fixable list.
    fn build_report_inner(&self, issues: &[ReviewIssue], changed_files: &[ChangedFile]) -> ReviewReport {
        let critical_count = issues.iter().filter(|i| i.severity == Severity::Critical).count();
        let high_count = issues.iter().filter(|i| i.severity == Severity::High).count();
        let medium_count = issues.iter().filter(|i| i.severity == Severity::Medium).count();
        let low_count = issues.iter().filter(|i| i.severity == Severity::Low).count();
        let info_count = issues.iter().filter(|i| i.severity == Severity::Info).count();

        let has_functional_completeness_issues = issues.iter().any(|i| matches!(i.category, ReviewCategory::FunctionalCompleteness));
        let has_blocking_issues = critical_count > 0
            || high_count > 0
            || medium_count > 0
            || has_functional_completeness_issues;

        let verdict = if has_blocking_issues || !issues.is_empty() {
            if critical_count > 0 && has_functional_completeness_issues {
                ReviewVerdict::Rejected
            } else if has_blocking_issues {
                ReviewVerdict::NeedsRevision
            } else {
                ReviewVerdict::Approved
            }
        } else {
            ReviewVerdict::Approved
        };

        let overall_score = if issues.is_empty() {
            100.0
        } else {
            let penalty = (critical_count * 25 + high_count * 10 + medium_count * 5 + low_count * 2 + info_count) as f64;
            (100.0 - penalty).max(0.0)
        };

        let auto_fixable: Vec<ReviewIssue> = issues
            .iter()
            .filter(|i| i.fix_example.is_some())
            .cloned()
            .collect();

        ReviewReport {
            summary: ReviewSummary {
                total_issues: issues.len(),
                critical_count,
                high_count,
                medium_count,
                low_count,
                info_count,
                overall_score,
                verdict,
            },
            issues: issues.to_vec(),
            changed_files: changed_files.to_vec(),
            metrics: CodeMetrics {
                files_changed: changed_files.len(),
                total_lines_added: changed_files.iter().map(|f| f.lines_added).sum(),
                total_lines_removed: changed_files.iter().map(|f| f.lines_removed).sum(),
                complexity_estimate: None,
            },
            auto_fixable,
        }
    }

    /// Extract JSON from response
    fn extract_json(&self, response: &str) -> Result<String> {
        extract_json_from_response(response)
    }

}

/// Extract a JSON object from an LLM response string.
///
/// Handles multiple formats:
/// 1. ```json ... ``` code blocks
/// 2. ``` ... ``` code blocks (looks for JSON-like content inside)
/// 3. Bare `{...}` objects using brace counting (handles nesting)
/// 4. If the entire string is valid JSON, returns it directly
pub fn extract_json_from_response(response: &str) -> Result<String> {
    let response = response.trim();

    // Strategy 1: Try ```json ... ``` code block (most common with LLMs)
    if let Some(start) = response.find("```json") {
        let json_start = start + 7;
        if let Some(end) = response[json_start..].find("```") {
            return Ok(response[json_start..json_start + end].trim().to_string());
        }
    }

    // Strategy 2: Try ``` ... ``` code block without language specifier
    if let Some(start) = response.rfind("```") {
        let before = &response[..start];
        // Find matching opening ```
        if let Some(open) = before.rfind("```") {
            let inner = response[open + 3..start].trim();
            // Check if it looks like JSON (starts with { or [)
            if inner.starts_with('{') || inner.starts_with('[') {
                return Ok(inner.to_string());
            }
        }
    }

    // Strategy 3: Find outermost { ... } pair using string-aware brace counting
    // This handles nested braces properly and skips braces inside string literals.
    //
    // NOTE: `find('{')` returns a byte index, so we use `char_indices()` which
    // also returns byte indices (not `.chars().enumerate()` which returns char
    // indices). This is critical for correctness when multi-byte Unicode
    // characters (emoji, CJK, etc.) appear before the first `{`.
    if let Some(start) = response.find('{') {
        let mut depth = 0_i64;
        let mut json_start = None;
        let mut json_end = None;
        let mut in_string = false;
        let mut prev_was_escape = false;
        for (byte_i, ch) in response[start..].char_indices() {
            let i = start + byte_i;
            // Track string boundaries to skip braces inside strings
            if ch == '"' && !prev_was_escape {
                in_string = !in_string;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            if in_string { continue; }
            match ch {
                '{' => {
                    if depth == 0 {
                        json_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        json_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let (Some(js), Some(je)) = (json_start, json_end) {
            if je > js {
                return Ok(response[js..=je].to_string());
            }
        }
    }

    // Strategy 4: If response is itself valid JSON, return it
    if response.starts_with('{') && response.ends_with('}') {
        if serde_json::from_str::<serde_json::Value>(response).is_ok() {
            return Ok(response.to_string());
        }
    }

    anyhow::bail!(
        "Unable to extract JSON from response. Response was:\n{}",
        response.chars().take(500).collect::<String>()
    )
}

/// Sanitize a JSON string by fixing invalid escape sequences.
///
/// LLMs frequently produce JSON with invalid escapes like `\x`, `\uGGGG`,
/// or unescaped backslashes in Windows paths (`C:\Users\test`).
/// This function converts invalid `\X` → `\\X` so that `serde_json::from_str`
/// can parse the result without error.
///
/// Valid JSON escapes (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`)
/// are left untouched.
pub fn sanitize_json_escapes(json: &str) -> String {
    let mut result = String::with_capacity(json.len() + json.len() / 20);
    let mut chars = json.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }
        if !in_string || ch != '\\' {
            result.push(ch);
            continue;
        }

        // We're inside a string and just saw a backslash — check what follows
        let Some(next) = chars.peek() else {
            // Trailing backslash at end of string — escape it
            result.push_str("\\\\");
            break;
        };

        match next {
            // Valid JSON escape sequences — keep as-is
            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                result.push(ch);
                result.push(chars.next().unwrap());
            }
            // Unicode escape: must be \u followed by exactly 4 hex digits
            'u' => {
                // Peek ahead to check 4 hex digits
                let mut hex_valid = true;
                let mut hex_chars = Vec::new();
                for _ in 0..4 {
                    chars.next(); // consume 'u' on first iteration
                    if let Some(h) = chars.peek() {
                        if h.is_ascii_hexdigit() {
                            hex_chars.push(*h);
                        } else {
                            hex_valid = false;
                            break;
                        }
                    } else {
                        hex_valid = false;
                        break;
                    }
                }
                if hex_valid {
                    // Valid \uXXXX — keep entire sequence
                    result.push('\\');
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                } else {
                    // Invalid unicode escape — treat backslash as literal
                    result.push_str("\\\\");
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                }
            }
            // Invalid escape — double the backslash to make it literal
            _ => {
                result.push_str("\\\\");
                result.push(chars.next().unwrap());
            }
        }
    }

    result
}

fn clean_review_content(content: &str) -> String {
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
fn is_fix_prompt(content: &str) -> bool {
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
fn extract_previous_iteration_feedback(history: &[crate::core::types::Message]) -> String {
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
fn char_boundary_at_or_before(s: &str, max: usize) -> usize {
    s.char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Safely truncate a string to at most `max_bytes` bytes, appending "..." if truncated.
/// Never panics on multi-byte UTF-8 characters.
fn truncate_content(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let boundary = char_boundary_at_or_before(content, max_bytes);
    let mut s = content[..boundary].to_string();
    s.push_str("...");
    s
}

/// Attempt to repair a truncated/malformed JSON string from LLM output.
///
/// LLM responses sometimes get cut off mid-JSON (typically hitting output token limits).
/// This function handles the common truncation patterns:
/// 1. Trailing comma before closing bracket (`...,` → `...}`)
/// 2. Unclosed string literals (appends `"`)
/// 3. Unbalanced braces `{}` and brackets `[]` (appends missing closers)
///
/// Uses proper string-aware tracking to avoid misinterpreting
/// braces/brackets inside string values.
pub fn repair_truncated_json(s: &str) -> String {
    let mut result = s.trim_end().to_string();

    if let Some('}' | ']') = result.chars().last() {
        let closer = result.pop().unwrap();
        result = result.trim_end_matches(',').to_string();
        result.push(closer);
    }

    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut stack: Vec<char> = Vec::new();

    for ch in result.chars() {
        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if in_string {
            continue;
        }

        match ch {
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if let Some(&top) = stack.last() {
                    if top == ch {
                        stack.pop();
                    }
                }
            }
            _ => {}
        }
    }

    if in_string {
        result.push('"');
    }

    while let Some(closing) = stack.pop() {
        result.push(closing);
    }

    result
}
