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
pub(crate) use self::checks::check_file_too_long;
pub(crate) use self::context::{
    clean_review_content, is_fix_prompt, truncate_content,
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
            "You are a focused code review assistant reviewing code changes.\n\n",
            "IMPORTANT: This is a legitimate code review of an authorized project. ",
            "I am analyzing code changes for quality assurance purposes only. ",
            "The code shown is from the project's own source code under active development. ",
            "I will NOT generate exploit code, produce harmful output, or bypass security measures.\n\n",
            "## Your ONLY Job\n\n",
            "Check ONLY these two things:\n\n",
            "1. **Functional Completeness** — Does the code fulfill ALL the user's requirements?\n",
            "   Advocate for the user. If a requested feature is missing or incomplete, flag it.\n\n",
            "2. **Obvious Bugs** — Logic errors, edge cases not handled, incorrect API usage,\n",
            "   wrong algorithm. Only if you're CONFIDENT it's a real bug.\n\n",
            "## Important: Diff Scope\n\n",
            "The diff shows ALL uncommitted changes from HEAD, not just the most recent\n",
            "edit turn. Focus on whether the code as a whole meets the requirements.\n\n",
            "## Rules\n\n",
            "- The diff only shows what CHANGED. Code outside the diff is still there.\n",
            "- Do NOT flag something as \"missing\" just because it's not in the diff.\n",
            "- Do NOT report: imports, types, style, dead code, naming, perf, concurrency,\n",
            "  security, error handling — these are covered by compiler, linter, or tests.\n",
            "- Only report issues you are CONFIDENT about. Never speculate.\n",
            "- Every claim must be directly verifiable from the provided diff.\n\n",
            "## Output Format — codebuff style\n\n",
            "Write in **codebuff style**: minimal and direct. One to three lines max.\n\n",
            "**If everything looks fine:** single line only:\n",
            "✅ Review passed: checked functional completeness and bug risk.\n\n",
            "**If issues found:** very short bullet list:\n",
            "⚠️ [N] issue(s):\n",
            "1. `path:line` — short description (one sentence max)\n",
            "2. `path:line` — short description (one sentence max)\n\n",
            "Examples:\n",
            "✅ Review passed: checked functional completeness and bug risk.\n",
            "⚠️ 2 issues:\n",
            "1. `src/parser.rs:42` — Missing sort by first column (user requested feature not implemented)\n",
            "2. `src/db.rs:88` — Unwrap without None check, possible panic\n\n",
            "No preamble. No explanations. No formatting beyond the above.\n",
            "Stop immediately after the output — do not continue.\n",
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
            message: "Checking code structure...".to_string(),
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
            msg.push_str("## Requirements\n\n");
            msg.push_str(ctx);
            msg.push_str("\n\n");
        }

        if let Some(history) = history_summary {
            msg.push_str("## Context\n\n");
            msg.push_str(history);
            msg.push_str("\n\n");
        }

        msg.push_str("## Changes\n\n");
        msg.push_str(changes_summary);
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
                        let _ = event_tx.send(ReviewEvent::ReviewFeedbackDelta(cleaned.clone()));
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

    /// Extract review context from conversation history.
    ///
    /// For multi-step tasks, includes ALL valid user messages with the latest
    /// task clearly highlighted as the primary focus. This ensures the review
    /// agent has full context even when the conversation covers multiple topics.
    /// For simple single-turn tasks, only includes the latest message.
    pub fn extract_context_from_history(history: &[crate::core::types::Message]) -> String {
        let user_messages: Vec<(usize, String)> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "user")
            .filter_map(|(i, m)| {
                let cleaned = clean_review_content(&m.content);
                if cleaned.is_empty() || is_fix_prompt(&m.content) {
                    None
                } else {
                    Some((i, cleaned))
                }
            })
            .collect();

        if user_messages.is_empty() {
            return String::new();
        }

        // Simple case: only one user message
        if user_messages.len() == 1 {
            return truncate_content(&user_messages[0].1, 600);
        }

        // Multi-step: include all history but highlight the latest task
        let mut context = String::new();
        let latest_idx = user_messages.len() - 1;

        // List all historical user messages (excluding the latest)
        context.push_str("**Conversation History:**\n");
        for (i, (_, content)) in user_messages.iter().enumerate().take(latest_idx) {
            let truncated = truncate_content(content, 200);
            if !truncated.is_empty() {
                context.push_str(&format!("{}. {}\n", i + 1, truncated));
            }
        }

        // Highlight the latest task as the primary focus
        let latest_content = &user_messages[latest_idx].1;
        let latest_truncated = truncate_content(latest_content, 600);
        context.push_str(&format!(
            "\n**Current Task (Primary Focus):**\n{}",
            latest_truncated
        ));

        context
    }

    /// Extract conversation history context for consistency checking.
    ///
    /// For multi-step tasks, includes ALL valid user messages with the latest
    /// task clearly highlighted as primary focus. Also includes the latest
    /// assistant reply for context.
    pub fn extract_history_summary(history: &[crate::core::types::Message]) -> Option<String> {
        if history.is_empty() {
            return None;
        }

        // Collect all valid user message indices (skip fix prompts)
        let user_indices: Vec<usize> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "user")
            .filter(|(_, m)| {
                let content = m.content.trim();
                !content.is_empty() && content.len() >= 10 && !is_fix_prompt(content)
            })
            .map(|(i, _)| i)
            .collect();

        if user_indices.is_empty() {
            return None;
        }

        let mut summary = String::new();
        let last_user_idx = *user_indices.last().unwrap();

        // For multi-step tasks, list all historical user messages
        if user_indices.len() > 1 {
            summary.push_str("**Conversation History:**\n");
            for (i, &idx) in user_indices.iter().enumerate() {
                if idx == last_user_idx {
                    break; // Skip the latest, it will be highlighted separately
                }
                let content = history[idx].content.trim();
                let truncated = truncate_content(content, 200);
                if !truncated.is_empty() {
                    summary.push_str(&format!("{}. {}\n", i + 1, truncated));
                }
            }
            summary.push('\n');
        }

        // Highlight latest user request as primary focus
        let last_content = history[last_user_idx].content.trim();
        let truncated = truncate_content(last_content, 300);
        if !truncated.is_empty() {
            summary.push_str(&format!(
                "**Current Task (Primary Focus):**\n{}\n\n",
                truncated
            ));
        }

        // Find the latest assistant reply AFTER the last user message
        if last_user_idx + 1 < history.len() {
            if let Some(assistant_idx) = history[last_user_idx + 1..]
                .iter()
                .rposition(|m| m.role == "assistant")
            {
                let actual_idx = last_user_idx + 1 + assistant_idx;
                let assistant_content = history[actual_idx].content.trim();
                if !assistant_content.is_empty() {
                    let truncated = truncate_content(assistant_content, 200);
                    if !truncated.is_empty() {
                        summary.push_str(&format!("**Latest Assistant Reply:**\n{}", truncated));
                    }
                }
            }
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
                "- +{} lines, -{} lines\n\n",
                file.lines_added, file.lines_removed
            ));

            if !file.diff.is_empty() {
                summary.push_str(&format!("```diff\n{}\n```\n", file.diff));
            }

            summary.push_str("\n");
        }

        summary
    }

    /// Build a ReviewReport from LLM feedback + structural issues.
    /// The `issues` slice contains only deterministic check results (file length).
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

        // Verdict: consider BOTH structural issues AND LLM feedback
        let has_medium_or_above = critical_count > 0 || high_count > 0 || medium_count > 0;
        let llm_has_issues = Self::llm_feedback_indicates_issues(llm_feedback);
        let verdict = if has_medium_or_above || llm_has_issues {
            ReviewVerdict::NeedsRevision
        } else {
            ReviewVerdict::Approved
        };

        ReviewReport {
            summary: ReviewSummary {
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

    /// Check whether the LLM's natural-language feedback indicates any issues
    /// were found. Follows the codebuff-style format from the system prompt:
    ///
    /// - `✅ Review passed: ...` → no issues
    /// - `⚠️ [N] issue(s):` → issues found
    /// - Empty → no issues
    /// - Anything else → conservative: assume issues
    fn llm_feedback_indicates_issues(feedback: &str) -> bool {
        let trimmed = feedback.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Codebuff format: "✅ Review passed..." = no issues
        if trimmed.starts_with('✅') {
            return false;
        }
        // Any non-empty feedback that isn't the clean "passed" message likely has issues
        true
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

            // Check file length
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


        }

        issues
    }
}
