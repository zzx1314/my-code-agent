//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.

use anyhow::Result;

use super::client::LlmClient;
use crate::core::types::review::*;

/// Code Review Agent
///
/// Reviews code changes by sending diffs directly to the LLM.
/// Does NOT register tools — the LLM should analyze the diffs we provide
/// and return JSON, not call additional tools.
pub struct ReviewAgent {
    pub client: LlmClient,
    pub config: ReviewConfig,
}

/// Review Request
pub struct ReviewRequest {
    pub changed_files: Vec<ChangedFile>,
    pub context: Option<String>,  // Original task description
}

/// Review response events
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Started { file_count: usize },
    FileAnalyzed { file: String, issues_found: usize },
    Progress { message: String },
    Completed { report: ReviewReport },
    Error { message: String },
}

impl ReviewAgent {
    pub fn new(client: LlmClient, config: ReviewConfig) -> Self {
        Self { client, config }
    }

    /// Get the review system prompt
    fn system_prompt(&self) -> String {
        r#"You are a professional code review expert. Your task is to analyze code changes, identify issues, and provide improvement suggestions.

## Review Categories
Analyze code across all of the following categories:

1. **security** — SQL injection, XSS, sensitive data leaks, permission issues, unsafe dependencies
2. **performance** — Memory leaks, algorithm complexity, unreleased resources, unnecessary cloning/allocation
3. **bug_risk** — Null/unchecked pointers, off-by-one errors, edge cases, type confusion, logic errors
4. **error_handling** — Missing error propagation, ignored Results, unwrap/expect panics, silent failures
5. **maintainability** — Code duplication, overlong functions, poor naming, magic numbers, dead code
6. **style** — Inconsistent formatting, non-idiomatic patterns, lint violations, naming convention breaks
7. **documentation** — Missing/misleading doc comments, stale comments, missing public API docs
8. **concurrency** — Data races, deadlocks, missing synchronization, async misuse, Send/Sync issues

## Valid Values
- **severity**: `"critical"` | `"high"` | `"medium"` | `"low"` | `"info"`
- **category**: one of the 8 categories listed above (`"security"`, `"performance"`, etc.)
- **verdict**: `"approved"` (no blocking issues) | `"needs_revision"` (fixes required) | `"rejected"` (fundamental problems)
- **overall_score**: integer 0–100 (higher = better)

## Output Format
You MUST output ONLY a valid JSON object. Do NOT include any other text or explanation. Output the JSON directly (either raw or wrapped in a ```json code block).
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
- Focus on high-impact issues, avoid nitpicking. It's better to report 3 real bugs than 20 minor style nits.
- Provide concrete fix code examples for every issue where possible.
- Explain the root cause of issues, not just the symptom.
- Adapt the review to the language of each file (detected by file extension):
  - **Rust** — ownership/borrowing, unsafe blocks, unwrap/expect
  - **TypeScript/JavaScript** — type safety, null handling, async/promise hygiene
  - **Python** — exception handling, type hints, dynamic pitfalls
  - **Other languages** — apply language-appropriate best practices
- Prioritize correctness, security, and stability over style preferences.
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
    ///
    /// Note: tool definitions are intentionally NOT passed here.
    /// The review agent should analyze the diffs we provide directly,
    /// not call additional tools. Passing tools causes the LLM to
    /// return tool calls instead of JSON review output.
    async fn call_llm(&self, user_message: &str) -> Result<String> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_message),
        ];

        let response = self.client.chat(&messages, &[]).await?;

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
        extract_json_from_response(response)
    }
}

/// Extract a JSON object from an LLM response string.
///
/// Handles multiple formats:
/// 1. ```json ... ``` code blocks
/// 2. ``` ... ``` code blocks (looks for JSON-like content inside)
/// 3. Bare `{...}` objects using brace counting (handles nesting)
/// 4. If the entire string is valid JSON, returns it directly
pub fn extract_json_from_response(response: &str) -> Result<String> {
    let response = response.trim();

    // Strategy 1: Try ```json ... ``` code block (most common with LLMs)
    if let Some(start) = response.find("```json") {
        let json_start = start + 7;
        if let Some(end) = response[json_start..].find("```") {
            return Ok(response[json_start..json_start + end].trim().to_string());
        }
    }

    // Strategy 2: Try ``` ... ``` code block without language specifier
    if let Some(start) = response.rfind("```") {
        let before = &response[..start];
        // Find matching opening ```
        if let Some(open) = before.rfind("```") {
            let inner = response[open + 3..start].trim();
            // Check if it looks like JSON (starts with { or [)
            if inner.starts_with('{') || inner.starts_with('[') {
                return Ok(inner.to_string());
            }
        }
    }

    // Strategy 3: Find outermost { ... } pair using string-aware brace counting
    // This handles nested braces properly and skips braces inside string literals.
    if let Some(start) = response.find('{') {
        let mut depth = 0_i64;
        let mut json_start = None;
        let mut json_end = None;
        let mut in_string = false;
        let mut prev_was_escape = false;
        for (i, ch) in response.chars().enumerate() {
            if i < start { continue; }
            // Track string boundaries to skip braces inside strings
            if ch == '"' && !prev_was_escape {
                in_string = !in_string;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            if in_string { continue; }
            match ch {
                '{' => {
                    if depth == 0 {
                        json_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        json_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let (Some(js), Some(je)) = (json_start, json_end) {
            if je > js {
                return Ok(response[js..=je].to_string());
            }
        }
    }

    // Strategy 4: If response is itself valid JSON, return it
    if response.starts_with('{') && response.ends_with('}') {
        if serde_json::from_str::<serde_json::Value>(response).is_ok() {
            return Ok(response.to_string());
        }
    }

    anyhow::bail!(
        "Unable to extract JSON from response. Response was:\n{}",
        response.chars().take(500).collect::<String>()
    )
}