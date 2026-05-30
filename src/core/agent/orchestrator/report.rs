//! Review report formatting and fix prompt generation for the orchestrator.
//!
//! Handles rendering review results as Markdown and building fix prompts
//! for the main agent to address review issues.

use super::AgentOrchestrator;
use crate::core::types::review::*;

impl AgentOrchestrator {
    /// Format a compact one-line review coverage summary.
    pub fn format_review_coverage(&self, report: &ReviewReport) -> String {
        let func_count = report
            .issues
            .iter()
            .filter(|i| i.category == ReviewCategory::FunctionalCompleteness)
            .count();
        let bug_count = report
            .issues
            .iter()
            .filter(|i| i.category == ReviewCategory::BugRisk)
            .count();

        let mut parts = Vec::new();
        if func_count > 0 {
            parts.push(format!("{} functional completeness issues", func_count));
        } else {
            parts.push("functional completeness ✅".to_string());
        }
        if bug_count > 0 {
            parts.push(format!("{} bug risks", bug_count));
        } else {
            parts.push("bug risk ✅".to_string());
        }

        format!("🔍 Review: {}", parts.join(", "))
    }

    /// Format review report in short codebuff-style.
    /// Output is minimal: verdict + stats + short bullet list.
    pub fn format_review_report(&self, report: &ReviewReport) -> String {
        let verdict_icon = report.summary.verdict.icon();
        let file_count = report.changed_files.len();
        let lines = format!(
            "+{}/-{} lines",
            report.metrics.total_lines_added, report.metrics.total_lines_removed
        );

        let mut output = String::new();

        if report.issues.is_empty() && report.llm_feedback.is_empty() {
            output.push_str(&format!(
                "{} **{}** — {} files, {}, no issues.",
                verdict_icon,
                report.summary.verdict.label(),
                file_count,
                lines,
            ));
            return output;
        }

        let issue_count = report.issues.len();
        if issue_count > 0 {
            output.push_str(&format!(
                "{} **{}** — {} files, {}, {} issue(s).",
                verdict_icon,
                report.summary.verdict.label(),
                file_count,
                lines,
                issue_count,
            ));
        } else {
            output.push_str(&format!(
                "{} **{}** — {} files, {}.",
                verdict_icon,
                report.summary.verdict.label(),
                file_count,
                lines,
            ));
        }
        output.push_str("\n\n");

        // Coverage (compact one-liner)
        output.push_str(&self.format_review_coverage(report));
        output.push_str("\n\n");

        // LLM feedback (concise — review agent is now prompted to be terse)
        if !report.llm_feedback.is_empty() {
            output.push_str(&report.llm_feedback);
            output.push_str("\n");
        }

        // Issues — short bullet list
        for (i, issue) in report.issues.iter().enumerate() {
            let loc = if let Some(line) = issue.line {
                if let Some(end) = issue.end_line {
                    if end != line {
                        format!("`{}:{}-{}`", issue.file, line, end)
                    } else {
                        format!("`{}:{}`", issue.file, line)
                    }
                } else {
                    format!("`{}:{}`", issue.file, line)
                }
            } else {
                format!("`{}`", issue.file)
            };

            output.push_str(&format!(
                "{}. {} [{}] {} — {}\n",
                i + 1,
                issue.severity.icon(),
                issue.severity.label(),
                issue.title,
                loc,
            ));
        }

        output
    }

    /// Build a fix prompt from a review report, asking the main agent to fix the issues.
    pub fn build_fix_prompt(
        &self,
        report: &ReviewReport,
        iteration: usize,
        max_iterations: usize,
    ) -> String {
        let mut prompt = format!(
            "## 🔄 Code Review - Iteration {}/{} — Fix Required\n\n",
            iteration + 1,
            max_iterations,
        );

        prompt.push_str("Verdict: ");
        prompt.push_str(&report.summary.verdict.label());
        prompt.push_str(" | ");
        prompt.push_str(&self.format_review_coverage(report));
        prompt.push_str(&format!(
            " | {} issue(s) total (Critical: {}, High: {})\n\n",
            report.issues.len(),
            report.summary.critical_count,
            report.summary.high_count,
        ));

        // LLM feedback (already concise from review agent)
        if !report.llm_feedback.is_empty() {
            prompt.push_str(&report.llm_feedback);
            prompt.push_str("\n\n");
        }

        // Issues list
        if !report.issues.is_empty() {
            for (i, issue) in report.issues.iter().enumerate() {
                let loc = if let Some(line) = issue.line {
                    format!(":{}", line)
                } else {
                    String::new()
                };
                prompt.push_str(&format!(
                    "{}. **{}** [{}] `{}`{} — {}\n",
                    i + 1,
                    issue.title,
                    issue.severity.label(),
                    issue.file,
                    loc,
                    issue.description,
                ));
            }
            prompt.push_str("\nPlease fix all of the above issues. ");
            prompt.push_str("After making fixes, the code will be auto-reviewed again.\n");
        }

        prompt.push_str(&format!(
            "---\n*Auto-review iteration {}/{}*\n",
            iteration + 1,
            max_iterations,
        ));

        prompt
    }
}
