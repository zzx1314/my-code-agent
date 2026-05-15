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
    /// Emitted when a review phase completes (used for phased/multi-category review)
    PhaseCompleted {
        phase_index: usize,           // 1-based phase number
        total_phases: usize,          // total number of phases
        phase_name: String,           // e.g. "Core Correctness"
        categories: Vec<String>,      // category names checked in this phase
        issues_found: usize,          // number of issues found
        passed: bool,                 // true if no issues
        details: String,              // brief summary
    },
    Completed { report: ReviewReport },
    Error { message: String },
}

/// Phase definition for phased review
struct ReviewPhase {
    pub name: &'static str,
    pub categories: &'static [ReviewCategory],
}

/// The 3 phases for phased review — ordered from most critical to least critical.
const REVIEW_PHASES: &[ReviewPhase] = &[
    // Phase 1: Does the code actually work and handle errors?
    ReviewPhase {
        name: "Core Correctness",
        categories: &[
            ReviewCategory::FunctionalCompleteness,
            ReviewCategory::BugRisk,
            ReviewCategory::ErrorHandling,
        ],
    },
    // Phase 2: Is it safe, fast, and concurrent?
    ReviewPhase {
        name: "Safety & Reliability",
        categories: &[
            ReviewCategory::Security,
            ReviewCategory::Performance,
            ReviewCategory::Concurrency,
        ],
    },
    // Phase 3: Is the code well-structured and documented?
    ReviewPhase {
        name: "Code Quality",
        categories: &[
            ReviewCategory::Maintainability,
            ReviewCategory::Style,
            ReviewCategory::Documentation,
        ],
    },
];

impl ReviewAgent {
    pub fn new(client: LlmClient, config: ReviewConfig) -> Self {
        Self { client, config }
    }

