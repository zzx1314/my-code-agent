//! AgentOrchestrator — Multi-Agent Collaboration Coordinator
//!
//! Manages the collaboration flow between the main Agent and the review Agent:
//! 1. Automatically triggers review after the main Agent completes code changes
//! 2. Supports manual `/review` command
//! 3. Detects changed files and generates review reports

use std::sync::Arc;

use anyhow::Result;

use super::preamble::Agent;
use super::review_agent::{ReviewAgent, ReviewRequest, ReviewEvent, sanitize_json_escapes};
use crate::core::config::Config;
use crate::core::types::review::*;
use crate::core::types::Message;

/// Multi-Agent Coordinator
pub struct AgentOrchestrator {
    /// Main agent (handles daily tasks)
    pub main_agent: Arc<Agent>,
    /// Review agent (dedicated to code review)
    pub review_agent: Arc<ReviewAgent>,
    /// Review configuration
    pub config: ReviewConfig,
    /// Whether auto-review is enabled
    pub auto_review_enabled: bool,
}

impl AgentOrchestrator {
    /// Create a new coordinator
    pub fn new(main_agent: Arc<Agent>, config: &Config) -> Self {
        let review_config = ReviewConfig::from_app_config(&config.review);
        let review_agent = Self::build_review_agent(&main_agent, config, &review_config);

        Self {
            main_agent,
            review_agent: Arc::new(review_agent),
            config: review_config,
            auto_review_enabled: config.review.auto_review,
        }
    }

    /// Derive a review agent from the main agent
    ///
    /// The review agent does NOT register tools — it sends diffs directly
    /// to the LLM and expects a JSON response.
    fn build_review_agent(
        main_agent: &Agent,
        _config: &Config,
        review_config: &ReviewConfig,
    ) -> ReviewAgent {
        ReviewAgent::new(main_agent.client.clone(), review_config.clone())
    }

    /// Detect file changes from the most recent round of the main agent
    pub fn detect_changed_files(&self, history: &[Message]) -> Vec<ChangedFile> {
        let mut files = Vec::new();

        for msg in history {
            if msg.role == "tool" {
                // Parse tool output JSON for write operations (file_write / file_update / file_delete / apply_patch)
                if let Some(changed_file) = Self::parse_tool_output(&msg.content) {
                    // Prefer entries with real diffs over earlier empty ones.
                    // This handles the case where file_read/file_outline ran before file_update
                    // for the same file — the read-only tools are now filtered out by
                    // parse_tool_output, but apply_patch/file_undo may write without git_diff.
                    if let Some(existing) = files.iter_mut().find(|f: &&mut ChangedFile| f.path == changed_file.path) {
                        if !changed_file.diff.is_empty() && existing.diff.is_empty() {
                            tracing::info!(path = %changed_file.path, "detect_changed_files: replaced empty diff with real diff");
                            *existing = changed_file;
                        }
                    } else {
                        tracing::info!(path = %changed_file.path, diff_lines = changed_file.diff.lines().count(), "detect_changed_files: parsed tool output");
                        files.push(changed_file);
                    }
                } else if let Some(path) = Self::extract_file_path(&msg.content) {
                    // Fallback: extract path from non-JSON text output
                    if !files.iter().any(|f: &ChangedFile| f.path == path) {
                        tracing::warn!(path = %path, "detect_changed_files: fallback text extraction (no git_diff)");
                        files.push(ChangedFile {
                            path,
                            change_type: ChangeType::Modified,
                            lines_added: 0,
                            lines_removed: 0,
                            diff: String::new(),
                        });
                    }
                }
            }
        }

        if files.is_empty() {
            let tool_count = history.iter().filter(|m| m.role == "tool").count();
            tracing::warn!(tool_messages = tool_count, history_len = history.len(), "detect_changed_files: no changes found");
        } else {
            tracing::info!(count = files.len(), "detect_changed_files: found changes");
        }

        files
    }

