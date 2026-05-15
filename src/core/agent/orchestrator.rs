//! AgentOrchestrator — 多 Agent 协作协调器
//!
//! 管理主 Agent 和审查 Agent 之间的协作流程：
//! 1. 主 Agent 完成代码变更后自动触发审查
//! 2. 支持手动 `/review` 命令
//! 3. 检测变更文件并生成审查报告

use std::sync::Arc;

use anyhow::Result;

use super::preamble::Agent;
use super::review_agent::{ReviewAgent, ReviewRequest, ReviewEvent};
use crate::core::config::Config;
use crate::core::types::review::*;
use crate::core::types::Message;
use crate::tools::ToolRegistry;

/// 多 Agent 协调器
pub struct AgentOrchestrator {
    /// 主 Agent（执行日常任务）
    pub main_agent: Arc<Agent>,
    /// 审查 Agent（代码审查专用）
    pub review_agent: Arc<ReviewAgent>,
    /// 审查配置
    pub config: ReviewConfig,
    /// 是否启用自动审查
    pub auto_review_enabled: bool,
}

impl AgentOrchestrator {
    /// 创建新的协调器
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

    /// 从主 Agent 派生审查 Agent
    fn build_review_agent(
        main_agent: &Agent,
        config: &Config,
        review_config: &ReviewConfig,
    ) -> ReviewAgent {
        // 审查 Agent 只注册只读工具
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

    /// 检测主 Agent 最近一轮中涉及的文件变更
    pub fn detect_changed_files(&self, history: &[Message]) -> Vec<ChangedFile> {
        let mut files = Vec::new();

        for msg in history {
            if msg.role == "tool" {
                if let Some(path) = Self::extract_file_path(&msg.content) {
                    // 检查是否已经存在
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

    /// 从工具执行结果中提取文件路径
    fn extract_file_path(content: &str) -> Option<String> {
        // 尝试从 JSON 中提取 path 字段
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                return Some(path.to_string());
            }
        }

        // 尝试从文本中提取路径模式：`path/to/file.rs`
        for line in content.lines() {
            let line = line.trim();
            // 匹配包含 .rs / .toml / .md 等扩展名的路径
            if line.ends_with(".rs")
                || line.ends_with(".toml")
                || line.ends_with(".md")
                || line.ends_with(".js")
                || line.ends_with(".ts")
                || line.ends_with(".py")
                || line.ends_with(".json")
            {
                // 移除行首的星号、引号等装饰
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

    /// 构建审查提示
    fn build_review_prompt(&self, changed_files: &[ChangedFile], context: Option<&str>) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "请审查以下 {} 个文件的代码变更：\n\n",
            changed_files.len()
        ));

        for file in changed_files {
            prompt.push_str(&format!("- {} ({:?})\n", file.path, file.change_type));
        }

        if let Some(ctx) = context {
            prompt.push_str(&format!("\n## 原始任务\n{}\n", ctx));
        }

        prompt.push_str(
            "\n请读取每个文件的内容，进行全面审查，并输出 JSON 格式的审查报告。",
        );

        prompt
    }

    /// 执行审查（同步等待结果）
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

    /// 执行审查并返回事件流（用于 UI 展示进度）
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
            message: "正在调用审查模型...".to_string(),
        });

        let report = self.review_agent.review(&request).await?;

        let _ = event_tx.send(ReviewEvent::Completed {
            report: report.clone(),
        });

        Ok(report)
    }

    /// 格式化审查报告为 Markdown
    pub fn format_review_report(&self, report: &ReviewReport) -> String {
        let mut output = String::new();

        // 标题
        output.push_str("## 📋 代码审查报告\n\n");

        // 摘要
        let verdict_icon = report.summary.verdict.icon();
        output.push_str(&format!(
            "{} **结论**: {} | **评分**: {:.0}/100\n\n",
            verdict_icon,
            report.summary.verdict.label(),
            report.summary.overall_score,
        ));

        output.push_str("### 统计摘要\n");
        output.push_str(&format!(
            "- 审查文件数: {}\n",
            report.changed_files.len()
        ));
        output.push_str(&format!(
            "- 总变更: +{} / -{} 行\n",
            report.metrics.total_lines_added, report.metrics.total_lines_removed
        ));
        output.push_str(&format!("- 问题总数: {}\n\n", report.summary.total_issues));

        // 严重程度统计
        output.push_str("### 严重程度分布\n\n");
        output.push_str(&format!("| 级别 | 数量 |\n|------|:----:|\n"));
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
            output.push_str("✅ 未发现问题！\n\n");
            return output;
        }

        // 问题列表
        output.push_str("### 发现的问题\n\n");

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
                "- **类别**: {} {:?}\n",
                cat_icon, issue.category
            ));
            output.push_str(&format!("- **文件**: `{}`", issue.file));
            if let Some(line) = issue.line {
                output.push_str(&format!(":{}", line));
                if let Some(end_line) = issue.end_line {
                    if end_line != line {
                        output.push_str(&format!("-{}", end_line));
                    }
                }
            }
            output.push_str("\n");

            output.push_str(&format!("- **描述**: {}\n", issue.description));

            if let Some(ref suggestion) = issue.suggestion {
                output.push_str(&format!("- **建议**: {}\n", suggestion));
            }

            if let Some(ref snippet) = issue.code_snippet {
                output.push_str("```\n");
                output.push_str(snippet);
                output.push_str("\n```\n");
            }

            if let Some(ref fix) = issue.fix_example {
                output.push_str("\n**修复示例**:\n```rust\n");
                output.push_str(fix);
                output.push_str("\n```\n");
            }

            output.push_str("\n---\n\n");
        }

        // 自动可修复的问题
        if !report.auto_fixable.is_empty() {
            output.push_str(&format!(
                "### 🔧 可自动修复的问题 ({} 个)\n\n",
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

    /// 判断是否应该触发自动审查
    pub fn should_auto_review(&self, history: &[Message]) -> bool {
        if !self.auto_review_enabled || !self.config.enabled {
            return false;
        }

        let changed_files = self.detect_changed_files(history);
        if changed_files.is_empty() {
            return false;
        }

        // 检查是否有写操作（file_write / file_update）
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

// 类型别名用于简化导入
pub type OrchestratorRef = std::sync::Arc<AgentOrchestrator>;
