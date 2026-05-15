//! Data structures for code review

use serde::{Deserialize, Serialize};

/// Review severity level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Critical,  // Must fix
    High,      // Should fix
    Medium,    // Recommended to fix
    Low,       // Could be improved
    Info,      // For reference only
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewCategory {
    Security,              // Security vulnerability
    Performance,           // Performance issue
    BugRisk,               // Potential bug
    Style,                 // Code style
    Maintainability,       // Maintainability
    Documentation,         // Documentation issue
    ErrorHandling,         // Error handling
    Concurrency,           // Concurrency issue
    FunctionalCompleteness, // Code does NOT fulfill the user's requirements
}

impl ReviewCategory {
    pub fn icon(&self) -> &str {
        match self {
            ReviewCategory::Security => "🔒",
            ReviewCategory::Performance => "⚡",
            ReviewCategory::BugRisk => "🐛",
            ReviewCategory::Style => "✨",
            ReviewCategory::Maintainability => "🔧",
            ReviewCategory::Documentation => "📝",
            ReviewCategory::ErrorHandling => "⚠️",
            ReviewCategory::Concurrency => "🔄",
            ReviewCategory::FunctionalCompleteness => "🎯",
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
}

/// Review summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total_issues: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub overall_score: f64,  // Score 0-100
    pub verdict: ReviewVerdict,
}

/// Review verdict
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approved,        // OK to merge
    NeedsRevision,   // Needs changes
    Rejected,        // Should be rejected
}

impl ReviewVerdict {
    pub fn icon(&self) -> &str {
        match self {
            ReviewVerdict::Approved => "✅",
            ReviewVerdict::NeedsRevision => "🔄",
            ReviewVerdict::Rejected => "❌",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ReviewVerdict::Approved => "Approved",
            ReviewVerdict::NeedsRevision => "Needs Revision",
            ReviewVerdict::Rejected => "Rejected",
        }
    }
}

impl ReviewReport {
    /// Produce a concise natural-language summary of the review results.
    pub fn natural_summary(&self) -> String {
        if self.issues.is_empty() {
            format!(
                "✅ Review passed — no issues found across {} files (score: {:.0}/100).",
                self.changed_files.len(),
                self.summary.overall_score,
            )
        } else {
            let top_issues: Vec<&str> = self.issues.iter().take(3).map(|i| i.title.as_str()).collect();
            format!(
                "⚠️ Found {} issues ({} critical, {} high) across {} files (score: {:.0}/100, verdict: {}). Key concerns: {}.",
                self.summary.total_issues,
                self.summary.critical_count,
                self.summary.high_count,
                self.changed_files.len(),
                self.summary.overall_score,
                self.summary.verdict.label(),
                top_issues.join("; "),
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
    pub auto_review: bool,                // Whether to auto-review
    pub severity_threshold: Severity,     // Only report issues at or above this level
    pub categories: Vec<ReviewCategory>,  // Categories to check
    pub max_issues: usize,                // Maximum number of issues
    pub include_suggestions: bool,        // Whether to include fix suggestions
    pub max_review_iterations: usize,     // Maximum auto-review iterations (default 3)
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
            categories: vec![
                ReviewCategory::Security,
                ReviewCategory::FunctionalCompleteness,
                ReviewCategory::BugRisk,
                ReviewCategory::Performance,
                ReviewCategory::ErrorHandling,
                ReviewCategory::Maintainability,
                ReviewCategory::Concurrency,
            ],
            max_issues: app_config.max_issues,
            include_suggestions: true,
            max_review_iterations: app_config.max_review_iterations,
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