    /// Build the system prompt, optionally filtering to only include specified categories
    fn system_prompt(&self, phase_categories: Option<&[ReviewCategory]>) -> String {
        // Build category filter set
        let category_filter = phase_categories.map(|cats| {
            cats.iter().map(|c| std::mem::discriminant(c)).collect::<std::collections::HashSet<_>>()
        });

        let mut prompt = String::from(
            "You are a professional code review expert. Your task is to analyze code changes, identify issues, and provide improvement suggestions.\n\n"
        );

        prompt.push_str("## Review Categories\n");
        if phase_categories.is_some() {
            prompt.push_str("Focus ONLY on the following categories for this review phase:\n\n");
        } else {
            prompt.push_str("Analyze code across all of the following categories:\n\n");
        }

        let all_categories: Vec<(ReviewCategory, &str)> = vec![
            (ReviewCategory::FunctionalCompleteness, "🎯 **CRITICAL: Does the code actually fulfill the user's requirements?** Check if:\n   - The implementation fully addresses ALL aspects of the user's request (not just part of it)\n   - There are no placeholder/stub implementations (e.g. `todo!()`, `unimplemented!()`, `throw new Error(\"Not implemented\")`, FIXME comments)\n   - All required functions/endpoints/components are actually implemented, not just declared\n   - Edge cases mentioned in the requirements are handled\n   - Return values and outputs match what was asked for\n   - Configuration and wiring (imports, routes, registrations) is complete — not just the core logic"),
            (ReviewCategory::Security, "Security — SQL injection, XSS, sensitive data leaks, permission issues, unsafe dependencies"),
            (ReviewCategory::Performance, "Performance — Memory leaks, algorithm complexity, unreleased resources, unnecessary cloning/allocation"),
            (ReviewCategory::BugRisk, "Bug Risk — Null/unchecked pointers, off-by-one errors, edge cases, type confusion, logic errors"),
            (ReviewCategory::ErrorHandling, "Error Handling — Missing error propagation, ignored Results, unwrap/expect panics, silent failures"),
            (ReviewCategory::Maintainability, "Maintainability — Code duplication, overlong functions, poor naming, magic numbers, dead code"),
            (ReviewCategory::Style, "Style — Inconsistent formatting, non-idiomatic patterns, lint violations, naming convention breaks"),
            (ReviewCategory::Documentation, "Documentation — Missing/misleading doc comments, stale comments, missing public API docs"),
            (ReviewCategory::Concurrency, "Concurrency — Data races, deadlocks, missing synchronization, async misuse, Send/Sync issues"),
        ];

        let mut idx = 1;
        for (category, description) in &all_categories {
            let include = match &category_filter {
                Some(filter) => filter.contains(&std::mem::discriminant(category)),
                None => true,
            };
            if include {
                let cat_name = match category {
                    ReviewCategory::FunctionalCompleteness => "functional_completeness",
                    ReviewCategory::Security => "security",
                    ReviewCategory::Performance => "performance",
                    ReviewCategory::BugRisk => "bug_risk",
                    ReviewCategory::ErrorHandling => "error_handling",
                    ReviewCategory::Maintainability => "maintainability",
                    ReviewCategory::Style => "style",
                    ReviewCategory::Documentation => "documentation",
                    ReviewCategory::Concurrency => "concurrency",
                };
                let icon = category.icon();
                prompt.push_str(&format!("{}. **{}** — {} {}\n", idx, cat_name, icon, description));
                idx += 1;
            }
        }
        prompt.push_str("\n");

        prompt.push_str("## Valid Values\n");
        prompt.push_str("- **severity**: `\"critical\"` | `\"high\"` | `\"medium\"` | `\"low\"` | `\"info\"`\n");
        prompt.push_str("- **category**: one of the category names listed above (`\"functional_completeness\"`, `\"security\"`, `\"performance\"`, etc.)\n");
        prompt.push_str("- **verdict**: `\"approved\"` (no blocking issues) | `\"needs_revision\"` (fixes required) | `\"rejected\"` (fundamental problems)\n");
        prompt.push_str("- **overall_score**: integer 0–100 (higher = better)\n\n");

        prompt.push_str("## Output Format\n");
        prompt.push_str("You MUST output ONLY a valid JSON object. Do NOT include any other text or explanation. Output the JSON directly (either raw or wrapped in a ```json code block).\n");
        prompt.push_str("```json\n");
        prompt.push_str("{\n");
        prompt.push_str("  \"issues\": [\n");
        prompt.push_str("    {\n");
        prompt.push_str("      \"file\": \"src/example.rs\",\n");
        prompt.push_str("      \"line\": 42,\n");
        prompt.push_str("      \"end_line\": 50,\n");
        prompt.push_str("      \"severity\": \"high\",\n");
        prompt.push_str("      \"category\": \"security\",\n");
        prompt.push_str("      \"title\": \"Issue title\",\n");
        prompt.push_str("      \"description\": \"Detailed description\",\n");
        prompt.push_str("      \"suggestion\": \"Fix suggestion\",\n");
        prompt.push_str("      \"code_snippet\": \"Problem code\",\n");
        prompt.push_str("      \"fix_example\": \"Fix example code\"\n");
        prompt.push_str("    }\n");
        prompt.push_str("  ],\n");
        prompt.push_str("  \"summary\": {\n");
        prompt.push_str("    \"overall_score\": 85,\n");
        prompt.push_str("    \"verdict\": \"approved\"\n");
        prompt.push_str("  }\n");
        prompt.push_str("}\n");
        prompt.push_str("```\n\n");

        prompt.push_str("## Verdict Guidelines\n");
        prompt.push_str("- Use `\"rejected\"` when functional_completeness has critical issues — the code fundamentally does not do what was asked.\n");
        prompt.push_str("- Use `\"needs_revision\"` when there are any high/critical severity issues of ANY category, or when parts of the requirements are missing.\n");
        prompt.push_str("- Use `\"approved\"` ONLY when ALL requirements are fully met AND there are no high/critical issues.\n");
        prompt.push_str("- **Be strict about functional_completeness.** It is better to catch a missing feature than to approve incomplete code.\n\n");

        prompt.push_str("## Review Principles\n");
        prompt.push_str("- Focus on high-impact issues, avoid nitpicking. It's better to report 3 real bugs than 20 minor style nits.\n");
        prompt.push_str("- Provide concrete fix code examples for every issue where possible.\n");
        prompt.push_str("- Explain the root cause of issues, not just the symptom.\n");
        prompt.push_str("- Adapt the review to the language of each file (detected by file extension):\n");
        prompt.push_str("  - **Rust** — ownership/borrowing, unsafe blocks, unwrap/expect\n");
        prompt.push_str("  - **TypeScript/JavaScript** — type safety, null handling, async/promise hygiene, `any` types, `as` casts\n");
        prompt.push_str("  - **Python** — exception handling, type hints, dynamic pitfalls\n");
        prompt.push_str("  - **Other languages** — apply language-appropriate best practices\n");
        prompt.push_str("- Prioritize correctness and functional completeness over style preferences.\n");

        prompt
    }

