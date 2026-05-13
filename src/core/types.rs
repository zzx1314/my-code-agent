//! Core message types replacing `rig::completion::*` types.
//!
//! These types map directly to the OpenAI Chat Completions API format
//! (also used by DeepSeek, OpenRouter, and compatible providers).

use serde::{Deserialize, Serialize};
use std::ops::AddAssign;

// ─────────────────────────────────────────────────────────────────────────────
// Token Usage (replaces `rig::completion::Usage`)
// ─────────────────────────────────────────────────────────────────────────────

/// Token usage data from an API response.
///
/// Different providers/models may omit some usage fields in their SSE
/// responses (e.g. deepseek-chat may not include `input_tokens` in
/// streaming chunks). The custom `Deserialize` impl handles this by
/// defaulting any missing field to 0.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

/// Custom deserializer that defaults missing usage fields to 0.
///
/// Different providers use different field names for token usage:
/// - OpenAI/DeepSeek (OpenAI-compatible): `prompt_tokens`, `completion_tokens`,
///   `total_tokens`, with cached tokens under `prompt_tokens_details.cached_tokens`
/// - OpenRouter: `input_tokens`, `output_tokens`, `total_tokens`,
///   `cached_input_tokens`, `cache_creation_input_tokens`
///
/// This deserializer handles both naming conventions, preferring the
/// OpenRouter names first as fallback to OpenAI-style names.
///
/// Using `serde_json::Value` as intermediary ensures any field that is
/// missing or has an unexpected type simply defaults to 0, rather than
/// causing a deserialization error.
impl<'de> serde::Deserialize<'de> for Usage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        Ok(Usage {
            input_tokens: v
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| v.get("prompt_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0),
            output_tokens: v
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| v.get("completion_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0),
            total_tokens: v.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            cached_input_tokens: v
                .get("cached_input_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    v.get("prompt_tokens_details")
                        .and_then(|d| d.get("cached_tokens"))
                        .and_then(|v| v.as_u64())
                })
                .unwrap_or(0),
            cache_creation_input_tokens: v
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        })
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool types (replaces `rig::completion::ToolDefinition`, `rig::tool::ToolCall`)
// ─────────────────────────────────────────────────────────────────────────────

/// Tool definition for API registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A tool call from the assistant, matching OpenAI API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Chat Message (replaces `rig::completion::Message`)
// ─────────────────────────────────────────────────────────────────────────────

/// A chat message in OpenAI API format.
///
/// This maps directly to the API wire format.
/// `reasoning_content` is a DeepSeek-specific extension for their reasoning
/// models — it MUST be included when sending previous assistant messages
/// back to the API (otherwise the API returns an error).
/// Tool calls are stored as a separate field on Assistant messages.
/// Tool results are stored as Tool-role messages with a `tool_call_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// DeepSeek reasoning content — must be echoed back to the API in
    /// subsequent requests when using reasoning models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with DeepSeek reasoning content.
    pub fn assistant_with_reasoning(
        content: impl Into<String>,
        reasoning_content: impl Into<String>,
    ) -> Self {
        let rc = reasoning_content.into();
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: if rc.is_empty() { None } else { Some(rc) },
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create an assistant message with both tool calls AND DeepSeek reasoning content.
    pub fn assistant_with_tool_calls_and_reasoning(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: impl Into<String>,
    ) -> Self {
        let rc = reasoning_content.into();
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: if rc.is_empty() { None } else { Some(rc) },
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE Streaming types
// ─────────────────────────────────────────────────────────────────────────────

/// A single chunk from the SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    pub index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// DeepSeek reasoning content (non-standard OpenAI extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamToolCallDelta {
    pub index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamToolCallFunctionDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamToolCallFunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
