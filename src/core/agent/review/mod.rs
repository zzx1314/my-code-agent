//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.
//! The review agent returns natural language feedback — no JSON parsing required.

use anyhow::Result;
use std::path::Path;

use super::client::LlmClient;
use crate::core::types::review::*;

mod checks;
mod context;
mod types;

// Re-export types and free functions from sub-modules for backward compatibility.
pub(crate) use self::checks::{check_file_too_long, has_tests, is_source_file};
pub(crate) use self::context::{
    char_boundary_at_or_before, clean_review_content, extract_previous_iteration_feedback,
    get_file_outline, is_fix_prompt, truncate_content,
};
pub use self::types::{ReviewEvent, ReviewRequest};

/// Code Review Agent
///
/// Reviews code changes by sending diffs directly to the LLM.
/// The LLM returns natural language feedback — no structured JSON output.
pub struct ReviewAgent {
    pub client: LlmClient,
    pub config: ReviewConfig,
    pub reasoning_field: String,
    /// How thinking/reasoning content should be displayed to the user.
    /// "streaming" | "collapsed" | "hidden"
    pub thinking_display: String,
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
            "- Do NOT flag 'missing test coverage' — the project has a deterministic check for that.\n",
            "- Do NOT flag 'tests removed' — tests may have moved to dedicated test files.\n",
            "- Do NOT flag files under `tests/` for missing their own tests — they are test harnesses.\n",
            "- Be concise. If nothing is wrong, just say so.\n",
            "- Only report issues you are CONFIDENT about. Never speculate.\n",
            "- Every claim must be directly verifiable from the provided diff.\n\n",
            "## Output\n\n",
            "Write your review as plain natural language. Be concise.\n",
            "- If everything looks good, simply say what was checked and that it looks fine.\n",
            "- If you find issues, describe each one: which file, what's wrong, and how to fix it.\n",
            "- Reference specific file paths and line numbers when relevant.\n",
            "- Do NOT output JSON or any structured format.\n",
        ).to_string()
    }

    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        let changes_summary = self.format_changes_summary(&request.changed_files);
        let user_message =
            self.build_user_message(&changes_summary, &request.context, &request.history_summary);
        let (response, _reasoning) = self.call_llm(&user_message).await?;

        let structural = self.check_code_structure(&request.changed_files);
        Ok(self.build_report_inner(&response, &structural, &request.changed_files))
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

        let _ = event_tx.send(ReviewEvent::Progress {
            message: "Checking code structure and test coverage...".to_string(),
        });
        let structural = self.check_code_structure(&request.changed_files);
        let report = self.build_report_inner(&response, &structural, &request.changed_files);

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
        let mut reasoning_buf = String::new();

        while let Some(chunk_result) = chat_stream.next().await {
            let chunk = chunk_result?;
            for choice in &chunk.choices {
                let delta = &choice.delta;

                let reasoning_text = delta
                    .reasoning_content
                    .as_ref()
                    .or_else(|| delta.reasoning.as_ref());
                if let Some(rt) = reasoning_text {
                    if !rt.is_empty() && self.thinking_display != "hidden" {
                        let cleaned = tag_stripper.process(rt);

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
                        let _ = event_tx
                            .send(ReviewEvent::ReviewFeedbackDelta(cleaned.clone()));
                        full_content.push_str(&cleaned);
                    }
                }
            }
        }

        // Fallback: if content is empty, use reasoning content
        if full_content.trim().is_empty() {
            if !reasoning_buf.trim().is_empty() {
                tracing::warn!(
                    "call_llm_stream: content was empty, falling back to reasoning_content ({} chars)",
                    reasoning_buf.len()
                );
                return Ok(reasoning_buf);
            } else {
                tracing::error!("call_llm_stream: both content and reasoning_content were empty");
            }
        }

        Ok(full_content)
    }

    /// Extract user's original request from conversation history for review context.
    pub fn extract_context_from_history(history: &[crate::core::types::Message]) -> String {
        let mut result = String::new();

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

        // 3. Include most recent follow-up user message if substantial
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

        // 4. Add previous iteration feedback
        let agent_feedback = extract_previous_iteration_feedback(history);
        if !agent_feedback.is_empty() {
            result.push_str("## Previous Iteration Feedback\n");
            result.push_str("The main agent's response to the previous code review:\n");
            result.push_str(&agent_feedback);
            result.push_str("\n\n");
        }

        // 5. Cap at 2000 characters
        if result.len() > 2000 {
            let boundary = char_boundary_at_or_before(&result, 2000);
            result.truncate(boundary);
            let search_end = char_boundary_at_or_before(&result, 1997.min(result.len()));
            if let Some(last_newline) = result[..search_end].rfind('\n') {
                result.truncate(last_newline + 1);
            }
        }

        if result.is_empty() {
            if let Some(msg) = history.iter().rev().find(|m| m.role == "user") {
                result = clean_review_content(&msg.content);
            }
        }

        result
    }

    /// Extract conversation history summary for consistency checking.
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

            if is_fix_prompt(content) {
                continue;
            }

            let lower = content.to_lowercase();

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
            summary.push_str(&format!("### {} ({})\n", file.path, change_type_str));
            summary.push_str(&format!(
                "- +{} lines, -{} lines\n",
                file.lines_added, file.lines_removed
            ));

            if file.change_type != ChangeType::Deleted {
                if let Some(outline) = get_file_outline(&file.path) {
                    summary.push_str("**File Outline:**\n");
                    summary.push_str("```\n");
                    summary.push_str(&outline);
                    summary.push_str("\n```\n");
                }
            }

            if !file.diff.is_empty() {
                summary.push_str("**Diff:**\n");
                summary.push_str("```diff\n");
                summary.push_str(&file.diff);
                summary.push_str("\n```\n");
            }

            summary.push_str("\n");
        }

        summary
    }

    /// Build a ReviewReport from LLM feedback + structural issues.
    /// The `issues` slice contains only deterministic check results (file length, test coverage).
    fn build_report_inner(
        &self,
        llm_feedback: &str,
        issues: &[ReviewIssue],
        changed_files: &[ChangedFile],
    ) -> ReviewReport {
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

        // Verdict based on structural issues only
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
            llm_feedback: llm_feedback.to_string(),
        }
    }

    /// Rebuild a report from filtered issues, preserving or providing llm_feedback.
    /// Used after deduplication to recalculate summary, metrics, and verdict.
    pub fn rebuild_report(
        &self,
        issues: &[ReviewIssue],
        changed_files: &[ChangedFile],
        llm_feedback: &str,
    ) -> ReviewReport {
        self.build_report_inner(llm_feedback, issues, changed_files)
    }

    /// Perform deterministic code structure checks that don't require an LLM.
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

            // 2. Check test existence
            if is_source_file(path) && !has_tests(path) {
                let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
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
}