    /// Execute review (single phase — all categories at once)
    pub async fn review(&self, request: &ReviewRequest) -> Result<ReviewReport> {
        let (all_issues, changed_files) = self.execute_phase(request, None).await?;
        self.build_report(&all_issues, &changed_files)
    }

    /// Execute phased review — runs 3 sequential phases, each reporting results via event_tx
    pub async fn review_phased(
        &self,
        request: &ReviewRequest,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let file_count = request.changed_files.len();
        let _ = event_tx.send(ReviewEvent::Started { file_count });

        // Token optimization strategy:
        // - Phase 1: full diff (must check functional completeness + bugs)
        // - Phase 2: signature summary (function/struct/import signatures for safety/perf)
        // - Phase 3: compressed file list only (style/docs/maintainability)
        let full_changes_summary = self.format_changes_summary(&request.changed_files);
        let signature_changes_summary = self.format_changes_summary_signatures(&request.changed_files);
        let compressed_changes_summary = self.format_changes_summary_compressed(&request.changed_files);

        let build_base_message = |summary: &str, ctx: &Option<String>| -> String {
            if let Some(context) = ctx {
                format!(
                    "## User Request (Requirements)\n\n{context}\n\n## Code Changes to Review\n\n{summary}",
                    context = context,
                    summary = summary,
                )
            } else {
                format!(
                    "Please review the following code changes:\n\n{summary}",
                    summary = summary,
                )
            }
        };

        let mut all_issues = Vec::new();
        let total_phases = REVIEW_PHASES.len();

        for (phase_index, phase) in REVIEW_PHASES.iter().enumerate() {
            // Phase 1: full diff | Phase 2: signatures | Phase 3: compressed
            let changes_summary = match phase_index {
                0 => &full_changes_summary,
                1 => &signature_changes_summary,
                _ => &compressed_changes_summary,
            };
            let base_message = build_base_message(changes_summary, &request.context);

            // Build phase-specific user message
            let cat_names: Vec<String> = phase.categories.iter().map(|c| {
                match c {
                    ReviewCategory::FunctionalCompleteness => "functional_completeness".to_string(),
                    ReviewCategory::Security => "security".to_string(),
                    ReviewCategory::Performance => "performance".to_string(),
                    ReviewCategory::BugRisk => "bug_risk".to_string(),
                    ReviewCategory::ErrorHandling => "error_handling".to_string(),
                    ReviewCategory::Maintainability => "maintainability".to_string(),
                    ReviewCategory::Style => "style".to_string(),
                    ReviewCategory::Documentation => "documentation".to_string(),
                    ReviewCategory::Concurrency => "concurrency".to_string(),
                }
            }).collect();

            let phase_message = format!(
                "{}\n\n## Review Phase Focus\nFocus ONLY on the following categories in this phase:\n- {}\n\nDo NOT report issues in categories outside this phase. They will be checked in separate phases.",
                base_message,
                cat_names.join("\n- "),
            );

            let _ = event_tx.send(ReviewEvent::Progress {
                message: format!("Phase {}/{}: {} — Reviewing...", phase_index + 1, total_phases, phase.name),
            });

            // Call LLM for this phase
            let response = self.call_llm(Some(phase.categories), &phase_message).await?;

            // Parse issues for this phase — extract raw issues from JSON
            let phase_issues = self.parse_issues_from_response(&response)?;

            // Filter to only include issues in our phase's categories
            let filtered: Vec<ReviewIssue> = phase_issues
                .into_iter()
                .filter(|i| phase.categories.contains(&i.category))
                .collect();

            let passed = filtered.is_empty();
            let issues_found = filtered.len();

            // Send phase completed event with details
            let _ = event_tx.send(ReviewEvent::PhaseCompleted {
                phase_index: phase_index + 1,
                total_phases,
                phase_name: phase.name.to_string(),
                categories: cat_names,
                issues_found,
                passed,
                details: if passed {
                    format!("✅ No issues in {}", phase.name)
                } else {
                    format!("⚠️ Found {} issue(s) in {}", issues_found, phase.name)
                },
            });

            all_issues.extend(filtered);
        }

        // Build final report from all aggregated issues
        let report = self.build_report(&all_issues, &request.changed_files)?;

        let _ = event_tx.send(ReviewEvent::Completed {
            report: report.clone(),
        });

        Ok(report)
    }

