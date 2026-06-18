//! Data structures for code review

use serde::{Deserialize, Serialize};

/// Review severity level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Critical, // Must fix
    High,     // Should fix
    Medium,   // Recommended to fix
    Low,      // Could be improved
    Info,     // For reference only
}

impl Severity {
    pub fn icon(&self) -> &str {
        match self {
            Severity::Critical => "🔴",
            Severity::High => "🟠",
            Severity::Medium => "🟡",
            Severity::Low => "🔵",
            Severity::Info => "ℹ️",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Severity::Critical => "Critical",
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
            Severity::Info => "Info",
        }
    }
}

/// Review issue category
///
/// The review agent's system prompt limits LLM output to `FunctionalCompleteness`
/// and `BugRisk`. Other variants exist for configuration, display, and extensibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewCategory {
    /// Code does NOT fulfill the user's requirements
    FunctionalCompleteness,
    /// Potential bug
    BugRisk,
    /// Maintainability / code health (used as default fallback)
    Maintainability,
    /// Security vulnerability
    Security,
    /// Performance issue
    Performance,
    /// Code style
    Style,
    /// Error handling
    ErrorHandling,
    /// Concurrency issue
    Concurrency,
}

impl ReviewCategory {
    pub fn icon(&self) -> &str {
        match self {
            ReviewCategory::FunctionalCompleteness => "🎯",
            ReviewCategory::BugRisk => "🐛",
            ReviewCategory::Maintainability => "🔧",
            ReviewCategory::Security => "🔒",
            ReviewCategory::Performance => "⚡",
            ReviewCategory::Style => "✨",
            ReviewCategory::ErrorHandling => "⚠️",
            ReviewCategory::Concurrency => "🔄",
        }
    }
}

/// A single review issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewIssue {
    pub file: String,
    pub line: Option<usize>,
    pub end_line: Option<usize>,
    pub severity: Severity,
    pub category: ReviewCategory,
    pub title: String,
    pub description: String,
    pub suggestion: Option<String>,
    pub code_snippet: Option<String>,
    pub fix_example: Option<String>,
}

impl ReviewIssue {
    /// Compute a stable fingerprint for deduplication across review iterations.
    ///
    /// Uses `(file, title, description_prefix)` to identify duplicate issues.
    /// This catches repeated false positives where the review agent reports the
    /// same issue in consecutive iterations with no file changes.
    pub fn fingerprint(&self) -> String {
        let desc_prefix: String = self.description.chars().take(80).collect();
        format!("{}::{}::{}", self.file, self.title, desc_prefix)
    }

    /// Filter out issues whose fingerprints match any in `previous_issues`.
    ///
    /// Only filters out if the issue's file was NOT modified in the current
    /// iteration (checked via `current_changed_files`). If the file was
    /// modified, the issue is kept — the main agent may have attempted a fix
    /// and a remaining or different issue deserves another chance.
    ///
    /// This prevents the auto-review loop from repeatedly reporting the same
    /// false positive across iterations when the main agent has chosen not
    /// to act on a particular issue.
    pub fn deduplicate_against(
        issues: Vec<ReviewIssue>,
        previous_issues: &[ReviewIssue],
        current_changed_files: &[ChangedFile],
    ) -> Vec<ReviewIssue> {
        if previous_issues.is_empty() {
            return issues;
        }

        // Build a set of file paths that were actually modified in this iteration.
        // If a file was modified, we should NOT suppress issues for it even if
        // the fingerprint matches — the agent may have fixed something else in
        // that file but the issue remains.
        let modified_files: std::collections::HashSet<&str> = current_changed_files
            .iter()
            .filter(|f| f.change_type != ChangeType::Deleted)
            .map(|f| f.path.as_str())
            .collect();

        let prev_fingerprints: std::collections::HashSet<String> = previous_issues
            .iter()
            .filter(|i| !modified_files.contains(i.file.as_str()))
            .map(|i| i.fingerprint())
            .collect();

        if prev_fingerprints.is_empty() {
            return issues;
        }

        issues
            .into_iter()
            .filter(|issue| !prev_fingerprints.contains(&issue.fingerprint()))
            .collect()
    }
}

