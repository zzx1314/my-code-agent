//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.

use anyhow::Result;
use std::path::Path;

use super::client::LlmClient;
use crate::core::parser::ParsedFile;
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
    /// How thinking/reasoning content should be displayed to the user.
    /// "streaming" | "collapsed" | "hidden"
    pub thinking_display: String,
}

/// Review Request
pub struct ReviewRequest {
    pub changed_files: Vec<ChangedFile>,
    pub context: Option<String>,         // Original task description
    pub history_summary: Option<String>, // Conversation history summary for consistency checking
}

/// Review response events
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Started {
        file_count: usize,
    },
    FileAnalyzed {
        file: String,
        issues_found: usize,
    },
    Progress {
        message: String,
    },
    /// Emitted when a review phase completes (used for phased/multi-category review)
    PhaseCompleted {
        phase_index: usize,      // 1-based phase number
        total_phases: usize,     // total number of phases
        phase_name: String,      // e.g. "Core Correctness"
        categories: Vec<String>, // category names checked in this phase
        issues_found: usize,     // number of issues found
        passed: bool,            // true if no issues
        details: String,         // brief summary
    },
    /// Reasoning/thinking content from the LLM during review.
    /// Displayed on the frontend but NOT added to conversation history.
    ReasoningDelta(String),
    Completed {
        report: ReviewReport,
    },
    Error {
        message: String,
    },
}

impl ReviewAgent {
    pub fn new(
        client: LlmClient,
        config: ReviewConfig,
        reasoning_field: String,
        thinking_display: String,
    ) -> Self {
        Self {
            client,
            config,
            reasoning_field,
            thinking_display,
        }
    }

    pub fn system_prompt(&self) -> String {
        concat!(
            "You are a focused code review assistant. Review the code changes below.\n\n",
            "## Your ONLY Job\n\n",
            "Check ONLY these two things:\n\n",
            "1. **Functional Completeness** — Does the code fulfill ALL the user's requirements?\n",
            "   Advocate for the user. If a requested feature is missing or incomplete, flag it.\n\n",
            "2. **Obvious Bugs** — Logic errors, edge cases not handled, incorrect API usage,\n",
            "   wrong algorithm. Only if you're CONFIDENT it's a real bug.\n\n",
            "## Rules\n\n",
            "- The diff only shows what CHANGED. Code outside the diff is still there.\n",
            "- Do NOT flag something as \"missing\" just because it's not in the diff.\n",
            "- Do NOT report: imports, types, style, dead code, naming, perf, concurrency,\n",
            "  security, error handling — these are covered by compiler, linter, or tests.\n",
            "- Be concise. If nothing is wrong, just say so.\n",
            "- Only report issues you are CONFIDENT about. Never speculate.\n",
            "- Every claim must be directly verifiable from the provided diff.\n\n",
            "## Output Format\n\n",
            "Return ONLY a valid JSON object:\n\n",
            "```json\n",
            "{\n",
            "  \"issues\": [\n",
            "    {\n",
            "      \"file\": \"src/example.rs\",\n",
            "      \"line\": 42,\n",
            "      \"severity\": \"high\",\n",
            "      \"category\": \"functional_completeness\" or \"bug_risk\",\n",
            "      \"title\": \"Short title\",\n",
            "      \"description\": \"What's wrong and why\",\n",
            "      \"suggestion\": \"How to fix it\"\n",
            "    }\n",
            "  ],\n",
            "  \"summary\": {\n",
            "    \"verdict\": \"approved\" or \"needs_revision\"\n",
            "  }\n",
            "}\n",
            "```\n\n",
            "Severity: \"critical\" (crash/data loss), \"high\" (wrong behavior),\n",
            "\"medium\" (potential bug), \"low\" (minor).\n\n",
            "Verdict: \"approved\" (no issues or only low), \"needs_revision\" (medium+ issues).\n\n",
            "If no issues, return:\n",
            "{\"issues\": [], \"summary\": {\"verdict\": \"approved\"}}\n",
        ).to_string()
    }

    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        let changes_summary = self.format_changes_summary(&request.changed_files);
        let user_message =
            self.build_user_message(&changes_summary, &request.context, &request.history_summary);
        let (response, _reasoning) = self.call_llm(&user_message).await?;
        let issues = self.parse_issues_from_response(&response)?;
        let filtered = filter_known_false_positives(&issues);
        let filtered_count = issues.len() - filtered.len();
        if filtered_count > 0 {
            tracing::info!(
                filtered_count,
                "Pre-filter removed known false-positive issues"
            );
        }
        // Add deterministic structural checks (file length, test existence)
        let structural = self.check_code_structure(&request.changed_files);
        let all_issues = [filtered.as_slice(), structural.as_slice()].concat();
        self.build_report(&all_issues, &request.changed_files)
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
        let user_message =
            self.build_user_message(&changes_summary, &request.context, &request.history_summary);