    /// Execute a single review phase (or full review if phase_categories is None)
    async fn execute_phase(
        &self,
        request: &ReviewRequest,
        phase_categories: Option<&[ReviewCategory]>,
    ) -> Result<(Vec<ReviewIssue>, Vec<ChangedFile>)> {
        let changes_summary = self.format_changes_summary(&request.changed_files);

        let user_message = if let Some(ref context) = request.context {
            format!(
                "## User Request (Requirements)\n\n{context}\n\n## Code Changes to Review\n\n{changes_summary}",
                context = context,
                changes_summary = changes_summary,
            )
        } else {
            format!(
                "Please review the following code changes:\n\n{changes_summary}",
                changes_summary = changes_summary,
            )
        };

        let response = self.call_llm(phase_categories, &user_message).await?;
        let issues = self.parse_issues_from_response(&response)?;

        Ok((issues, request.changed_files.clone()))
    }

    /// Call LLM for review (non-streaming, returns full response)
    ///
    /// Note: tool definitions are intentionally NOT passed here.
    /// The review agent should analyze the diffs we provide directly,
    /// not call additional tools. Passing tools causes the LLM to
    /// return tool calls instead of JSON review output.
    async fn call_llm(&self, phase_categories: Option<&[ReviewCategory]>, user_message: &str) -> Result<String> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt(phase_categories)),
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

    /// Extract user's original request from conversation history for review context.
    /// Returns the concatenated user messages (excluding auto-generated fix prompts from review iterations).
    pub fn extract_context_from_history(history: &[crate::core::types::Message]) -> String {
        let mut user_messages: Vec<&str> = history
            .iter()
            .filter(|m| {
                m.role == "user"
                    && !m.content.contains("fix the issues found in the code review")
                    && !m.content.contains("Auto-Review Iteration")
                    && !m.content.contains("Fix Required")
            })
            .map(|m| m.content.as_str())
            .collect();

        // Take the last 2 user messages (original request + any followups)
        let count = user_messages.len();
        if count > 2 {
            user_messages = user_messages[count.saturating_sub(2)..].to_vec();
        }

        user_messages.join("\n\n---\n\n")
    }

    /// Format changes summary (full diff included)
    fn format_changes_summary(&self, files: &[ChangedFile]) -> String {
        self.format_changes_summary_inner(files, true, false)
    }

    /// Format changes summary with function signatures extracted from the diff (no full diff body).
    /// Used by Phase 2 — gives enough context for safety/performance/concurrency checks
    /// without sending the entire diff.
    fn format_changes_summary_signatures(&self, files: &[ChangedFile]) -> String {
        self.format_changes_summary_inner(files, false, true)
    }

    /// Format changes summary (compressed — no diff body, only metadata).
    /// Used by Phase 3 — only needs file list for style/docs/maintainability checks.
    fn format_changes_summary_compressed(&self, files: &[ChangedFile]) -> String {
        self.format_changes_summary_inner(files, false, false)
    }

    /// Shared implementation for formatting changes summary.
    /// When `include_diff` is true, includes the full diff body for each file.
    /// When `include_signatures` is true, includes function/struct signatures extracted from the diff.
    fn format_changes_summary_inner(&self, files: &[ChangedFile], include_diff: bool, include_signatures: bool) -> String {
        let mut summary = String::new();

        summary.push_str(&format!("## Changed Files ({})\n\n", files.len()));

        for file in files {
            let change_type_str = match file.change_type {
                ChangeType::Added => "Added",
                ChangeType::Modified => "Modified",
                ChangeType::Deleted => "Deleted",
                ChangeType::Renamed => "Renamed",
            };
            summary.push_str(&format!(
                "### {} ({})\n",
                file.path, change_type_str,
            ));
            summary.push_str(&format!(
                "- +{} lines, -{} lines\n",
                file.lines_added, file.lines_removed
            ));

            if include_diff && !file.diff.is_empty() {
                summary.push_str("```diff\n");
                summary.push_str(&file.diff);
                summary.push_str("\n```\n");
            } else if include_signatures && !file.diff.is_empty() {
                let sigs = Self::extract_signatures_from_diff(&file.diff);
                if !sigs.is_empty() {
                    summary.push_str(&format!("```\n{}\
```\n", sigs.join("\n")));
                }
            }

            summary.push_str("\n");
        }

        if !include_diff {
            let note = if include_signatures {
                "*Key signatures extracted from diff (context for Phase 2 review).*"
            } else {
                "*Full diff was reviewed in Phase 1. This phase checks additional quality dimensions.*"
            };
            summary.push_str(note);
            summary.push_str("\n\n");
        }

        summary
    }

    /// Extract function/struct/enum/trait/import signatures from a diff string.
    /// Scans added lines (starting with `+`) for common declaration patterns
    /// across Rust, TypeScript, Python, and other languages.
    fn extract_signatures_from_diff(diff: &str) -> Vec<String> {
        let mut sigs = Vec::new();

        for line in diff.lines() {
            let trimmed = line.trim_start();
            // Only look at added lines (diff lines starting with +)
            if !trimmed.starts_with('+') {
                continue;
            }
            let content = trimmed[1..].trim();
            if content.is_empty() {
                continue;
            }

            // Check for common declaration patterns (language-agnostic)
            let is_signature = content.starts_with("fn ")
                || content.starts_with("pub fn ")
                || content.starts_with("pub async fn ")
                || content.starts_with("async fn ")
                || content.starts_with("pub struct ")
                || content.starts_with("struct ")
                || content.starts_with("pub enum ")
                || content.starts_with("enum ")
                || content.starts_with("pub trait ")
                || content.starts_with("trait ")
                || content.starts_with("impl ")
                || content.starts_with("pub type ")
                || content.starts_with("type ")
                || content.starts_with("pub const ")
                || content.starts_with("const ")
                || content.starts_with("use ")
                || content.starts_with("macro_rules!")
                || content.starts_with("def ")
                || content.starts_with("async def ")
                || content.starts_with("class ")
                || content.starts_with("function ")
                || content.starts_with("export function ")
                || content.starts_with("export default ")
                || content.starts_with("export class ")
                || content.starts_with("export interface ")
                || content.starts_with("interface ")
                || content.starts_with("import ")
                || content.starts_with("from ");

            if is_signature {
                // Take the full line up to the first { or ; for compactness
                let sig = content.split(['{', ';', '=']).next().unwrap_or(content).trim();
                sigs.push(sig.to_string());
            }
        }

        sigs
    }

    /// Parse review response — full pipeline: extract JSON, parse issues, build report.
    /// (Kept for backward compatibility with tests.)
    pub fn parse_review_response(
        &self,
        response: &str,
        changed_files: &[ChangedFile],
    ) -> Result<ReviewReport> {
        let issues = self.parse_issues_from_response(response)?;
        self.build_report(&issues, changed_files)
    }

    /// Extract issues from a JSON review response (without building the full report).
    /// Returns the raw list of ReviewIssue structs.
    fn parse_issues_from_response(&self, response: &str) -> Result<Vec<ReviewIssue>> {
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
                    Some("functional_completeness") => ReviewCategory::FunctionalCompleteness,
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

        Ok(issues)
    }

    /// Build a complete ReviewReport from a list of issues and changed files.
    /// Calculates summary statistics, verdict, and auto-fixable list.
    fn build_report(&self, issues: &[ReviewIssue], changed_files: &[ChangedFile]) -> Result<ReviewReport> {
        let critical_count = issues.iter().filter(|i| i.severity == Severity::Critical).count();
        let high_count = issues.iter().filter(|i| i.severity == Severity::High).count();
        let medium_count = issues.iter().filter(|i| i.severity == Severity::Medium).count();
        let low_count = issues.iter().filter(|i| i.severity == Severity::Low).count();
        let info_count = issues.iter().filter(|i| i.severity == Severity::Info).count();

        let has_functional_completeness_issues = issues.iter().any(|i| matches!(i.category, ReviewCategory::FunctionalCompleteness));
        let has_blocking_issues = critical_count > 0
            || high_count > 1
            || (high_count > 0 && has_functional_completeness_issues)
            || medium_count > 3
            || has_functional_completeness_issues;

        let verdict = if has_blocking_issues || !issues.is_empty() {
            if critical_count > 0 && has_functional_completeness_issues {
                ReviewVerdict::Rejected
            } else if has_blocking_issues {
                ReviewVerdict::NeedsRevision
            } else {
                ReviewVerdict::Approved
            }
        } else {
            ReviewVerdict::Approved
        };

        let overall_score = if issues.is_empty() {
            100.0
        } else {
            let penalty = (critical_count * 25 + high_count * 10 + medium_count * 5 + low_count * 2 + info_count) as f64;
            (100.0 - penalty).max(0.0)
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
            issues: issues.to_vec(),
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
