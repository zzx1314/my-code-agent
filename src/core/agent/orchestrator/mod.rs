//! AgentOrchestrator — Multi-Agent Collaboration Coordinator
//!
//! Manages the collaboration flow between the main Agent and the review Agent:
//! 1. Automatically triggers review after the main Agent completes code changes
//! 2. Supports manual `/review` command
//! 3. Detects changed files and generates review reports

mod changes;
mod report;

pub use changes::{detect_changed_files_from_git, parse_git_diff};

use std::sync::Arc;

use anyhow::Result;

use super::preamble::Agent;
use super::review::{ReviewAgent, ReviewEvent, ReviewRequest};
use crate::core::config::Config;
use crate::core::types::Message;
use crate::core::types::review::*;

/// Multi-Agent Coordinator
pub struct AgentOrchestrator {
    /// Main agent (handles daily tasks)
    pub main_agent: Arc<Agent>,
    /// Review agent (dedicated to code review)
    pub review_agent: Arc<ReviewAgent>,
    /// Review configuration
    pub config: ReviewConfig,
    /// Whether auto-review is enabled
    pub auto_review_enabled: bool,
}

impl AgentOrchestrator {
    /// Create a new coordinator
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

    /// Derive a review agent from the main agent
    ///
    /// The review agent does NOT register tools — it sends diffs directly
    /// to the LLM and expects a JSON response.
    fn build_review_agent(
        main_agent: &Agent,
        config: &Config,
        review_config: &ReviewConfig,
    ) -> ReviewAgent {
        ReviewAgent::new(
            main_agent.client.clone(),
            review_config.clone(),
            config.llm.reasoning_field.clone(),
            config.agent.thinking_display.clone(),
        )
    }

    /// Execute review (synchronously wait for result)
    pub async fn review(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        history_summary: Option<&str>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
            history_summary: history_summary.map(|s| s.to_string()),
        };

        self.review_agent.review(&request).await
    }

    pub async fn review_with_events(
        &self,
        changed_files: Vec<ChangedFile>,
        context: Option<&str>,
        history_summary: Option<&str>,
        event_tx: tokio::sync::mpsc::UnboundedSender<ReviewEvent>,
    ) -> Result<ReviewReport> {
        let request = ReviewRequest {
            changed_files,
            context: context.map(|s| s.to_string()),
            history_summary: history_summary.map(|s| s.to_string()),
        };

        self.review_agent
            .review_with_events(&request, event_tx)
            .await
    }

    /// Determine whether auto-review should be triggered
    ///
    /// Checks the **current turn** (messages after the last user message) for
    /// write operations, preventing auto-review from re-triggering on subsequent
    /// interactions that don't involve new file changes.
    ///
    /// This is important because the LLM often follows up a file_write/file_update
    /// tool call with a text-only response (e.g. "Done!"): if we only checked the
    /// **last** assistant message, we would miss the write operation.
    pub fn should_auto_review(&self, history: &[Message]) -> bool {
        if !self.auto_review_enabled || !self.config.enabled {
            return false;
        }

        // Check all assistant messages within the current turn (after the last
        // user message). The LLM often follows up a tool call with a text-only
        // response, so we can't just look at the LAST assistant message.
        let has_recent_write = history
            .iter()
            .rev()
            .take_while(|msg| msg.role != "user")
            .filter(|msg| msg.role == "assistant")
            .filter_map(|msg| msg.tool_calls.as_ref())
            .flat_map(|tcs| tcs.iter())
            .any(|tc| {
                tc.function.name == "file_write"
                    || tc.function.name == "file_update"
                    || tc.function.name == "file_delete"
                    || tc.function.name == "apply_patch"
            });

        has_recent_write
    }
}

// Type alias for simplified imports
pub type OrchestratorRef = std::sync::Arc<AgentOrchestrator>;