    /// Parse a tool message's JSON content to extract file change info.
    /// Handles FileWriteOutput, FileUpdateOutput, FileDeleteOutput, and ApplyPatchOutput.
    /// Returns `None` for read-only tool outputs (file_read, file_outline, code_review, etc.).
    pub fn parse_tool_output(content: &str) -> Option<ChangedFile> {
        let sanitized = sanitize_json_escapes(content);
        let val: serde_json::Value = serde_json::from_str(&sanitized).ok()?;
        let path = val.get("path")?.as_str()?.to_string();

        // Skip read-only tools (file_read/file_outline/code_review) that have
        // a `path` field but didn't modify any files.
        let is_read_only = val.get("lines").is_some()
            || val.get("total_lines").is_some()
            || val.get("files").is_some();
        if is_read_only {
            return None;
        }

        // Extract git_diff if available
        let diff = val
            .get("git_diff")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Count lines from git_diff to provide meaningful stats
        let lines_added: usize = diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
        let lines_removed: usize = diff.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();

        // Determine change type from git_diff header, or infer from context
        let change_type = if diff.contains("new file mode") || diff.contains("--- /dev/null") {
            ChangeType::Added
        } else if diff.contains("deleted file mode") || diff.contains("+++ /dev/null") {
            ChangeType::Deleted
        } else {
            ChangeType::Modified
        };

        Some(ChangedFile {
            path,
            change_type,
            lines_added,
            lines_removed,
            diff,
        })
    }

