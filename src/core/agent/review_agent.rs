//! 代码审查 Agent
//!
//! 负责在主 Agent 完成代码修改后，自动审查变更的代码。

use anyhow::Result;

use super::client::LlmClient;
use crate::core::types::review::*;
use crate::tools::ToolRegistry;

/// 代码审查 Agent
pub struct ReviewAgent {
    pub client: LlmClient,
    pub tools: ToolRegistry,
    pub config: ReviewConfig,
}

/// 审查请求
pub struct ReviewRequest {
    pub changed_files: Vec<ChangedFile>,
    pub context: Option<String>,  // 原始任务描述
}

/// 审查响应事件
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Started { file_count: usize },
    FileAnalyzed { file: String, issues_found: usize },
    Progress { message: String },
    Completed { report: ReviewReport },
    Error { message: String },
}

impl ReviewAgent {
    pub fn new(client: LlmClient, tools: ToolRegistry, config: ReviewConfig) -> Self {
        Self { client, tools, config }
    }

    /// 获取审查系统提示词
    fn system_prompt(&self) -> String {
        r#"你是一个专业的代码审查专家。你的任务是分析代码变更，发现问题并提供改进建议。

## 审查维度
1. **安全性** - SQL 注入、XSS、敏感信息泄露、权限问题、不安全的依赖
2. **性能** - 内存泄漏、算法复杂度、资源未释放、不必要的克隆
3. **可靠性** - 空指针、边界条件、错误处理、panic 风险
4. **可维护性** - 代码重复、函数过长、命名不规范、魔法数字
5. **并发安全** - 死锁、竞态条件、数据竞争

## 输出格式
你必须输出一个 JSON 对象，包含以下字段：
```json
{
  "issues": [
    {
      "file": "src/example.rs",
      "line": 42,
      "end_line": 50,
      "severity": "high",
      "category": "security",
      "title": "问题标题",
      "description": "详细描述",
      "suggestion": "修复建议",
      "code_snippet": "问题代码",
      "fix_example": "修复示例代码"
    }
  ],
  "summary": {
    "overall_score": 85,
    "verdict": "approved"
  }
}
```

## 审查原则
- 关注高影响问题，避免吹毛求疵
- 提供具体的修复代码示例
- 解释问题的根本原因
- 优先处理安全和稳定性问题
- 考虑 Rust 的所有权和借用规则
"#.to_string()
    }

    /// 执行审查
    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        // 1. 收集变更信息
        let changes_summary = self.format_changes_summary(&request.changed_files);

        // 2. 构建审查请求
        let user_message = format!(
            "请审查以下代码变更：\n\n{}\n\n{}",
            changes_summary,
            request.context.as_deref().unwrap_or("")
        );

        // 3. 调用 LLM 进行审查
        let response = self.call_llm(&user_message).await?;

        // 4. 解析审查结果
        let report = self.parse_review_response(&response, &request.changed_files)?;

