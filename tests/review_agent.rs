//! 代码审查 Agent 集成测试
//!
//! 测试 ReviewAgent、AgentOrchestrator 和多 Agent 协作流程。

use my_code_agent::core::types::review::*;

/// 测试 ReviewIssue 结构的创建和基本方法
#[test]
fn test_review_issue_creation() {
    let issue = ReviewIssue {
        file: "src/main.rs".to_string(),
        line: Some(42),
        end_line: Some(50),
        severity: Severity::High,
        category: ReviewCategory::Security,
        title: "不安全的函数调用".to_string(),
        description: "使用了不安全的函数，可能导致缓冲区溢出".to_string(),
        suggestion: Some("使用安全的替代函数".to_string()),
        code_snippet: Some("unsafe { ... }".to_string()),
        fix_example: Some("safe_function()".to_string()),
    };

    assert_eq!(issue.severity.icon(), "🟠");
    assert_eq!(issue.severity.label(), "High");
    assert_eq!(issue.category.icon(), "🔒");
    assert_eq!(issue.file, "src/main.rs");
    assert_eq!(issue.line, Some(42));
}
/// 测试 Severity 排序（声明顺序即为优先级排序）
#[test]
fn test_severity_ordering() {
    // Critical 优先级最高（先声明），所以 Critical < High 为 true
    assert!(Severity::Critical < Severity::High);
    assert!(Severity::High < Severity::Medium);
    assert!(Severity::Medium < Severity::Low);
    assert!(Severity::Low < Severity::Info);
}

/// 测试 ReviewVerdict 方法

/// 测试 ReviewVerdict 方法
#[test]
fn test_review_verdict() {
    assert_eq!(ReviewVerdict::Approved.icon(), "✅");
    assert_eq!(ReviewVerdict::Approved.label(), "Approved");
    assert_eq!(ReviewVerdict::NeedsRevision.icon(), "🔄");
    assert_eq!(ReviewVerdict::Rejected.icon(), "❌");
}

/// 测试 ReviewReport 的构建
#[test]
fn test_review_report_creation() {
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 2,
            critical_count: 1,
            high_count: 1,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            overall_score: 65.0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: Some(10),
                end_line: None,
                severity: Severity::Critical,
                category: ReviewCategory::Security,
                title: "SQL 注入风险".to_string(),
                description: "用户输入未经验证直接拼接 SQL 查询".to_string(),
                suggestion: Some("使用参数化查询".to_string()),
                code_snippet: None,
                fix_example: Some("query!(\"SELECT * FROM users WHERE id = ?\", id)".to_string()),
            },
            ReviewIssue {
                file: "src/lib.rs".to_string(),
                line: Some(100),
                end_line: Some(120),
                severity: Severity::High,
                category: ReviewCategory::BugRisk,
                title: "可能的空指针引用".to_string(),
                description: "未检查 Option 值可能为 None".to_string(),
                suggestion: None,
                code_snippet: Some("let x = opt.unwrap();".to_string()),
                fix_example: None,
            },
        ],
        changed_files: vec![
            ChangedFile {
                path: "src/main.rs".to_string(),
                change_type: ChangeType::Modified,
                lines_added: 15,
                lines_removed: 3,
                diff: "+ fn main() {".to_string(),
            },
        ],
        metrics: CodeMetrics {
            files_changed: 1,
            total_lines_added: 15,
            total_lines_removed: 3,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
    };

    assert_eq!(report.summary.total_issues, 2);
    assert_eq!(report.summary.verdict, ReviewVerdict::NeedsRevision);
    assert_eq!(report.issues[0].severity, Severity::Critical);
    assert_eq!(report.metrics.files_changed, 1);
    assert!(report.auto_fixable.is_empty());
}

/// 测试 ChangedFile 和 ChangeType
#[test]
fn test_changed_file() {
    let file = ChangedFile {
        path: "src/core/agent/mod.rs".to_string(),
        change_type: ChangeType::Added,
        lines_added: 100,
        lines_removed: 0,
        diff: String::new(),
    };

    assert_eq!(file.change_type, ChangeType::Added);
    assert_eq!(file.lines_added, 100);
    assert_eq!(file.lines_removed, 0);
}

/// 测试 ReviewConfig 的 from_app_config 方法
#[test]
fn test_review_config_from_app_config() {
    let app_config = my_code_agent::core::config::ReviewConfig {
        enabled: true,
        auto_review: true,
        threshold_lines: 5,
        max_issues: 50,
        severity_threshold: "high".to_string(),
        on_file_write: true,
        on_file_update: true,
    };

    let config = ReviewConfig::from_app_config(&app_config);
    assert!(config.enabled);
    assert!(config.auto_review);
    assert_eq!(config.severity_threshold, Severity::High);
    assert_eq!(config.max_issues, 50);
    assert!(!config.categories.is_empty());
}

/// 测试 CodeMetrics 创建
#[test]
fn test_code_metrics() {
    let metrics = CodeMetrics {
        files_changed: 3,
        total_lines_added: 200,
        total_lines_removed: 50,
        complexity_estimate: Some(15.5),
    };

    assert_eq!(metrics.files_changed, 3);
    assert_eq!(metrics.total_lines_added, 200);
    assert_eq!(metrics.total_lines_removed, 50);
    assert_eq!(metrics.complexity_estimate, Some(15.5));
}

/// 测试 ReviewSummary 的默认值
#[test]
fn test_review_summary_defaults() {
    let summary = ReviewSummary {
        total_issues: 0,
        critical_count: 0,
        high_count: 0,
        medium_count: 0,
        low_count: 0,
        info_count: 0,
        overall_score: 100.0,
        verdict: ReviewVerdict::Approved,
    };

    assert_eq!(summary.overall_score, 100.0);
    assert_eq!(summary.total_issues, 0);
    assert_eq!(summary.verdict, ReviewVerdict::Approved);
}

/// 测试 ReviewEvent 枚举
#[test]
fn test_review_event_creation() {
    use my_code_agent::app::commands::review::ReviewEvent;

    let started = ReviewEvent::Started { file_count: 5 };
    let progress = ReviewEvent::Progress {
        message: "分析中...".to_string(),
    };
    let error = ReviewEvent::Error {
        message: "出错了".to_string(),
    };

    // 验证这些事件可以匹配
    match started {
        ReviewEvent::Started { file_count } => assert_eq!(file_count, 5),
        _ => panic!("事件类型不匹配"),
    }

    match progress {
        ReviewEvent::Progress { message } => assert_eq!(message, "分析中..."),
        _ => panic!("事件类型不匹配"),
    }

    match error {
        ReviewEvent::Error { message } => assert_eq!(message, "出错了"),
        _ => panic!("事件类型不匹配"),
    }
}
