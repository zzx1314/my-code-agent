//! 代码审查相关的数据结构

use serde::{Deserialize, Serialize};

/// 审查严重程度
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Critical,  // 严重 - 必须修复
    High,      // 高 - 应该修复
    Medium,    // 中 - 建议修复
    Low,       // 低 - 可以改进
    Info,      // 信息 - 仅供参考
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

/// 审查问题类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewCategory {
    Security,       // 安全漏洞
    Performance,    // 性能问题
    BugRisk,        // 潜在 Bug
    Style,          // 代码风格
    Maintainability,// 可维护性
    Documentation,  // 文档问题
    ErrorHandling,  // 错误处理
    Concurrency,    // 并发问题
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
        }
    }
}

/// 单个审查问题
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

/// 变更文件信息
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

/// 审查报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub summary: ReviewSummary,
    pub issues: Vec<ReviewIssue>,
    pub changed_files: Vec<ChangedFile>,
    pub metrics: CodeMetrics,
    pub auto_fixable: Vec<ReviewIssue>,
}

/// 审查摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total_issues: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub overall_score: f64,  // 0-100 分
    pub verdict: ReviewVerdict,
}

/// 审查结论
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approved,        // 可以合并
    NeedsRevision,   // 需要修改
    Rejected,        // 建议拒绝
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

/// 代码指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeMetrics {
    pub files_changed: usize,
    pub total_lines_added: usize,
    pub total_lines_removed: usize,
    pub complexity_estimate: Option<f64>,
}

/// 审查配置
#[derive(Debug, Clone)]
pub struct ReviewConfig {
    pub enabled: bool,
    pub auto_review: bool,              // 是否自动审查
    pub severity_threshold: Severity,   // 只报告此级别以上的问题
    pub categories: Vec<ReviewCategory>,// 要检查的类别
    pub max_issues: usize,              // 最大问题数
    pub include_suggestions: bool,      // 是否包含修复建议
}

impl ReviewConfig {
    /// 从应用配置创建审查配置
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
                ReviewCategory::BugRisk,
                ReviewCategory::Performance,
                ReviewCategory::ErrorHandling,
                ReviewCategory::Maintainability,
                ReviewCategory::Concurrency,
            ],
            max_issues: app_config.max_issues,
            include_suggestions: true,
        }
    }
}