    /// Extract file path from tool execution result
    fn extract_file_path(content: &str) -> Option<String> {
        // Try to extract path field from JSON
        let sanitized = sanitize_json_escapes(content);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&sanitized) {
            if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                return Some(path.to_string());
            }
        }

        // Try to extract path pattern from text: `path/to/file.rs`
        for line in content.lines() {
            let line = line.trim();
            // Match path containing extensions like .rs / .toml / .md
            if line.ends_with(".rs")
                || line.ends_with(".toml")
                || line.ends_with(".md")
                || line.ends_with(".js")
                || line.ends_with(".ts")
                || line.ends_with(".py")
                || line.ends_with(".json")
            {
                // Remove leading asterisks, quotes, and other decorations
                let cleaned = line
                    .trim_start_matches(|c: char| "*`\"'".contains(c))
                    .trim_end_matches(|c: char| "*`\"'".contains(c));
                if cleaned.contains('/') || cleaned.contains('\\') {
                    return Some(cleaned.to_string());
                }
            }
        }

        None
    }


    /// Execute review (synchronously wait for result)
    pub async fn review(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
        };

        self.review_agent.review(&request).await
    }

    /// Execute review and return event stream (for phased progress display)
    pub async fn review_with_events(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
        };

        // Use phased review — runs 3 sequential phases with progress events
        self.review_agent.review_phased(&request, event_tx).await
    }

    /// Format a review coverage summary table showing what categories were checked
    /// and how many issues were found in each.
    pub fn format_review_coverage(&self, report: &ReviewReport) -> String {
        let all_categories: Vec<(ReviewCategory, &str)> = vec![
            (ReviewCategory::FunctionalCompleteness, "Functional Completeness"),
            (ReviewCategory::Security, "Security"),
            (ReviewCategory::BugRisk, "Bug Risk"),
            (ReviewCategory::Performance, "Performance"),
            (ReviewCategory::ErrorHandling, "Error Handling"),
            (ReviewCategory::Maintainability, "Maintainability"),
            (ReviewCategory::Style, "Style"),
            (ReviewCategory::Documentation, "Documentation"),
            (ReviewCategory::Concurrency, "Concurrency"),
        ];

        let mut output = String::new();
        output.push_str("### 🔍 Review Coverage\n\n");
        output.push_str("| Category | Status | Issues |\n");
        output.push_str("|----------|--------|:-----:|\n");

        for (category, label) in &all_categories {
            let count = report.issues.iter().filter(|i| i.category == *category).count();
            let icon = category.icon();
            let (status, status_icon) = if count > 0 {
                ("Needs Attention", "⚠️")
            } else {
                ("Passed", "✅")
            };
            let count_str = if count > 0 {
                format!("{}", count)
            } else {
                "—".to_string()
            };
            output.push_str(&format!(
                "| {} {} | {} {} | {} |\n",
                icon, label, status_icon, status, count_str
            ));
        }

        output.push_str("\n");
        output
    }

    /// Format review report as Markdown
    pub fn format_review_report(&self, report: &ReviewReport) -> String {
        let mut output = String::new();

        // Title
        output.push_str("## 📋 Code Review Report\n\n");

        // Summary
        let verdict_icon = report.summary.verdict.icon();
        output.push_str(&format!(
            "{} **Verdict**: {} | **Score**: {:.0}/100\n\n",
            verdict_icon,
            report.summary.verdict.label(),
            report.summary.overall_score,
        ));

        // Inject review coverage summary
        output.push_str(&self.format_review_coverage(report));

        output.push_str("### Statistics Summary\n");
        output.push_str(&format!(
            "- Files Reviewed: {}\n",
            report.changed_files.len()
        ));
        output.push_str(&format!(
            "- Total Changes: +{} / -{} lines\n",
            report.metrics.total_lines_added, report.metrics.total_lines_removed
        ));
        output.push_str(&format!("- Total Issues: {}\n\n", report.summary.total_issues));

        // Severity Distribution
        output.push_str("### Severity Distribution\n\n");
        output.push_str(&format!("| Severity | Count |\n|---------|:-----:|\n"));
        output.push_str(&format!(
            "| 🔴 Critical | {} |\n",
            report.summary.critical_count
        ));
        output.push_str(&format!(
            "| 🟠 High | {} |\n",
            report.summary.high_count
        ));
        output.push_str(&format!(
            "| 🟡 Medium | {} |\n",
            report.summary.medium_count
        ));
        output.push_str(&format!(
            "| 🔵 Low | {} |\n",
            report.summary.low_count
        ));
        output.push_str(&format!(
            "| ℹ️ Info | {} |\n\n",
            report.summary.info_count
        ));

        if report.issues.is_empty() {
            output.push_str("✅ No issues found!\n\n");
            return output;
        }

        // Issues list
        output.push_str("### Issues Found\n\n");

        for (i, issue) in report.issues.iter().enumerate() {
            let icon = issue.severity.icon();
            let sev_label = issue.severity.label();
            let cat_icon = issue.category.icon();

            output.push_str(&format!(
                "#### {}. {} [{}] {}\n\n",
                i + 1,
                icon,
                sev_label,
                issue.title
            ));

            output.push_str(&format!(
                "- **Category**: {} {:?}\n",
                cat_icon, issue.category
            ));
            output.push_str(&format!("- **File**: `{}`", issue.file));
            if let Some(line) = issue.line {
                output.push_str(&format!(":{}", line));
                if let Some(end_line) = issue.end_line {
                    if end_line != line {
                        output.push_str(&format!("-{}", end_line));
                    }
                }
            }
            output.push_str("\n");

            output.push_str(&format!("- **Description**: {}\n", issue.description));

            if let Some(ref suggestion) = issue.suggestion {
                output.push_str(&format!("- **Suggestion**: {}\n", suggestion));
            }

            if let Some(ref snippet) = issue.code_snippet {
                output.push_str("```\n");
                output.push_str(snippet);
                output.push_str("\n```\n");
            }

            if let Some(ref fix) = issue.fix_example {
                output.push_str("\n**Fix Example**:\n```rust\n");
                output.push_str(fix);
                output.push_str("\n```\n");
            }

            output.push_str("\n---\n\n");
        }

        // Auto-fixable issues
        if !report.auto_fixable.is_empty() {
            output.push_str(&format!(
                "### 🔧 Auto-fixable Issues ({})\n\n",
                report.auto_fixable.len()
            ));
            for issue in &report.auto_fixable {
                output.push_str(&format!(
                    "- {} `{}`: {}",
                    issue.severity.icon(),
                    issue.file,
                    issue.title
                ));
                if let Some(ref fix) = issue.fix_example {
                    let first_line = fix.lines().next().unwrap_or("");
                    output.push_str(&format!(" → `{}`", first_line.trim()));
                }
                output.push_str("\n");
            }
            output.push_str("\n");
        }

        output
    }

    /// Build a fix prompt from a review report, asking the main agent to fix the issues.
    pub fn build_fix_prompt(&self, report: &ReviewReport, iteration: usize, max_iterations: usize) -> String {
        let mut prompt = format!(
            "## 🔄 Code Review - Iteration {}/{} — Fix Required\n\n",
            iteration + 1,
            max_iterations,
        );

        prompt.push_str("The code review has identified issues that need to be fixed. ");
        prompt.push_str(&format!(
            "Verdict: {} (Score: {:.0}/100)\n\n",
            report.summary.verdict.label(),
            report.summary.overall_score,
        ));

        // Inject review coverage summary — shows what was checked and what was found
        prompt.push_str(&self.format_review_coverage(report));

        if report.summary.total_issues > 0 {
            prompt.push_str(&format!(
                "### Found {} Issues\n\n",
                report.summary.total_issues
            ));
            prompt.push_str(&format!(
                "| Severity | Count |\n|---------|:-----:|\n"));
            prompt.push_str(&format!("| 🔴 Critical | {} |\n", report.summary.critical_count));
            prompt.push_str(&format!("| 🟠 High | {} |\n", report.summary.high_count));
            prompt.push_str(&format!("| 🟡 Medium | {} |\n", report.summary.medium_count));
            prompt.push_str(&format!("| 🔵 Low | {} |\n\n", report.summary.low_count));

            prompt.push_str("### Issues to Fix\n\n");
            for (i, issue) in report.issues.iter().enumerate() {
                prompt.push_str(&format!(
                    "{}. **{}** [{}] `{}`\n",
                    i + 1,
                    issue.title,
                    issue.severity.label(),
                    issue.file,
                ));
                if let Some(line) = issue.line {
                    prompt.push_str(&format!("   - Line: {}\n", line));
                }
                prompt.push_str(&format!("   - Description: {}\n", issue.description));
                if let Some(ref suggestion) = issue.suggestion {
                    prompt.push_str(&format!("   - Suggestion: {}\n", suggestion));
                }
                if let Some(ref fix) = issue.fix_example {
                    prompt.push_str(&format!("   - Fix example: `{}`\n", fix.lines().next().unwrap_or("").trim()));
                }
                prompt.push_str("\n");
            }

            prompt.push_str("\nPlease fix ALL of the above issues. ");
            prompt.push_str(&format!(
                "Focus on Critical ({}) and High ({}) severity issues first. ",
                report.summary.critical_count,
                report.summary.high_count,
            ));
            prompt.push_str("After making the fixes, the code will be automatically reviewed again.\n");
        } else {
            prompt.push_str("No specific issues were listed. Please review the code carefully and make any necessary improvements.\n");
        }

        prompt.push_str(&format!(
            "\n---\n*Auto-review iteration {}/{}*\n",
            iteration + 1,
            max_iterations,
        ));

        prompt
    }

    /// Determine whether auto-review should be triggered
    ///
    /// Only checks the **most recent assistant turn** for write operations,
    /// preventing auto-review from re-triggering on subsequent interactions
    /// that don't involve new file changes.
    pub fn should_auto_review(&self, history: &[Message]) -> bool {
        if !self.auto_review_enabled || !self.config.enabled {
            return false;
        }

        // Only check the most recent assistant turn for write operations.
        // This prevents auto-review from triggering on subsequent interactions
        // that don't involve new file changes, which was the previous behavior
        // when scanning the entire history.
        let has_recent_write = history
            .iter()
            .rev()
            .find(|msg| msg.role == "assistant")
            .and_then(|msg| msg.tool_calls.as_ref())
            .map(|calls| {
                calls.iter().any(|tc| {
                    tc.function.name == "file_write"
                        || tc.function.name == "file_update"
                        || tc.function.name == "file_delete"
                        || tc.function.name == "apply_patch"
                })
            })
            .unwrap_or(false);

        if !has_recent_write {
            return false;
        }

        let changed_files = self.detect_changed_files(history);
        !changed_files.is_empty()
    }
}

// Type alias for simplified imports
pub type OrchestratorRef = std::sync::Arc<AgentOrchestrator>;