        let response = self.call_llm_stream(&user_message, &event_tx).await?;
        let issues = self.parse_issues_from_response(&response)?;
        let filtered = filter_known_false_positives(&issues);
        let filtered_count = issues.len() - filtered.len();
        if filtered_count > 0 {
            tracing::info!(
                filtered_count,
                "Pre-filter removed known false-positive issues"
            );
        }
        // Add deterministic structural checks (file length, test existence)
        let _ = event_tx.send(ReviewEvent::Progress {
            message: "Checking code structure and test coverage...".to_string(),
        });
        let structural = self.check_code_structure(&request.changed_files);
        let all_issues = [filtered.as_slice(), structural.as_slice()].concat();
        let report = self.build_report(&all_issues, &request.changed_files)?;

        let _ = event_tx.send(ReviewEvent::Completed {
            report: report.clone(),
        });

        Ok(report)
    }

    fn build_user_message(
        &self,
        changes_summary: &str,
        context: &Option<String>,
        history_summary: &Option<String>,
    ) -> String {
        let mut msg = String::new();

        if let Some(ctx) = context {
            msg.push_str(&format!("## User Request (Requirements)\n\n{ctx}\n\n"));
        }

        if let Some(history) = history_summary {
            msg.push_str(&format!("## Conversation History Summary\n\n{history}\n\n"));
            msg.push_str("**Consistency Check**: Please verify the implementation matches what was discussed in the conversation. Flag any contradictions, missed requirements, or deviations from the agreed approach.\n\n");
        }

        msg.push_str(&format!("## Code Changes to Review\n\n{changes_summary}"));
        msg
    }

    async fn call_llm(&self, user_message: &str) -> Result<(String, String)> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let response = self
            .client
            .chat(&messages, &[], &self.reasoning_field)
            .await?;
        let message = &response["choices"][0]["message"];

        let content = message["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No content in review response"))?
            .to_string();

        let reasoning = message
            .get("reasoning_content")
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
        use crate::ui::render::StatefulTagStripper;
        let mut tag_stripper = StatefulTagStripper::new();
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let mut chat_stream = self
            .client
            .stream_chat(&messages, &[], &self.reasoning_field)
            .await?;
        let mut full_content = String::new();

        // Reasoning accumulation buffer for full-vs-incremental dedup.
        // Some API providers send FULL accumulated reasoning_content in each
        // SSE chunk; others send incremental deltas.
        let mut reasoning_buf = String::new();

        while let Some(chunk_result) = chat_stream.next().await {
            let chunk = chunk_result?;
            for choice in &chunk.choices {
                let delta = &choice.delta;

                // Process reasoning_content or reasoning field
                let reasoning_text = delta
                    .reasoning_content
                    .as_ref()
                    .or_else(|| delta.reasoning.as_ref());
                if let Some(rt) = reasoning_text {
                    if !rt.is_empty() && self.thinking_display != "hidden" {
                        // Strip HTML/XML tags (e.g., <think>, </think>)
                        // with cross-chunk state tracking.
                        let cleaned = tag_stripper.process(rt);

                        // Handle full vs incremental reasoning delta:
                        // If cleaned starts with what we already have, it's a
                        // full-accumulation response — send only the new portion.
                        // Otherwise it's incremental — send as-is.
                        if cleaned.starts_with(reasoning_buf.as_str()) {
                            let delta_text = &cleaned[reasoning_buf.len()..];
                            if !delta_text.is_empty() {
                                let _ = event_tx
                                    .send(ReviewEvent::ReasoningDelta(delta_text.to_string()));
                            }
                            reasoning_buf = cleaned;
                        } else {
                            reasoning_buf.push_str(&cleaned);
                            let _ = event_tx.send(ReviewEvent::ReasoningDelta(cleaned));
                        }
                    }
                }

                if let Some(ref text) = delta.content {
                    if !text.is_empty() {
                        let cleaned = tag_stripper.process(text);
                        full_content.push_str(&cleaned);
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

    /// Extract conversation history summary for consistency checking.
    ///
    /// Creates a structured summary of the conversation that allows the review agent
    /// to verify if the implementation matches what was discussed. Focuses on:
    /// 1. User requirements and preferences expressed during conversation
    /// 2. Technical decisions and agreements made
    /// 3. Features/changes explicitly requested
    /// 4. Constraints or limitations mentioned
    ///
    /// Returns None if history is too short or contains no useful information.
    pub fn extract_history_summary(history: &[crate::core::types::Message]) -> Option<String> {
        if history.len() < 3 {
            return None;
        }

        let mut requirements = Vec::new();
        let mut decisions = Vec::new();
        let mut features = Vec::new();

        for msg in history.iter() {
            if msg.role != "user" {
                continue;
            }

            let content = msg.content.trim();
            if content.is_empty() || content.len() < 10 {
                continue;
            }

            // Skip fix prompts and auto-review messages
            if is_fix_prompt(content) {
                continue;
            }

            // Extract key points from user messages
            let lower = content.to_lowercase();

            // Detect requirements (keywords like "need", "want", "should", "must", "require")
            if lower.contains("need")
                || lower.contains("want")
                || lower.contains("should")
                || lower.contains("must")
                || lower.contains("require")
                || lower.contains("please")
            {
                let summary = truncate_content(content, 200);
                if !summary.is_empty() {
                    requirements.push(format!("- {}", summary));
                }
            }

            // Detect feature requests (keywords like "add", "implement", "create", "build")
            if lower.contains("add")
                || lower.contains("implement")
                || lower.contains("create")
                || lower.contains("build")
                || lower.contains("support")
                || lower.contains("feature")
            {
                let summary = truncate_content(content, 150);
                if !summary.is_empty() {
                    features.push(format!("- {}", summary));
                }
            }

            // Detect decisions/constraints (keywords like "use", "choose", "prefer", "instead", "must not")
            if lower.contains("use ")
                || lower.contains("choose")
                || lower.contains("prefer")
                || lower.contains("instead")
                || lower.contains("must not")
                || lower.contains("don't")
                || lower.contains("avoid")
            {
                let summary = truncate_content(content, 150);
                if !summary.is_empty() {
                    decisions.push(format!("- {}", summary));
                }
            }
        }

        let mut summary = String::new();

        if !requirements.is_empty() {
            summary.push_str("**User Requirements:**\n");
            // Take up to 5 most recent requirements
            let start = requirements.len().saturating_sub(5);
            for req in &requirements[start..] {
                summary.push_str(&format!("{}\n", req));
            }
            summary.push_str("\n");
        }

        if !features.is_empty() {
            summary.push_str("**Requested Features:**\n");
            let start = features.len().saturating_sub(5);
            for feat in &features[start..] {
                summary.push_str(&format!("{}\n", feat));
            }
            summary.push_str("\n");
        }

        if !decisions.is_empty() {
            summary.push_str("**Technical Decisions/Constraints:**\n");
            let start = decisions.len().saturating_sub(3);
            for dec in &decisions[start..] {
                summary.push_str(&format!("{}\n", dec));
            }
            summary.push_str("\n");
        }

        if summary.is_empty() {
            None
        } else {
            Some(summary)
        }
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
            summary.push_str(&format!("### {} ({})\n", file.path, change_type_str,));
            summary.push_str(&format!(
                "- +{} lines, -{} lines\n",
                file.lines_added, file.lines_removed
            ));

            // Include file outline for context (skip deleted files)
            if file.change_type != ChangeType::Deleted {
                if let Some(outline) = get_file_outline(&file.path) {
                    summary.push_str("**File Outline:**\n");
                    summary.push_str("```\n");
                    summary.push_str(&outline);
                    summary.push_str("\n```\n");
                }
            }

            if !file.diff.is_empty() {
                summary.push_str(&format!("**Diff:**\n"));
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
        let parsed = parse_json_with_fallback(&sanitized).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse review JSON after all repair strategies: {}",
                e
            )
        })?;

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
                    Some("bug_risk") => ReviewCategory::BugRisk,
                    // The review agent's system prompt limits LLM output to the two
                    // categories above. Other categories (security, performance, etc.)
                    // are supported in the enum for display/config but not produced
                    // by the current review pipeline.
                    _ => ReviewCategory::Maintainability,
                };

                issues.push(ReviewIssue {
                    file: issue
                        .get("file")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    line: issue
                        .get("line")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize),
                    end_line: issue
                        .get("end_line")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize),
                    severity,
                    category,
                    title: issue
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: issue
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    suggestion: issue
                        .get("suggestion")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    code_snippet: issue
                        .get("code_snippet")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    fix_example: issue
                        .get("fix_example")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
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
    pub fn rebuild_report(
        &self,
        issues: &[ReviewIssue],
        changed_files: &[ChangedFile],
    ) -> ReviewReport {
        self.build_report_inner(issues, changed_files)
    }

    /// Build a complete ReviewReport from a list of issues and changed files.
    /// This wrapper exists for callers that use `?` (Result-returning).
    /// Delegates to the shared `build_report_inner` logic.
    fn build_report(
        &self,
        issues: &[ReviewIssue],
        changed_files: &[ChangedFile],
    ) -> Result<ReviewReport> {
        Ok(self.build_report_inner(issues, changed_files))
    }

    /// Shared internal logic for building a ReviewReport from issues and changed files.
    /// Calculates summary statistics, verdict, overall score, and auto-fixable list.
    fn build_report_inner(
        &self,
        issues: &[ReviewIssue],
        changed_files: &[ChangedFile],
    ) -> ReviewReport {
        // Single-pass counting: 6 traversals → 1 for all severity levels + auto-fixable.
        let mut critical_count = 0;
        let mut high_count = 0;
        let mut medium_count = 0;
        let mut low_count = 0;
        let mut info_count = 0;
        let mut auto_fixable = Vec::new();

        for issue in issues {
            match issue.severity {
                Severity::Critical => critical_count += 1,
                Severity::High => high_count += 1,
                Severity::Medium => medium_count += 1,
                Severity::Low => low_count += 1,
                Severity::Info => info_count += 1,
            }
            if issue.fix_example.is_some() {
                auto_fixable.push(issue.clone());
            }
        }

        // Verdict: needs_revision if there are any medium+ issues
        let has_medium_or_above = critical_count > 0 || high_count > 0 || medium_count > 0;
        let verdict = if has_medium_or_above {
            ReviewVerdict::NeedsRevision
        } else {
            ReviewVerdict::Approved
        };

        ReviewReport {
            summary: ReviewSummary {
                total_issues: issues.len(),
                critical_count,
                high_count,
                medium_count,
                low_count,
                info_count,
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

    /// Perform deterministic code structure checks that don't require an LLM.
    ///
    /// Checks two things for each non-deleted changed file:
    /// 1. **File length** — if the file exceeds `max_file_lines`, suggests splitting
    /// 2. **Test existence** — if the source file lacks inline tests or a corresponding
    ///    test file in the `tests/` directory
    pub fn check_code_structure(&self, files: &[ChangedFile]) -> Vec<ReviewIssue> {
        let mut issues = Vec::new();

        for file in files {
            if file.change_type == ChangeType::Deleted {
                continue;
            }

            let path = Path::new(&file.path);

            // 1. Check file length
            if let Some(max_lines) = self.config.max_file_lines {
                if check_file_too_long(path, max_lines) {
                    issues.push(ReviewIssue {
                        file: file.path.clone(),
                        line: Some(max_lines + 1),
                        end_line: None,
                        severity: Severity::Medium,
                        category: ReviewCategory::Maintainability,
                        title: "File too long, consider splitting by function".to_string(),
                        description: format!(
                            "File `{}` exceeds the recommended {} line limit. Long files hurt readability and maintainability. Consider splitting into smaller files by functional responsibility.",
                            file.path, max_lines
                        ),
                        suggestion: Some(format!(
                            "Extract different concerns from `{}` into separate files. For example, create one file per major function, each focused on a single responsibility.",
                            file.path
                        )),
                        code_snippet: None,
                        fix_example: None,
                    });
                }
            }

            // 2. Check test existence (only for source files, skip test/config files)
            if is_source_file(path) && !has_tests(path) {
                let filename = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                issues.push(ReviewIssue {
                    file: file.path.clone(),
                    line: None,
                    end_line: None,
                    severity: Severity::Low,
                    category: ReviewCategory::Maintainability,
                    title: "Missing test coverage".to_string(),
                    description: format!(
                        "File `{}` has no corresponding test file or inline tests. Please add test coverage for the new functionality.",
                        file.path
                    ),
                    suggestion: Some(format!(
                        "Create a test file `tests/test_{}.rs` in the `tests/` directory, or add an inline `#[cfg(test)]\n    mod tests {{ ... }}` module at the end of the file.",
                        filename
                    )),
                    code_snippet: None,
                    fix_example: None,
                });
            }
        }

        issues
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
            if in_string {
                continue;
            }
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

/// Try to read a file from disk and return its structural outline.
/// Returns None if the file can't be read or is not a supported language.
fn get_file_outline(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let parsed = ParsedFile::parse_with_path(content, file_path)?;
    Some(parsed.get_outline_string())
}

/// Filter out known false-positive review issues using deterministic rules.
///
/// Before sending issues to `build_report`, this function checks each issue
/// against a set of regex-like patterns that match common false-positive
/// signals from the review LLM. Patterns are grouped by category:
///
/// 1. **`block_in_place` / async panic** — LLMs see `tokio::task::block_in_place`
///    and claim it will panic, but this is a valid tokio utility.
/// 2. **`block_on` convention** — `futures::executor::block_on` is used deliberately
///    throughout this project; it's not a bug.
/// 3. **"Silent fallback" / "silently"** — LLM flags intentional fallback/default
///    patterns as error-masking, even when the fallback is correct.
/// 4. **Generic "consider error handling"** — Vague, non-actionable suggestions
///    without a specific scenario.
/// 5. **"Missing documentation" / "consider documenting"** — Low-value doc suggestions
///    for internal or self-explanatory code.
/// 6. **"Hardcoded" values** — Test fixtures, config defaults, or intentional
///    constants flagged as problematic.
/// 7. **`unwrap()` on safe operations** — When unwrap is provably safe (e.g., on
///    a `Receiver::try_recv` or a freshly-created value).
///
/// Each rule uses simple string matching (case-insensitive) on the issue's
/// combined title + description + file path. This is intentionally cheap
/// and deterministic — no regex overhead, no LLM calls.
pub fn filter_known_false_positives(issues: &[ReviewIssue]) -> Vec<ReviewIssue> {
    issues
        .iter()
        .filter(|issue| !is_known_false_positive(issue))
        .cloned()
        .collect()
}

/// Check a single issue against all known false-positive patterns.
fn is_known_false_positive(issue: &ReviewIssue) -> bool {
    // Build a combined text for pattern matching
    let title_lower = issue.title.to_lowercase();
    let desc_lower = issue.description.to_lowercase();
    let file_lower = issue.file.to_lowercase();

    // ── Rule 1: block_in_place async panic ──
    // LLMs frequently see `tokio::task::block_in_place` and claim it will
    // cause a panic in async context. In reality, `block_in_place` is a
    // valid tokio utility for running blocking code without blocking the
    // async runtime. The LLM confuses it with `block_on` in async context.
    if desc_lower.contains("block_in_place") || title_lower.contains("block_in_place") {
        if desc_lower.contains("async")
            || desc_lower.contains("panic")
            || desc_lower.contains("blocking")
        {
            return true;
        }
    }

    // ── Rule 2: futures::executor::block_on project convention ──
    // This project deliberately uses `futures::executor::block_on()` to
    // run async code from sync contexts (e.g., startup, tests, drop guards).
    // The LLM, seeing only the diff, flags it as a "blocking in sync context"
    // issue, which is the intended usage.
    if desc_lower.contains("block_on") || title_lower.contains("block_on") {
        if desc_lower.contains("async")
            || desc_lower.contains("blocking")
            || desc_lower.contains(".await")
            || desc_lower.contains("runtime")
        {
            return true;
        }
    }

    // ── Rule 3: "Silent fallback" / "silently swallows" ──
    // LLMs often flag intentional fallback/default patterns as "silently
    // masking errors". The word "silent" in a review context is a reliable
    // false-positive signal when paired with "fallback" or "default".
    let combined = format!("{} {}", desc_lower, title_lower);
    if (combined.contains("silent") || combined.contains("silently"))
        && (combined.contains("fallback")
            || combined.contains("default")
            || combined.contains("swallow"))
    {
        return true;
    }

    // ── Rule 4: Generic "consider error handling" ──
    // Vague suggestions without a specific error scenario. If the issue
    // just says to add error handling but doesn't describe what error
    // could occur or how, it's likely a default LLM template response.
    if (combined.contains("consider") || combined.contains("recommend"))
        && combined.contains("error handling")
        && !combined.contains("specific")
        && !combined.contains("scenario")
    {
        return true;
    }

    // ── Rule 5: "Missing documentation" ──
    // Low-value doc suggestions for internal/trivial code. If the title
    // or description says to add docs but the code is self-explanatory
    // (e.g., a simple getter, a test, or internal helper), it's noise.
    if combined.contains("documentation")
        || combined.contains("document")
        || combined.contains("docstring")
    {
        // Only filter if it's low severity — real doc gaps are worth noting
        if issue.severity == Severity::Low || issue.severity == Severity::Info {
            return true;
        }
    }

    // ── Rule 6: "Hardcoded" values ──
    // LLMs frequently flag string literals, URLs, or numbers as "hardcoded"
    // without considering whether they're test data, config defaults, or
    // intentional constants. Filter if the issue doesn't provide a specific
    // attack scenario or risk.
    if combined.contains("hardcoded") || combined.contains("hard-coded") {
        // Keep if it's about security (hardcoded credentials/API keys)
        if combined.contains("password")
            || combined.contains("secret")
            || combined.contains("credential")
            || combined.contains("api_key")
            || combined.contains("token")
        {
            // Don't filter — this is a real security concern
        } else {
            // Filter "hardcoded" for non-security items
            if issue.severity == Severity::Low || issue.severity == Severity::Medium {
                return true;
            }
        }
    }

    // ── Rule 7: Test file noise ──
    // In test files, certain patterns like "magic number", "unwrap",
    // "missing error handling" are typically intentional. Test code
    // often uses unwrap liberally and doesn't need production-level
    // error handling.
    if file_lower.contains("test")
        || file_lower.ends_with("_test.rs")
        || file_lower.ends_with("_spec.rs")
        || file_lower.ends_with(".test.ts")
        || file_lower.ends_with(".spec.ts")
    {
        if combined.contains("unwrap")
            || combined.contains("magic number")
            || combined.contains("error handling")
        {
            if issue.severity == Severity::Low || issue.severity == Severity::Info {
                return true;
            }
        }
    }

    false
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

/// Escape raw control characters inside JSON string values.
///
/// LLMs frequently include multi-line content (code snippets, descriptions)
/// with raw newlines (`\n`, `\r`) or tabs inside JSON string values.
/// These are invalid in JSON and cause `serde_json` parse errors like
/// "expected `,` or `}` at line X column Y".
///
/// Replaces raw control characters with their JSON escape equivalents:
/// - `\x00-\x1F (except valid JSON whitespace in strings)` → `\uXXXX` or standard escapes
/// - Specifically: `\n` → `\\n`, `\r` → `\\r`, `\t` → `\\t`
///
/// Properly tracks string boundaries (respects escaped quotes).
pub fn escape_control_chars_in_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 32);
    let mut in_string = false;
    let mut prev_was_escape = false;

    for ch in s.chars() {
        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if in_string {
            match ch {
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\x08' => result.push_str("\\b"),
                '\x0C' => result.push_str("\\f"),
                // Other C0 control characters (U+0000-U+001F except the ones above)
                '\x00'..='\x07' | '\x0B' | '\x0E'..='\x1F' | '\x7F' => {
                    // These are extremely unlikely but escape them as \uXXXX just in case
                    let code = ch as u32;
                    result.push_str(&format!("\\u{:04x}", code));
                }
                _ => result.push(ch),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Remove trailing commas before `}` and `]` throughout JSON.
///
/// LLMs frequently output JSON with trailing commas in nested objects/arrays:
/// ```json
/// {"issues": [{"file": "x.rs", "line": 42,}], ...}
///                                   ^^ trailing comma
/// ```
///
/// `serde_json` rejects these by default. This function removes ALL trailing
/// commas throughout the JSON by scanning for `,` followed by optional
/// whitespace and then `}` or `]`, respecting string boundaries.
pub fn remove_trailing_commas_from_json(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if !in_string && ch == ',' {
            // Look ahead past whitespace for } or ]
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\n' || chars[j] == '\r')
            {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                // This is a trailing comma — skip it
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    result
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

/// Try to parse a JSON string with progressive repair strategies.
///
/// Instead of deeply nested if-let-Err chains, uses a flat sequential
/// strategy pattern:
/// 1. Direct parse (fast path for already-valid JSON)
/// 2. Truncation repair (unclosed braces, unclosed strings)
/// 3. Trailing comma removal (common LLM output issue)
/// 4. Control character escaping (raw newlines/tabs in strings)
///
/// Each strategy is applied independently and returns early on success.
pub fn parse_json_with_fallback(s: &str) -> std::result::Result<serde_json::Value, String> {
    // Strategy 1: Direct parse (fast path — most responses are valid)
    if let Ok(v) = serde_json::from_str(s) {
        return Ok(v);
    }

    // Strategy 2: Repair truncation (unclosed braces, unclosed strings)
    let repaired = repair_truncated_json(s);
    if let Ok(v) = serde_json::from_str(&repaired) {
        return Ok(v);
    }

    // Strategy 3: Remove trailing commas throughout (common LLM artifact)
    let no_trailing = remove_trailing_commas_from_json(&repaired);
    if let Ok(v) = serde_json::from_str(&no_trailing) {
        return Ok(v);
    }

    // Strategy 4: Escape raw control characters in string values
    let escaped = escape_control_chars_in_strings(&no_trailing);
    serde_json::from_str(&escaped).map_err(|e| format!("all strategies exhausted: {e}"))
}

// =============================================================================
// Code Structure Checks
// =============================================================================

/// Check if a file exceeds the maximum line threshold.
fn check_file_too_long(path: &Path, max_lines: usize) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let line_count = content.lines().count();
    line_count > max_lines
}

/// Determine if a file is a Rust source file that should have tests.
///
/// Returns `true` for Rust source files under `src/`. Returns `false` for
/// existing test files (under `tests/`, ending with `_test.rs` or `_spec.rs`),
/// config/doc files, and files from other languages.
///
/// This is deliberately limited to `.rs` files to avoid false positives from
/// other languages whose test conventions differ (TypeScript uses `*.test.ts`,
/// Python uses `test_*.py`, etc.).
fn is_source_file(path: &Path) -> bool {
    let file_str = path.to_string_lossy();

    // Only check Rust source files
    if !file_str.ends_with(".rs") {
        return false;
    }

    // Skip files that are already test files themselves
    if file_str.ends_with("_test.rs") || file_str.ends_with("_spec.rs") {
        return false;
    }

    // Skip files under tests/, test-data/, fixtures/, mocks/ directories
    let skip_dir_patterns = [
        "/tests/",
        "/test-data/",
        "/fixtures/",
        "/mocks/",
    ];
    if skip_dir_patterns.iter().any(|p| file_str.contains(p)) {
        return false;
    }

    // Skip config/doc/generated files
    let skip_extensions = [
        ".toml",
        ".lock",
        ".md",
    ];
    if skip_extensions.iter().any(|ext| file_str.ends_with(ext)) {
        return false;
    }

    true
}

/// Check if a source file has either inline tests or a corresponding test file.
fn has_tests(path: &Path) -> bool {
    // Strategy 1: Check if the source file itself contains inline tests
    if has_inline_tests(path) {
        return true;
    }

    // Strategy 2: Check for a corresponding test file
    derive_test_path(path)
        .map(|test_path| test_path.exists())
        .unwrap_or(false)
}

/// Check if a source file contains inline test annotations.
fn has_inline_tests(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };

    // Check for Rust-style inline tests
    if content.contains("#[cfg(test)]") || content.contains("#[test]") {
        return true;
    }

    false
}

/// Derive a potential test file path from a source file path.
///
/// For `src/foo/bar.rs`, checks `tests/foo/bar.rs` and `tests/test_bar.rs`.
/// For the root `src/lib.rs`, checks `tests/lib.rs` and `tests/test_lib.rs`.
fn derive_test_path(source_path: &Path) -> Option<std::path::PathBuf> {
    let file_str = source_path.to_string_lossy();

    // Strategy A: Strip "src/" prefix, replace with "tests/"
    if let Some(relative) = file_str.strip_prefix("src/") {
        let test_path = Path::new("tests").join(relative);
        if test_path.exists() {
            return Some(test_path);
        }
    }

    // Strategy B: Look for tests/test_<filename>
    let filename = source_path.file_stem()?.to_str()?;
    let test_file = format!("tests/test_{}.rs", filename);
    let test_path = Path::new(&test_file);
    if test_path.exists() {
        return Some(test_path.to_path_buf());
    }

    None
}
