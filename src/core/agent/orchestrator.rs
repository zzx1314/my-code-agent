//! AgentOrchestrator — Multi-Agent Collaboration Coordinator
//!
//! Manages the collaboration flow between the main Agent and the review Agent:
//! 1. Automatically triggers review after the main Agent completes code changes
//! 2. Supports manual `/review` command
//! 3. Detects changed files and generates review reports

use std::sync::Arc;

use anyhow::Result;

use super::preamble::Agent;
use super::review_agent::{ReviewAgent, ReviewRequest, ReviewEvent};
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
        config: &Config,
        review_config: &ReviewConfig,
    ) -> ReviewAgent {
        ReviewAgent::new(
            main_agent.client.clone(),
            review_config.clone(),
            config.llm.reasoning_field.clone(),
            config.agent.thinking_display.clone(),
        )
    }

    /// Detect changed files by running `git diff` directly.
    /// This is more reliable than parsing tool outputs, as it always reflects
    /// the actual working tree state.
    ///
    /// If `baseline` is `Some(sha)`, diff against that baseline commit instead of HEAD.
    /// This enables incremental reviews: after each review completes, a baseline is
    /// created via `git stash create`, and subsequent reviews only show changes since
    /// that baseline.
    ///
    /// Also detects untracked files (e.g., from `mv` via shell tool) so the review
    /// LLM has complete context — without this, a moved file would appear only as
    /// "Deleted" with no corresponding "Added" entry, causing false positives.
    pub async fn detect_changed_files_from_git(&self, baseline: Option<&str>) -> Vec<ChangedFile> {
        let mut files = Vec::new();

        // Step 1: Get tracked changes via git diff
        let mut args = vec!["diff", "--no-color"];
        if let Some(base) = baseline {
            args.push(base);
        }

        let has_tracked_changes = match tokio::process::Command::new("git")
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
        {
            Ok(o) if o.status.success() => {
                let diff_text = String::from_utf8_lossy(&o.stdout);
                if !diff_text.trim().is_empty() {
                    files = Self::parse_git_diff(&diff_text);
                    true
                } else {
                    false
                }
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("detect_changed_files_from_git: git diff failed: {}", stderr);
                false
            }
            Err(e) => {
                tracing::warn!("detect_changed_files_from_git: failed to run git diff: {}", e);
                false
            }
        };

        // Step 2: Find untracked files (e.g., created by `mv` or `cp` via shell tool)
        // `git ls-files --others --exclude-standard` lists untracked files not in .gitignore
        let untracked_files = match tokio::process::Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
        {
            Ok(o) if o.status.success() => {
                let output = String::from_utf8_lossy(&o.stdout);
                output.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("detect_changed_files_from_git: git ls-files failed: {}", stderr);
                Vec::new()
            }
            Err(e) => {
                tracing::warn!("detect_changed_files_from_git: failed to run git ls-files: {}", e);
                Vec::new()
            }
        };

        // Build ChangedFile entries for untracked files
        for path in &untracked_files {
            // Don't add duplicates if the file already appears in tracked changes
            if files.iter().any(|f| f.path == *path) {
                continue;
            }
            // Read the file content
            let content = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => continue, // skip files that can't be read
            };
            let line_count = content.lines().count();
            // Generate a unified diff for the added file
            let mut diff = format!(
                "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
            );
            diff.push_str(&format!("@@ -0,0 +1,{} @@\n", line_count.max(1)));
            for line in content.lines() {
                diff.push('+');
                diff.push_str(line);
                diff.push('\n');
            }
            // Ensure trailing newline
            if !content.ends_with('\n') {
                diff.push('+');
                diff.push('\n');
            }
            files.push(ChangedFile {
                path: path.clone(),
                change_type: ChangeType::Added,
                lines_added: line_count,
                lines_removed: 0,
                diff,
            });
        }

        if files.is_empty() {
            tracing::info!("detect_changed_files_from_git: no changes");
        } else {
            tracing::info!(count = files.len(), tracked = has_tracked_changes, untracked = untracked_files.len(), baseline = ?baseline, "detect_changed_files_from_git: found changes");
        }

        files
    }

    /// Create a review baseline by running `git stash create`.
    ///
    /// This creates a lightweight dangling commit that captures the current working
    /// tree state, and returns its SHA. The next call to `detect_changed_files_from_git`
    /// with this SHA as the baseline will only show changes made *after* this point.
    ///
    /// This enables incremental reviews when the user hasn't committed between
    /// agent change rounds, preventing previously reviewed changes from being
    /// re-reviewed in each iteration.
    pub fn create_review_baseline() -> Option<String> {
        let output = match std::process::Command::new("git")
            .args(["stash", "create"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("create_review_baseline: failed to run git stash create: {}", e);
                return None;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("create_review_baseline: git stash create failed: {}", stderr);
            return None;
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.is_empty() {
            // Clean working tree — nothing to baseline against
            tracing::info!("create_review_baseline: clean working tree, no baseline created");
            None
        } else {
            tracing::info!(sha = %sha, "create_review_baseline: created baseline");
            Some(sha)
        }
    }

    /// Parse unified diff output from `git diff` into per-file `ChangedFile` entries.
    fn parse_git_diff(diff_text: &str) -> Vec<ChangedFile> {
        let mut files = Vec::new();
        let mut current_diff = String::new();
        let mut current_path: Option<String> = None;
        let mut current_change_type = ChangeType::Modified;

        for line in diff_text.lines() {
            if line.starts_with("diff --git ") {
                // Save previous file if any
                if let Some(path) = current_path.take() {
                    let (added, removed) = count_diff_lines(&current_diff);
                    files.push(ChangedFile {
                        path,
                        change_type: current_change_type.clone(),
                        lines_added: added,
                        lines_removed: removed,
                        diff: current_diff.clone(),
                    });
                    current_diff.clear();
                }
                // Extract file path from "diff --git a/path b/path"
                let path = line
                    .strip_prefix("diff --git a/")
                    .and_then(|s| s.split(" b/").next())
                    .unwrap_or("")
                    .to_string();
                current_path = Some(path);
                current_change_type = ChangeType::Modified;
                current_diff.push_str(line);
                current_diff.push('\n');
            } else if line.starts_with("new file mode") {
                current_change_type = ChangeType::Added;
                current_diff.push_str(line);
                current_diff.push('\n');
            } else if line.starts_with("deleted file mode") {
                current_change_type = ChangeType::Deleted;
                current_diff.push_str(line);
                current_diff.push('\n');
            } else if current_path.is_some() {
                current_diff.push_str(line);
                current_diff.push('\n');
            }
        }

        // Save last file
        if let Some(path) = current_path {
            let (added, removed) = count_diff_lines(&current_diff);
            files.push(ChangedFile {
                path,
                change_type: current_change_type,
                lines_added: added,
                lines_removed: removed,
                diff: current_diff,
            });
        }

        files
    }

    /// Execute review (synchronously wait for result)
    pub async fn review(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        history_summary: Option<&str>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
            history_summary: history_summary.map(|s| s.to_string()),
        };

        self.review_agent.review(&request).await
    }

    pub async fn review_with_events(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        history_summary: Option<&str>,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
            history_summary: history_summary.map(|s| s.to_string()),
        };

        self.review_agent.review_with_events(&request, event_tx).await
    }

    /// Format a review coverage summary list showing what categories were checked
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

        for (category, label) in &all_categories {
            let count = report.issues.iter().filter(|i| i.category == *category).count();
            let icon = category.icon();
            let (status, status_icon) = if count > 0 {
                ("Needs Attention", "⚠️")
            } else {
                ("Passed", "✅")
            };
            if count > 0 {
                output.push_str(&format!(
                    "- {} {}: {} {} ({} issues)\n",
                    icon, label, status_icon, status, count
                ));
            } else {
                output.push_str(&format!(
                    "- {} {}: {} {}\n",
                    icon, label, status_icon, status
                ));
            }
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
        output.push_str(&format!("- 🔴 Critical: {}\n", report.summary.critical_count));
        output.push_str(&format!("- 🟠 High: {}\n", report.summary.high_count));
        output.push_str(&format!("- 🟡 Medium: {}\n", report.summary.medium_count));
        output.push_str(&format!("- 🔵 Low: {}\n", report.summary.low_count));
        output.push_str(&format!("- ℹ️ Info: {}\n\n", report.summary.info_count));

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
            prompt.push_str(&format!("- 🔴 Critical: {}\n", report.summary.critical_count));
            prompt.push_str(&format!("- 🟠 High: {}\n", report.summary.high_count));
            prompt.push_str(&format!("- 🟡 Medium: {}\n", report.summary.medium_count));
            prompt.push_str(&format!("- 🔵 Low: {}\n\n", report.summary.low_count));

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
    /// Checks the **current turn** (messages after the last user message) for
    /// write operations, preventing auto-review from re-triggering on subsequent
    /// interactions that don't involve new file changes.
    ///
    /// This is important because the LLM often follows up a file_write/file_update
    /// tool call with a text-only response (e.g. "Done!"): if we only checked the
    /// **last** assistant message, we would miss the write operation.
    pub fn should_auto_review(&self, history: &[Message]) -> bool {
        if !self.auto_review_enabled || !self.config.enabled {
            return false;
        }

        // Check all assistant messages within the current turn (after the last
        // user message). The LLM often follows up a tool call with a text-only
        // response, so we can't just look at the LAST assistant message.
        let has_recent_write = history
            .iter()
            .rev()
            .take_while(|msg| msg.role != "user")
            .filter(|msg| msg.role == "assistant" && msg.tool_calls.is_some())
            .flat_map(|msg| msg.tool_calls.as_ref().unwrap())
            .any(|tc| {
                tc.function.name == "file_write"
                    || tc.function.name == "file_update"
                    || tc.function.name == "file_delete"
                    || tc.function.name == "apply_patch"
            });

        has_recent_write
    }
}

/// Count added/removed lines from a unified diff string.
fn count_diff_lines(diff: &str) -> (usize, usize) {
    let added = diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
    let removed = diff.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();
    (added, removed)
}

// Type alias for simplified imports
pub type OrchestratorRef = std::sync::Arc<AgentOrchestrator>;