        Ok(report)
    }

    /// 调用 LLM 进行审查（非流式，返回完整响应）
    async fn call_llm(&self, user_message: &str) -> Result<String> {
        use crate::core::types::Message;

        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let tool_defs = self.tools.definitions();
        let response = self.client.chat(&messages, &tool_defs).await?;

        // 从 OpenAI-compatible 响应中提取 content
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No content in review response"))?
            .to_string();

        Ok(content)
    }

    /// 格式化变更摘要
    fn format_changes_summary(&self, files: &[ChangedFile]) -> String {
        let mut summary = String::new();

        summary.push_str(&format!("## 变更文件 ({} 个)\n\n", files.len()));

        for file in files {
            summary.push_str(&format!(
                "### {} ({})\n",
                file.path,
                match file.change_type {
                    ChangeType::Added => "新增",
                    ChangeType::Modified => "修改",
                    ChangeType::Deleted => "删除",
                    ChangeType::Renamed => "重命名",
                }
            ));
            summary.push_str(&format!(
                "- 添加 {} 行, 删除 {} 行\n",
                file.lines_added, file.lines_removed
            ));
            summary.push_str("```diff\n");
            summary.push_str(&file.diff);
            summary.push_str("\n```\n\n");
        }

        summary
    }

    /// 解析审查响应
    fn parse_review_response(
        &self,
        response: &str,
        changed_files: &[ChangedFile],
    ) -> Result<ReviewReport> {
        // 尝试从响应中提取 JSON
        let json_str = self.extract_json(response)?;

        let parsed: serde_json::Value = serde_json::from_str(&json_str)?;

        let mut issues = Vec::new();

        if let Some(issues_array) = parsed.get("issues").and_then(|v| v.as_array()) {
            for issue in issues_array {
                let severity = match issue.get("severity").and_then(|v| v.as_str()) {
                    Some("critical") => Severity::Critical,
                    Some("high") => Severity::High,
                    Some("medium") => Severity::Medium,
                    Some("low") => Severity::Low,
                    _ => Severity::Info,
                };

                let category = match issue.get("category").and_then(|v| v.as_str()) {
                    Some("security") => ReviewCategory::Security,
                    Some("performance") => ReviewCategory::Performance,
                    Some("bug_risk") => ReviewCategory::BugRisk,
                    Some("style") => ReviewCategory::Style,
                    Some("maintainability") => ReviewCategory::Maintainability,
                    Some("error_handling") => ReviewCategory::ErrorHandling,
                    Some("concurrency") => ReviewCategory::Concurrency,
                    _ => ReviewCategory::Maintainability,
                };

                issues.push(ReviewIssue {
                    file: issue.get("file").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    line: issue.get("line").and_then(|v| v.as_u64()).map(|v| v as usize),
                    end_line: issue.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize),
                    severity,
                    category,
                    title: issue.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: issue.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    suggestion: issue.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    code_snippet: issue.get("code_snippet").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    fix_example: issue.get("fix_example").and_then(|v| v.as_str()).map(|s| s.to_string()),
                });
            }
        }

        // 计算摘要
        let critical_count = issues.iter().filter(|i| i.severity == Severity::Critical).count();
        let high_count = issues.iter().filter(|i| i.severity == Severity::High).count();
        let medium_count = issues.iter().filter(|i| i.severity == Severity::Medium).count();
        let low_count = issues.iter().filter(|i| i.severity == Severity::Low).count();
        let info_count = issues.iter().filter(|i| i.severity == Severity::Info).count();

        let overall_score = parsed
            .get("summary")
            .and_then(|s| s.get("overall_score"))
            .and_then(|v| v.as_f64())
            .unwrap_or(100.0);

        let verdict = match parsed
            .get("summary")
            .and_then(|s| s.get("verdict"))
            .and_then(|v| v.as_str())
        {
            Some("approved") => ReviewVerdict::Approved,
            Some("needs_revision") => ReviewVerdict::NeedsRevision,
            Some("rejected") => ReviewVerdict::Rejected,
            _ => {
                if critical_count > 0 || high_count > 2 {
                    ReviewVerdict::NeedsRevision
                } else {
                    ReviewVerdict::Approved
                }
            }
        };

        let auto_fixable: Vec<ReviewIssue> = issues
            .iter()
            .filter(|i| i.fix_example.is_some())
            .cloned()
            .collect();

        Ok(ReviewReport {
            summary: ReviewSummary {
                total_issues: issues.len(),
                critical_count,
                high_count,
                medium_count,
                low_count,
                info_count,
                overall_score,
                verdict,
            },
            issues,
            changed_files: changed_files.to_vec(),
            metrics: CodeMetrics {
                files_changed: changed_files.len(),
                total_lines_added: changed_files.iter().map(|f| f.lines_added).sum(),
                total_lines_removed: changed_files.iter().map(|f| f.lines_removed).sum(),
                complexity_estimate: None,
            },
            auto_fixable,
        })
    }

    /// 从响应中提取 JSON
    fn extract_json(&self, response: &str) -> Result<String> {
        // 尝试找到 JSON 块
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                if end > start {
                    return Ok(response[start..=end].to_string());
                }
            }
        }

        // 尝试找到 ```json ... ``` 块
        if let Some(start) = response.find("```json") {
            let json_start = start + 7;
            if let Some(end) = response[json_start..].find("```") {
                return Ok(response[json_start..json_start + end].trim().to_string());
            }
        }

        anyhow::bail!("无法从响应中提取 JSON")
    }
}
