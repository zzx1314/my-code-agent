//! Review report formatting and fix prompt generation for the orchestrator.
//!
//! Handles rendering review results as Markdown and building fix prompts
//! for the main agent to address review issues.

use super::AgentOrchestrator;
use crate::core::types::review::*;

impl AgentOrchestrator {
    /// Format a review coverage summary list showing what categories were checked
    /// and how many issues were found in each.
    pub fn format_review_coverage(&self, report: &ReviewReport) -> String {
        // Only the categories the review agent actually checks.
        // Other ReviewCategory variants exist for config/extensibility
        // but are not produced by the current review pipeline.
        let all_categories: Vec<(ReviewCategory, &str)> = vec![
            (
                ReviewCategory::FunctionalCompleteness,
                "Functional Completeness",
            ),
            (ReviewCategory::BugRisk, "Bug Risk"),
        ];

        let mut output = String::new();
        output.push_str("### 🔍 Review Coverage\n\n");

        for (category, label) in &all_categories {
            let count = report
                .issues
                .iter()
                .filter(|i| i.category == *category)
                .count();
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
            "{} **Verdict**: {}\n\n",
            verdict_icon,
            report.summary.verdict.label(),
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
        output.push_str(&format!(
            "- Total Issues: {}\n\n",
            report.issues.len()
        ));

        if !report.llm_feedback.is_empty() {
            output.push_str("### Review Feedback\n\n");
            output.push_str(&report.llm_feedback);
            output.push_str("\n\n");
        }

        if report.issues.is_empty() {
            output.push_str("✅ No structural issues found!\n\n");
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

        prompt.push_str("The code review has identified issues that need to be fixed. ");
        prompt.push_str(&format!("Verdict: {}\n\n", report.summary.verdict.label(),));

        // Inject review coverage summary — shows what was checked and what was found
        prompt.push_str(&self.format_review_coverage(report));

        if !report.llm_feedback.is_empty() {
            prompt.push_str("### Review Feedback\n\n");
            prompt.push_str(&report.llm_feedback);
            prompt.push_str("\n\n");
        }

        if !report.issues.is_empty() {
            prompt.push_str(&format!(
                "### Found {} Issues\n\n",
                report.issues.len()
            ));

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
                    prompt.push_str(&format!(
                        "   - Fix example: `{}`\n",
                        fix.lines().next().unwrap_or("").trim()
                    ));
                }
                prompt.push_str("\n");
            }

            prompt.push_str("\nPlease fix ALL of the above issues. ");
            prompt.push_str(&format!(
                "Focus on Critical ({}) and High ({}) severity issues first. ",
                report.summary.critical_count, report.summary.high_count,
            ));
            prompt.push_str(
                "After making the fixes, the code will be automatically reviewed again.\n",
            );
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
}
