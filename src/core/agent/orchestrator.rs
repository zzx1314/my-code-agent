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
use crate::tools::ToolRegistry;

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
    fn build_review_agent(
        main_agent: &Agent,
        config: &Config,
        review_config: &ReviewConfig,
    ) -> ReviewAgent {
        // Review agent only registers read-only tools
        let mut tools = ToolRegistry::new();
        tools.register(crate::tools::fs::FileRead::from_config(config));
        tools.register(crate::tools::fs::FileOutline);
        tools.register(crate::tools::search::CodeSearch);
        tools.register(crate::tools::fs::ListDir);
        tools.register(crate::tools::fs::GlobSearch);
        tools.register(crate::tools::search::CodeReview);

        ReviewAgent::new(
            main_agent.client.clone(),
            tools,
            review_config.clone(),
        )
    }

    /// Detect file changes from the most recent round of the main agent
    pub fn detect_changed_files(&self, history: &[Message]) -> Vec<ChangedFile> {
        let mut files = Vec::new();

        for msg in history {
            if msg.role == "tool" {
                if let Some(path) = Self::extract_file_path(&msg.content) {
                    // Check if already exists
                    if !files.iter().any(|f: &ChangedFile| f.path == path) {
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

        files
    }

    /// Extract file path from tool execution result
    fn extract_file_path(content: &str) -> Option<String> {
        // Try to extract path field from JSON
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
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

    /// Execute review and return event stream (for UI progress display)
    pub async fn review_with_events(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let file_count = changed_files.len();
        let _ = event_tx.send(ReviewEvent::Started { file_count });

        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
        };

        let _ = event_tx.send(ReviewEvent::Progress {
            message: "Calling review model...".to_string(),
        });

        let report = self.review_agent.review(&request).await?;

        let _ = event_tx.send(ReviewEvent::Completed {
            report: report.clone(),
        });

        Ok(report)
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

    /// Determine whether auto-review should be triggered
    pub fn should_auto_review(&self, history: &[Message]) -> bool {
        if !self.auto_review_enabled || !self.config.enabled {
            return false;
        }

        let changed_files = self.detect_changed_files(history);
        if changed_files.is_empty() {
            return false;
        }

        // Check for write operations (file_write / file_update)
        history.iter().any(|msg| {
            msg.role == "assistant"
                && msg
                    .tool_calls
                    .as_ref()
                    .map(|calls| {
                        calls
                            .iter()
                            .any(|tc| tc.function.name == "file_write" || tc.function.name == "file_update")
                    })
                    .unwrap_or(false)
        })
    }
}

// Type alias for simplified imports
pub type OrchestratorRef = std::sync::Arc<AgentOrchestrator>;
