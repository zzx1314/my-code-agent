//! Code Review Agent
//!
//! Responsible for automatically reviewing code changes after the main Agent completes modifications.

use anyhow::Result;
use futures::future::join_all;

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
    /// Reasoning/thinking content from the LLM during review.
    /// Displayed on the frontend but NOT added to conversation history.
    ReasoningDelta(String),
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

/// Default focus areas for parallel review
const DEFAULT_FOCUSES: &[&str] = &[
    "Security and bug risk review",
    "Functional completeness — does the code fully address user requirements?",
    "Code simplification and reuse of existing patterns",
    "Performance and concurrency review",
    "Maintainability and code style",
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
        prompt.push_str("- **Be concise**: Focus on high-impact issues. If there are no critical issues, say so briefly. Do not pad the review with unnecessary praise.\n\n");

        prompt.push_str("## Review Guidelines (from industry best practices)\n");
        prompt.push_str("- **Advocate for the user**: Ensure ALL aspects of the user's request are addressed. Call out any missing requirements.\n");
        prompt.push_str("- **Minimal changes**: Prefer changing as few lines of code as possible. Don't rewrite working code.\n");
        prompt.push_str("- **Reuse existing code**: Where a function already exists, reuse it — do not create a new one.\n");
        prompt.push_str("- **No dead code**: Ensure no unused imports, variables, or functions are introduced.\n");
        prompt.push_str("- **No missing imports**: Verify all imports are present for new code.\n");
        prompt.push_str("- **No accidental deletions**: Verify no sections were deleted that weren't supposed to be.\n");
        prompt.push_str("- **Match existing style**: New code must match the existing codebase's style and conventions.\n");
        prompt.push_str("- **Avoid unnecessary try/catch**: Don't add try/catch blocks unless truly needed — they clutter the code.\n");
        prompt.push_str("- **Simplify when possible**: If logic can be simplified, suggest it.\n\n");

        prompt.push_str("## Reasoning Step\n");
        prompt.push_str("Before providing your JSON review output, use <antThinking> tags to reason through the code changes and identify issues. Think through:\n");
        prompt.push_str("1. Do the changes fully address the user's requirements?\n");
        prompt.push_str("2. Are there any bugs, security issues, or performance problems?\n");
        prompt.push_str("3. Can any code be simplified or reused?\n");
        prompt.push_str("4. Are there any edge cases missed?\n\n");
        prompt.push_str("Example format:\n");
        prompt.push_str("<antThinking>\n");
        prompt.push_str("The changes add a new API endpoint. Let me check: authentication is present but rate limiting is missing...\n");
        prompt.push_str("</thinking>\n\n");
        prompt.push_str("{\"issues\": [...], \"summary\": {...}}\n\n");

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

            // Call LLM with streaming — reasoning deltas are sent in real-time via event_tx
            let response = self.call_llm_stream(Some(phase.categories), &phase_message, &event_tx).await?;

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

        let (response, _reasoning) = self.call_llm(phase_categories, &user_message).await?;
        let issues = self.parse_issues_from_response(&response)?;

        Ok((issues, request.changed_files.clone()))
    }

    /// Call LLM for review (non-streaming, returns full response and reasoning content)
    ///
    /// Returns `(content, reasoning_content)` where `reasoning_content` may be empty
    /// for non-reasoning models.
    ///
    /// Note: tool definitions are intentionally NOT passed here.
    /// The review agent should analyze the diffs we provide directly,
    /// not call additional tools. Passing tools causes the LLM to
    /// return tool calls instead of JSON review output.
    async fn call_llm(&self, phase_categories: Option<&[ReviewCategory]>, user_message: &str) -> Result<(String, String)> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt(phase_categories)),
            Message::user(user_message),
        ];

        let response = self.client.chat(&messages, &[]).await?;

        let message = &response["choices"][0]["message"];

        // Extract content from OpenAI-compatible response
        let content = message["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No content in review response"))?
            .to_string();

        // Extract reasoning_content if present (for reasoning models like DeepSeek Reasoner)
        let reasoning = message.get("reasoning_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok((content, reasoning))
    }

    /// Call LLM with streaming, sending reasoning deltas via event_tx in real-time.
    ///
    /// Returns the full accumulated text content for JSON parsing.
    /// Reasoning content is streamed as `ReviewEvent::ReasoningDelta` deltas to
    /// the frontend for real-time display, without being added to conversation history.
    async fn call_llm_stream(
        &self,
        phase_categories: Option<&[ReviewCategory]>,
        user_message: &str,
        event_tx: &tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<String> {
        use crate::core::types::Message;
        let messages = vec![
            Message::system(self.system_prompt(phase_categories)),
            Message::user(user_message),
        ];

        let mut chat_stream = self.client.stream_chat(&messages, &[]).await?;
        let mut full_content = String::new();

        while let Some(chunk_result) = chat_stream.next().await {
            let chunk = chunk_result?;
            for choice in &chunk.choices {
                let delta = &choice.delta;

                // Stream reasoning deltas in real-time
                if let Some(ref rt) = delta.reasoning_content {
                    if !rt.is_empty() {
                        let _ = event_tx.send(ReviewEvent::ReasoningDelta(rt.clone()));
                    }
                } else if let Some(ref rt) = delta.reasoning {
                    if !rt.is_empty() {
                        let _ = event_tx.send(ReviewEvent::ReasoningDelta(rt.clone()));
                    }
                }

                // Accumulate text content for later JSON parsing
                if let Some(ref text) = delta.content {
                    if !text.is_empty() {
                        full_content.push_str(text);
                    }
                }
            }
        }

        Ok(full_content)
    }

    /// Extract user's original request from conversation history for review context.
    ///
    /// Uses an improved strategy:
    /// 1. Takes the FIRST user message (original request) — the most important context
    /// 2. Includes the LAST assistant message before the review (what was implemented)
    /// 3. Includes the most recent follow-up user message (if any substantial one exists)
    /// 4. Caps total context at ~2000 characters
    pub fn extract_context_from_history(history: &[crate::core::types::Message]) -> String {
        let mut result = String::new();

        // Find positions of first user and last assistant messages
        let first_user_idx = history.iter().position(|m| m.role == "user");
        let last_assistant_idx = history.iter().rposition(|m| m.role == "assistant");

        // 1. Always include the first user message (original request)
        if let Some(idx) = first_user_idx {
            let content = clean_review_content(&history[idx].content);
            if !content.is_empty() {
                result.push_str("## Original Request\n");
                result.push_str(&truncate_content(&content, 1500));
                result.push_str("\n\n");
            }
        }

        // 2. Include last assistant message summary (what was implemented)
        if let Some(idx) = last_assistant_idx {
            let content = clean_review_content(&history[idx].content);
            if !content.is_empty() {
                result.push_str("## What Was Implemented\n");
                result.push_str(&truncate_content(&content, 1000));
                result.push_str("\n\n");
            }
        }

        // 3. If there are follow-up user messages between first and last assistant,
        //    include the most recent substantial one
        if let (Some(first_idx), Some(last_idx)) = (first_user_idx, last_assistant_idx) {
            for i in (first_idx + 1..last_idx).rev() {
                if history[i].role == "user" {
                    let content = clean_review_content(&history[i].content);
                    if !content.is_empty() && content.len() > 20 {
                        result.push_str("## Follow-up Context\n");
                        result.push_str(&truncate_content(&content, 500));
                        result.push_str("\n\n");
                        break;
                    }
                }
            }
        }

        // 4. Cap at 2000 characters (safely at UTF-8 char boundaries)
        if result.len() > 2000 {
            let boundary = char_boundary_at_or_before(&result, 2000);
            result.truncate(boundary);
            // Try to break at a newline for cleaner appearance
            let search_end = char_boundary_at_or_before(&result, 1997.min(result.len()));
            if let Some(last_newline) = result[..search_end].rfind('\n') {
                result.truncate(last_newline + 1);
            }
        }

        if result.is_empty() {
            // Fallback: return last user message
            if let Some(msg) = history.iter().rev().find(|m| m.role == "user") {
                result = clean_review_content(&msg.content);
            }
        }

        result
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

        // Append file context from disk — only for phases WITHOUT full diff
        // (Phase 2: signatures, Phase 3: compressed). Phase 1 already has the
        // complete diff, so file context would be redundant and waste tokens.
        // This provides function signatures, struct/enum/trait declarations,
        // and imports that may be outside the diff range, preventing false
        // positives where the LLM sees a symbol used but can't find its definition.
        if !include_diff {
            summary.push_str(&Self::format_file_context(files));
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

            if Self::is_declaration_line(content) {
                // Take the full line up to the first { or ; for compactness
                let sig = content.split(['{', ';', '=']).next().unwrap_or(content).trim();
                sigs.push(sig.to_string());
            }
        }

        sigs
    }

    /// Check if a line of code is a declaration (fn, struct, enum, trait, etc.)
    /// across multiple languages.
    pub fn is_declaration_line(line: &str) -> bool {
        line.starts_with("fn ")
            || line.starts_with("pub fn ")
            || line.starts_with("pub async fn ")
            || line.starts_with("async fn ")
            || line.starts_with("pub struct ")
            || line.starts_with("struct ")
            || line.starts_with("pub enum ")
            || line.starts_with("enum ")
            || line.starts_with("pub trait ")
            || line.starts_with("trait ")
            || line.starts_with("impl ")
            || line.starts_with("pub type ")
            || line.starts_with("type ")
            || line.starts_with("pub const ")
            || line.starts_with("const ")
            || line.starts_with("use ")
            || line.starts_with("macro_rules!")
            || line.starts_with("def ")
            || line.starts_with("async def ")
            || line.starts_with("class ")
            || line.starts_with("function ")
            || line.starts_with("export function ")
            || line.starts_with("export default ")
            || line.starts_with("export class ")
            || line.starts_with("export interface ")
            || line.starts_with("interface ")
            || line.starts_with("import ")
            || line.starts_with("from ")
    }

    /// Read changed files from disk and extract key declarations (function signatures,
    /// struct, enum, trait definitions, imports, etc.) to provide full-file context
    /// alongside the diff. This prevents false positives where the LLM sees a function
    /// being used but can't see its definition because it's outside the diff range.
    ///
    /// Limits: MAX_SIGNATURES_PER_FILE signatures per file, MAX_TOTAL_CHARS total.
    pub fn format_file_context(files: &[ChangedFile]) -> String {
        const MAX_SIGNATURES_PER_FILE: usize = 80;
        const MAX_TOTAL_CHARS: usize = 4000;

        let mut context = String::new();
        let mut has_content = false;

        for file in files {
            // Skip deleted files — they no longer exist on disk
            if file.change_type == ChangeType::Deleted {
                continue;
            }

            let path = std::path::Path::new(&file.path);
            if !path.exists() {
                continue;
            }

            // Read file content
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Extract key declarations from the file
            let sigs = Self::extract_signatures_from_content(&content);
            if sigs.is_empty() {
                continue;
            }

            if !has_content {
                context.push_str(
                    "## File Context (key declarations from source files)\n\n"
                );
                context.push_str(
                    "This section shows key declarations from the actual source files "
                );
                context.push_str(
                    "to help you understand the full context beyond just the diff:\n\n"
                );
                has_content = true;
            }

            context.push_str(&format!("### `{}`\n", file.path));
            context.push_str("```\n");
            let display_sigs: Vec<&str> = sigs.iter().take(MAX_SIGNATURES_PER_FILE).map(|s| s.as_str()).collect();
            for sig in &display_sigs {
                context.push_str(sig);
                context.push('\n');
            }
            if sigs.len() > MAX_SIGNATURES_PER_FILE {
                context.push_str(&format!(
                    "... and {} more declarations\n",
                    sigs.len() - MAX_SIGNATURES_PER_FILE
                ));
            }
            context.push_str("```\n\n");

            // Early exit if we've accumulated enough context
            if context.len() > MAX_TOTAL_CHARS {
                break;
            }
        }

        context
    }

    /// Extract function/struct/enum/trait/import signatures from a file's content.
    /// Similar to `extract_signatures_from_diff` but applies to raw file content
    /// without the `+` prefix stripping needed for diff lines.
    pub fn extract_signatures_from_content(content: &str) -> Vec<String> {
        let mut sigs = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if Self::is_declaration_line(trimmed) {
                // Take the full line up to the first { or ; for compactness
                let sig = trimmed.split(['{', ';', '=']).next().unwrap_or(trimmed).trim();
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
        let sanitized = sanitize_json_escapes(&json_str);
        let parsed: serde_json::Value = match serde_json::from_str(&sanitized) {
            Ok(v) => v,
            Err(_) => {
                // Attempt JSON repair for truncated/malformed responses
                let repaired = repair_truncated_json(&sanitized);
                serde_json::from_str(&repaired)?
            }
        };

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

    /// Execute parallel review — runs multiple focused review prompts concurrently
    /// and merges the results, deduplicating overlapping issues.
    pub async fn review_parallel(
        &self,
        request: &ReviewRequest,
        focus_prompts: Vec<String>,
    ) -> Result<ReviewReport> {
        let focuses = if focus_prompts.is_empty() {
            DEFAULT_FOCUSES.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        } else {
            focus_prompts
        };

        let changes_summary = self.format_changes_summary(&request.changed_files);

        let base_message = if let Some(ref context) = request.context {
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

        // Spawn concurrent review tasks for each focus area
        let tasks: Vec<_> = focuses.into_iter().map(|focus| {
            let message = format!(
                "{}\n\n## Review Phase Focus\nFocus your review specifically on the following aspect:\n{focus}",
                base_message, focus = focus,
            );

            async move {
                let (response, _reasoning) = self.call_llm(None, &message).await?;
                self.parse_issues_from_response(&response)
            }
        }).collect();

        let results = join_all(tasks).await;

        let mut all_issues = Vec::new();
        for result in results {
            match result {
                Ok(issues) => all_issues.extend(issues),
                Err(e) => {
                    tracing::warn!("Parallel review task failed: {}", e);
                }
            }
        }

        // Deduplicate by (file, line)
        all_issues = Self::deduplicate_issues(all_issues);

        self.build_report(&all_issues, &request.changed_files)
    }

    /// Deduplicate issues: two issues are duplicates if they share the same file AND line number.
    fn deduplicate_issues(issues: Vec<ReviewIssue>) -> Vec<ReviewIssue> {
        let mut seen = std::collections::HashSet::new();
        let mut deduped = Vec::new();
        for issue in issues {
            let key = (issue.file.clone(), issue.line);
            if seen.insert(key) {
                deduped.push(issue);
            }
        }
        deduped
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

/// Sanitize a JSON string by fixing invalid escape sequences.
///
/// LLMs frequently produce JSON with invalid escapes like `\x`, `\uGGGG`,
/// or unescaped backslashes in Windows paths (`C:\Users\test`).
/// This function converts invalid `\X` → `\\X` so that `serde_json::from_str`
/// can parse the result without error.
///
/// Valid JSON escapes (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`)
/// are left untouched.
pub fn sanitize_json_escapes(json: &str) -> String {
    let mut result = String::with_capacity(json.len() + json.len() / 20);
    let mut chars = json.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }
        if !in_string || ch != '\\' {
            result.push(ch);
            continue;
        }

        // We're inside a string and just saw a backslash — check what follows
        let Some(next) = chars.peek() else {
            // Trailing backslash at end of string — escape it
            result.push_str("\\\\");
            break;
        };

        match next {
            // Valid JSON escape sequences — keep as-is
            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                result.push(ch);
                result.push(chars.next().unwrap());
            }
            // Unicode escape: must be \u followed by exactly 4 hex digits
            'u' => {
                // Peek ahead to check 4 hex digits
                let mut hex_valid = true;
                let mut hex_chars = Vec::new();
                for _ in 0..4 {
                    chars.next(); // consume 'u' on first iteration
                    if let Some(h) = chars.peek() {
                        if h.is_ascii_hexdigit() {
                            hex_chars.push(*h);
                        } else {
                            hex_valid = false;
                            break;
                        }
                    } else {
                        hex_valid = false;
                        break;
                    }
                }
                if hex_valid {
                    // Valid \uXXXX — keep entire sequence
                    result.push('\\');
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                } else {
                    // Invalid unicode escape — treat backslash as literal
                    result.push_str("\\\\");
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                }
            }
            // Invalid escape — double the backslash to make it literal
            _ => {
                result.push_str("\\\\");
                result.push(chars.next().unwrap());
            }
        }
    }

    result
}

fn clean_review_content(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            !line.contains("fix the issues found in the code review")
                && !line.contains("Auto-Review Iteration")
                && !line.contains("Fix Required")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Find the largest byte index ≤ `max` that is a valid UTF-8 char boundary.
fn char_boundary_at_or_before(s: &str, max: usize) -> usize {
    s.char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Safely truncate a string to at most `max_bytes` bytes, appending "..." if truncated.
/// Never panics on multi-byte UTF-8 characters.
fn truncate_content(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let boundary = char_boundary_at_or_before(content, max_bytes);
    let mut s = content[..boundary].to_string();
    s.push_str("...");
    s
}

/// Attempt to repair a truncated/malformed JSON string from LLM output.
///
/// LLM responses sometimes get cut off mid-JSON (typically hitting output token limits).
/// This function handles the common truncation patterns:
/// 1. Trailing comma before closing bracket (`...,` → `...}`)
/// 2. Unclosed string literals (appends `"`)
/// 3. Unbalanced braces `{}` and brackets `[]` (appends missing closers)
///
/// Uses proper string-aware tracking to avoid misinterpreting
/// braces/brackets inside string values.
pub fn repair_truncated_json(s: &str) -> String {
    let mut result = s.trim_end().to_string();

    if let Some('}' | ']') = result.chars().last() {
        let closer = result.pop().unwrap();
        result = result.trim_end_matches(',').to_string();
        result.push(closer);
    }

    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut stack: Vec<char> = Vec::new();

    for ch in result.chars() {
        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if in_string {
            continue;
        }

        match ch {
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if let Some(&top) = stack.last() {
                    if top == ch {
                        stack.pop();
                    }
                }
            }
            _ => {}
        }
    }

    if in_string {
        result.push('"');
    }

    while let Some(closing) = stack.pop() {
        result.push(closing);
    }

    result
}
