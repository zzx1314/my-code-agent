//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.

use anyhow::Result;

use super::client::LlmClient;
use crate::core::types::review::*;
use crate::tools::ToolRegistry;

/// Code Review Agent
pub struct ReviewAgent {
    pub client: LlmClient,
    pub tools: ToolRegistry,
    pub config: ReviewConfig,
}

/// Review Request
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

    /// Get the review system prompt
    fn system_prompt(&self) -> String {
        r#"You are a professional code review expert. Your task is to analyze code changes, identify issues, and provide improvement suggestions.

## Review Dimensions
1. **Security** - SQL injection, XSS, sensitive data leaks, permission issues, unsafe dependencies
2. **Performance** - Memory leaks, algorithm complexity, unreleased resources, unnecessary cloning
3. **Reliability** - Null pointers, edge cases, error handling, panic risks
4. **Maintainability** - Code duplication, overlong functions, naming conventions, magic numbers
5. **Concurrency Safety** - Deadlocks, race conditions, data races

## Output Format
You must output a JSON object with the following fields:
```json
{
  "issues": [
    {
      "file": "src/example.rs",
      "line": 42,
      "end_line": 50,
      "severity": "high",
      "category": "security",
      "title": "Issue title",
      "description": "Detailed description",
      "suggestion": "Fix suggestion",
      "code_snippet": "Problem code",
      "fix_example": "Fix example code"
    }
  ],
  "summary": {
    "overall_score": 85,
    "verdict": "approved"
  }
}
```

## Review Principles
- Focus on high-impact issues, avoid nitpicking
- Provide concrete fix code examples
- Explain the root cause of issues
- Prioritize security and stability issues
- Consider Rust's ownership and borrowing rules
"#.to_string()
    }

    /// Execute review
    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        // 1. Gather change information
        let changes_summary = self.format_changes_summary(&request.changed_files);

        // 2. Build review request
        let user_message = format!(
            "Please review the following code changes:\n\n{changes_summary}\n\n{context}",
            changes_summary = changes_summary,
            context = request.context.as_deref().unwrap_or("")
        );

        // 3. Call LLM for review
        let response = self.call_llm(&user_message).await?;

        // 4. Parse review results
        let report = self.parse_review_response(&response, &request.changed_files)?;

        Ok(report)
    }

    /// Call LLM for review (non-streaming, returns full response)
    async fn call_llm(&self, user_message: &str) -> Result<String> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let tool_defs = self.tools.definitions();
        let response = self.client.chat(&messages, &tool_defs).await?;

        // Extract content from OpenAI-compatible response
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No content in review response"))?
            .to_string();

        Ok(content)
    }

    /// Format changes summary
    fn format_changes_summary(&self, files: &[ChangedFile]) -> String {
        let mut summary = String::new();

        summary.push_str(&format!("## Changed Files ({})\n\n", files.len()));

        for file in files {
            summary.push_str(&format!(
                "### {} ({})\n",
                file.path,
                match file.change_type {
                    ChangeType::Added => "Added",
                    ChangeType::Modified => "Modified",
                    ChangeType::Deleted => "Deleted",
                    ChangeType::Renamed => "Renamed",
                }
            ));
            summary.push_str(&format!(
                "- +{} lines, -{} lines\n",
                file.lines_added, file.lines_removed
            ));
            summary.push_str("```diff\n");
            summary.push_str(&file.diff);
            summary.push_str("\n```\n\n");
        }

        summary
    }

    /// Parse review response
    fn parse_review_response(
        &self,
        response: &str,
        changed_files: &[ChangedFile],
    ) -> Result<ReviewReport> {
        // Try to extract JSON from response
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

        // Calculate summary
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

    /// Extract JSON from response
    fn extract_json(&self, response: &str) -> Result<String> {
        // Try to find JSON block
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                if end > start {
                    return Ok(response[start..=end].to_string());
                }
            }
        }

        // Try to find ```json ... ``` block
        if let Some(start) = response.find("```json") {
            let json_start = start + 7;
            if let Some(end) = response[json_start..].find("```") {
                return Ok(response[json_start..json_start + end].trim().to_string());
            }
        }

        anyhow::bail!("Unable to extract JSON from response")
    }
}