/// Changed file information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub change_type: ChangeType,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Review report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub summary: ReviewSummary,
    pub issues: Vec<ReviewIssue>,
    pub changed_files: Vec<ChangedFile>,
    pub metrics: CodeMetrics,
    pub auto_fixable: Vec<ReviewIssue>,
    /// Natural language feedback from the review LLM (not parsed as JSON issues).
    pub llm_feedback: String,
}

/// Review summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub verdict: ReviewVerdict,
}

/// Review verdict
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approved,      // OK to merge
    NeedsRevision, // Needs changes
}

impl ReviewVerdict {
    pub fn icon(&self) -> &str {
        match self {
            ReviewVerdict::Approved => "✅",
            ReviewVerdict::NeedsRevision => "🔄",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ReviewVerdict::Approved => "Approved",
            ReviewVerdict::NeedsRevision => "Needs Revision",
        }
    }
}

impl ReviewReport {
    /// Produce a concise natural-language summary of the review results.
    pub fn natural_summary(&self) -> String {
        if self.issues.is_empty() && self.llm_feedback.is_empty() {
            format!(
                "✅ Review passed — no issues found across {} files.",
                self.changed_files.len(),
            )
        } else {
            let mut parts = Vec::new();
            if !self.llm_feedback.is_empty() {
                let preview: String = self.llm_feedback.chars().take(120).collect();
                parts.push(preview);
            }
            if !self.issues.is_empty() {
                parts.push(format!(
                    "Structural issues: {} ({} critical, {} high).",
                    self.issues.len(),
                    self.summary.critical_count,
                    self.summary.high_count,
                ));
            }
            format!(
                "⚠️ Verdict: {} ({} files). {}",
                self.summary.verdict.label(),
                self.changed_files.len(),
                parts.join(" "),
            )
        }
    }
}

/// Code metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeMetrics {
    pub files_changed: usize,
    pub total_lines_added: usize,
    pub total_lines_removed: usize,
    pub complexity_estimate: Option<f64>,
}

/// Review configuration
#[derive(Debug, Clone)]
pub struct ReviewConfig {
    pub enabled: bool,
    pub auto_review: bool,               // Whether to auto-review
    pub severity_threshold: Severity,    // Only report issues at or above this level
    pub categories: Vec<ReviewCategory>, // Categories to check
    pub max_issues: usize,               // Maximum number of issues
    pub include_suggestions: bool,       // Whether to include fix suggestions
    pub max_review_iterations: usize,    // Maximum auto-review iterations (default 3)
    pub max_file_lines: Option<usize>,   // Max lines before suggesting split (None = disabled)
}

impl ReviewConfig {
    /// Create review config from application config
    pub fn from_app_config(app_config: &crate::core::config::ReviewConfig) -> Self {
        let severity_threshold = match app_config.severity_threshold.to_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Low,
        };

        Self {
            enabled: app_config.enabled,
            auto_review: app_config.auto_review,
            severity_threshold,
            // The review agent's system prompt limits LLM output to
            // FunctionalCompleteness and BugRisk. Other categories in
            // the enum exist for extensibility.
            categories: vec![
                ReviewCategory::FunctionalCompleteness,
                ReviewCategory::BugRisk,
            ],
            max_issues: app_config.max_issues,
            include_suggestions: true,
            max_review_iterations: app_config.max_review_iterations,
            max_file_lines: app_config.max_file_lines,
        }
    }
}

/// Review outcome (used for auto-review iteration loop)
#[derive(Debug, Clone)]
pub struct ReviewOutcome {
    /// Review report display text
    pub display_text: String,
    /// Review verdict
    pub verdict: ReviewVerdict,
    /// Review summary (used to build fix prompts)
    pub report_summary: String,
    /// Review report (used to build fix prompts)
    pub report: Option<ReviewReport>,
    /// Whether to auto-trigger fixes (auto-review=true, manual=false)
    pub auto_trigger: bool,
}
