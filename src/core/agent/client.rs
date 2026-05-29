//! LLM client backed by the `rig` framework.
//!
//! Uses `rig::providers::openai::CompletionsClient` for OpenAI-compatible providers
//! (DeepSeek, Custom, Ollama, etc.) and `rig::providers::openrouter::Client` for
//! OpenRouter (which handles HTTP-Referer / X-OpenRouter-Title headers internally).

use anyhow::{Context, Result};
use futures::{StreamExt, future};
use rig::client::CompletionClient;
use rig::completion::message::{
    Text, ToolCall, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use rig::completion::{
    AssistantContent, CompletionModel, Message as RigMessage, ToolDefinition as RigToolDef,
};
use rig::one_or_many::OneOrMany;
use rig::providers::openai;
use rig::providers::openrouter;
use rig::streaming::{StreamedAssistantContent, ToolCallDeltaContent};
use tracing::debug;

use crate::core::types::{
    FinishReason, Message, StreamChoice, StreamChunk, StreamDelta, StreamToolCallDelta,
    StreamToolCallFunctionDelta, ToolDefinition,
};

// ─────────────────────────────────────────────────────────────────────────────
// Provider type enum
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderType {
    DeepSeek,
    OpenAI,
    OpenRouter,
    Custom,
    Ollama,
}

impl ProviderType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "deepseek" => Some(ProviderType::DeepSeek),
            "openai" => Some(ProviderType::OpenAI),
            "openrouter" => Some(ProviderType::OpenRouter),
            "custom" => Some(ProviderType::Custom),
            "ollama" => Some(ProviderType::Ollama),
            _ => None,
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            ProviderType::DeepSeek => "deepseek-v4-flash",
            ProviderType::OpenAI => "gpt-4o",
            ProviderType::OpenRouter => "openrouter/owl-alpha",
            ProviderType::Custom => "gpt-4o",
            ProviderType::Ollama => "llama3.2",
        }
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderType::DeepSeek => "https://api.deepseek.com/v1",
            ProviderType::OpenAI => "https://api.openai.com/v1",
            ProviderType::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderType::Custom => "",
            ProviderType::Ollama => "http://localhost:11434/v1",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RigClient — wraps the two possible rig client types
// ─────────────────────────────────────────────────────────────────────────────

enum RigClient {
    OpenAi(openai::CompletionsClient),
    OpenRouter(openrouter::Client),
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamingResponseExt — uniform usage extraction for streaming responses
// ─────────────────────────────────────────────────────────────────────────────

/// Trait to extract usage info from different provider streaming response types.
trait StreamingResponseExt {
    fn extract_usage(&self) -> Option<crate::core::types::Usage>;
}

impl StreamingResponseExt for openai::StreamingCompletionResponse {
    fn extract_usage(&self) -> Option<crate::core::types::Usage> {
        serde_json::to_value(&self.usage)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
    }
}

impl StreamingResponseExt for openrouter::streaming::StreamingCompletionResponse {
    fn extract_usage(&self) -> Option<crate::core::types::Usage> {
        serde_json::to_value(&self.usage)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LlmClient — rig-backed LLM client
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP client for Chat Completions API, backed by `rig-core`.
#[derive(Clone)]
pub struct LlmClient {
    provider: ProviderType,
    base_url: String,
    api_key: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    pub timeout_secs: u64,
    reasoning_disabled: bool,
    use_completion_tokens: bool,
    temperature: Option<f64>,
    top_p: Option<f64>,
    stop: Option<Vec<String>>,
    frequency_penalty: Option<f64>,
    presence_penalty: Option<f64>,
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        let provider = ProviderType::from_str(model)
            .or_else(|| {
                if base_url.contains("api.deepseek.com") {
                    Some(ProviderType::DeepSeek)
                } else if base_url.contains("openrouter.ai") {
                    Some(ProviderType::OpenRouter)
                } else if base_url.contains("localhost:11434")
                    || base_url.contains("127.0.0.1:11434")
                {
                    Some(ProviderType::Ollama)
                } else if base_url.contains("api.openai.com") {
                    Some(ProviderType::OpenAI)
                } else {
                    Some(ProviderType::Custom)
                }
            })
            .unwrap_or(ProviderType::Custom);

        Self {
            provider,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            max_tokens: None,
            timeout_secs: 0,
            reasoning_disabled: false,
            use_completion_tokens: false,
            temperature: None,
            top_p: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
        }
    }

    pub fn with_reasoning_disabled(mut self, disabled: bool) -> Self {
        self.reasoning_disabled = disabled;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        if secs > 0 {
            self.timeout_secs = secs;
        }
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_use_completion_tokens(mut self, val: bool) -> Self {
        self.use_completion_tokens = val;
        self
    }

    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn with_frequency_penalty(mut self, penalty: f64) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    pub fn with_presence_penalty(mut self, penalty: f64) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    fn build_rig_client(&self) -> Result<RigClient> {
        // Build an HTTP client with the configured timeout.
        // rig-core's ClientBuilder defaults to reqwest::Client::default() which
        // has no timeout. We inject a custom client via http_client().
        // Note: use rig's own ReqwestClient type alias (from reqwest 0.13)
        // to match the version rig-core was built against.
        let mut http_builder = rig::http_client::ReqwestClient::builder();
        if self.timeout_secs > 0 {
            http_builder = http_builder.timeout(std::time::Duration::from_secs(self.timeout_secs));
        }
        let http_client = http_builder
            .build()
            .context("Failed to build HTTP client with timeout")?;

        match self.provider {
            ProviderType::OpenRouter => {
                let client = openrouter::Client::builder()
                    .api_key(&self.api_key)
                    .base_url(&self.base_url)
                    .http_client(http_client)
                    .build()
                    .context("Failed to build rig OpenRouter client")?;
                Ok(RigClient::OpenRouter(client))
            }
            _ => {
                let client = openai::CompletionsClient::builder()
                    .api_key(&self.api_key)
                    .base_url(&self.base_url)
                    .http_client(http_client)
                    .build()
                    .context("Failed to build rig OpenAI client")?;
                Ok(RigClient::OpenAi(client))
            }
        }
    }

    /// Convert our custom Message slice to rig Messages.
    fn convert_to_rig_messages(&self, messages: &[Message]) -> Vec<RigMessage> {
        let mut rig_messages: Vec<RigMessage> = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    rig_messages.push(RigMessage::System {
                        content: msg.content.clone(),
                    });
                }
                "user" => {
                    rig_messages.push(RigMessage::User {
                        content: OneOrMany::one(UserContent::Text(Text::from(msg.content.clone()))),
                    });
                }
                "assistant" => {
                    let mut contents = Vec::new();

                    if !msg.content.is_empty() {
                        contents.push(AssistantContent::Text(Text::from(msg.content.clone())));
                    }

                    if let Some(ref tcs) = msg.tool_calls {
                        for tc in tcs {
                            let args: serde_json::Value = match serde_json::from_str(
                                &tc.function.arguments,
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    tracing::warn!(err = %e, "Failed to parse tool call arguments as JSON; using null");
                                    serde_json::Value::Null
                                }
                            };
                            let tool_call = ToolCall {
                                id: tc.id.clone(),
                                call_id: None,
                                function: ToolFunction {
                                    name: tc.function.name.clone(),
                                    arguments: args,
                                },
                                signature: None,
                                additional_params: None,
                            };
                            contents.push(AssistantContent::ToolCall(tool_call));
                        }
                    }

                    rig_messages.push(RigMessage::Assistant {
                        id: None,
                        content: OneOrMany::many(contents)
                            .expect("assistant message must have at least one content item"),
                    });
                }
                "tool" => {
                    let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                    let tool_result = ToolResult {
                        id: tool_call_id,
                        call_id: None,
                        content: OneOrMany::one(ToolResultContent::Text(Text::from(
                            msg.content.clone(),
                        ))),
                    };
                    rig_messages.push(RigMessage::User {
                        content: OneOrMany::one(UserContent::ToolResult(tool_result)),
                    });
                }
                _ => {
                    rig_messages.push(RigMessage::User {
                        content: OneOrMany::one(UserContent::Text(Text::from(msg.content.clone()))),
                    });
                }
            }
        }

        rig_messages
    }

    /// Collect additional model parameters into a JSON value.
    fn additional_params(&self) -> serde_json::Value {
        let mut params = serde_json::json!({});

        if let Some(t) = self.temperature {
            params["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = self.top_p {
            params["top_p"] = serde_json::json!(p);
        }
        if let Some(ref stop) = self.stop {
            params["stop"] = serde_json::json!(stop);
        }
        if let Some(fp) = self.frequency_penalty {
            params["frequency_penalty"] = serde_json::json!(fp);
        }
        if let Some(pp) = self.presence_penalty {
            params["presence_penalty"] = serde_json::json!(pp);
        }
        if let Some(m) = self.max_tokens {
            if self.use_completion_tokens {
                params["max_completion_tokens"] = serde_json::json!(m);
            } else {
                params["max_tokens"] = serde_json::json!(m);
            }
        }
        if self.reasoning_disabled {
            // `reasoning: {enabled: false}` is OpenRouter-specific.
            // DeepSeek, OpenAI, and other OpenAI-compatible providers do not
            // support this parameter in the request body — their non-reasoning
            // models simply don't produce reasoning output, and reasoning
            // models cannot disable it via a parameter. Only send for OpenRouter.
            if self.provider == ProviderType::OpenRouter {
                params["reasoning"] = serde_json::json!({"enabled": false});
            }
        }
        params
    }

    /// Normalize the response JSON to convert array-formatted content back to a string.
    /// Providers like OpenRouter and OpenAI (via rig's typed structs) serialize assistant
    /// message content as `[{"type": "text", "text": "..."}]` instead of a plain string.
    /// This function extracts the text so callers can use `.as_str()` consistently.
    fn normalize_response_content(mut value: serde_json::Value) -> serde_json::Value {
        let choices = match value.get_mut("choices").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => return value,
        };

        for choice in choices {
            let content = match choice
                .get_mut("message")
                .and_then(|m| m.get_mut("content"))
            {
                Some(c) => c,
                None => continue,
            };

            // Extract text via immutable borrow, then replace in place
            if let Some(text) = extract_text_from_array(content) {
                *content = serde_json::Value::String(text);
            }
        }

        value
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Send a non-streaming chat request.
    pub async fn chat(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
        _reasoning_field: &str,
    ) -> Result<serde_json::Value> {
        let client = self.build_rig_client()?;
        let rig_messages = self.convert_to_rig_messages(messages);

        let (prompt, chat_history) = Self::split_prompt_and_history(rig_messages);

        let params = self.additional_params();

        debug!(
            model = %self.model,
            message_count = messages.len(),
            tool_count = tool_definitions.len(),
            stream = false,
            "Sending chat request via rig"
        );

        match client {
            RigClient::OpenAi(c) => {
                let model = c.completion_model(&self.model);
                let mut builder = model.completion_request(&prompt);
                if !chat_history.is_empty() {
                    builder = builder.messages(chat_history);
                }
                builder = builder.additional_params(params);
                for td in tool_definitions {
                    builder = builder.tool(RigToolDef {
                        name: td.name.clone(),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
                let response = builder
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("Rig completion error: {e}"))?;
                serde_json::to_value(&response.raw_response)
                    .context("Failed to serialize rig response to JSON")
                    .map(Self::normalize_response_content)
            }
            RigClient::OpenRouter(c) => {
                let model = c.completion_model(&self.model);
                let mut builder = model.completion_request(&prompt);
                if !chat_history.is_empty() {
                    builder = builder.messages(chat_history);
                }
                builder = builder.additional_params(params);
                for td in tool_definitions {
                    builder = builder.tool(RigToolDef {
                        name: td.name.clone(),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
                let response = builder
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("Rig completion error: {e}"))?;
                serde_json::to_value(&response.raw_response)
                    .context("Failed to serialize rig response to JSON")
                    .map(Self::normalize_response_content)
            }
        }
    }

    /// Send a streaming chat request and return a `ChatStream`.
    pub async fn stream_chat(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
        _reasoning_field: &str,
    ) -> Result<ChatStream> {
        let client = self.build_rig_client()?;
        let rig_messages = self.convert_to_rig_messages(messages);

        let (prompt, chat_history) = Self::split_prompt_and_history(rig_messages);

        let params = self.additional_params();

        debug!(
            model = %self.model,
            message_count = messages.len(),
            tool_count = tool_definitions.len(),
            stream = true,
            "Sending streaming chat request via rig"
        );

        match client {
            RigClient::OpenAi(c) => {
                let model = c.completion_model(&self.model);
                let mut builder = model.completion_request(&prompt);
                if !chat_history.is_empty() {
                    builder = builder.messages(chat_history);
                }
                builder = builder.additional_params(params);
                for td in tool_definitions {
                    builder = builder.tool(RigToolDef {
                        name: td.name.clone(),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
                let rig_stream = builder
                    .stream()
                    .await
                    .map_err(|e| anyhow::anyhow!("Rig stream error: {e}"))?;

                let mapped = rig_stream.scan(false, |had_tool_calls, item| {
                    let result = match item {
                        Ok(content) => Self::stream_content_to_chunk(content, had_tool_calls),
                        Err(e) => Some(Err(anyhow::anyhow!("Stream error: {e}"))),
                    };
                    future::ready(result)
                });
                Ok(ChatStream {
                    stream: Box::pin(mapped),
                })
            }
            RigClient::OpenRouter(c) => {
                let model = c.completion_model(&self.model);
                let mut builder = model.completion_request(&prompt);
                if !chat_history.is_empty() {
                    builder = builder.messages(chat_history);
                }
                builder = builder.additional_params(params);
                for td in tool_definitions {
                    builder = builder.tool(RigToolDef {
                        name: td.name.clone(),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
                let rig_stream = builder
                    .stream()
                    .await
                    .map_err(|e| anyhow::anyhow!("Rig stream error: {e}"))?;

                let mapped = rig_stream.scan(false, |had_tool_calls, item| {
                    let result = match item {
                        Ok(content) => Self::stream_content_to_chunk(content, had_tool_calls),
                        Err(e) => Some(Err(anyhow::anyhow!("Stream error: {e}"))),
                    };
                    future::ready(result)
                });
                Ok(ChatStream {
                    stream: Box::pin(mapped),
                })
            }
        }
    }

    /// Split rig messages into prompt and chat_history.
    ///
    /// Normally the last message is a user text message which becomes the prompt.
    /// But in a tool loop continuation, the last message may be a tool result
    /// (`UserContent::ToolResult`). We detect this case and extract the tool
    /// result text as the prompt instead, keeping all previous messages
    /// (including other tool results) in chat_history.
    fn split_prompt_and_history(
        rig_messages: Vec<RigMessage>,
    ) -> (String, Vec<RigMessage>) {
        // Check if the last message is a tool result (tool loop continuation)
        let is_tool_continuation = rig_messages.last().map_or(false, |m| match m {
            RigMessage::User { content } => content
                .iter()
                .any(|c| matches!(c, UserContent::ToolResult(_))),
            _ => false,
        });

        if is_tool_continuation {
            // ── Tool loop continuation ───────────────────────────────
            // Extract the tool result text as the prompt, keep previous
            // messages (including other tool results) in chat_history.
            // The model sees the tool results in the message array and
            // can continue naturally.
            let prompt = rig_messages.last().map_or(String::new(), |m| match m {
                RigMessage::User { content } => content
                    .iter()
                    .filter_map(|c| match c {
                        UserContent::ToolResult(t) => t
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                ToolResultContent::Text(t) => Some(t.text().to_string()),
                                _ => None,
                            })
                            .next(),
                        _ => None,
                    })
                    .next()
                    .unwrap_or_default(),
                _ => String::new(),
            });
            let chat_history: Vec<RigMessage> = if rig_messages.len() > 1 {
                rig_messages[..rig_messages.len() - 1].to_vec()
            } else {
                Vec::new()
            };
            (prompt, chat_history)
        } else {
            // ── Normal case: last message is user text ────────────────
            let prompt = rig_messages.last().map_or(String::new(), |m| match m {
                RigMessage::User { content } => content
                    .iter()
                    .filter_map(|c| match c {
                        UserContent::Text(t) => Some(t.text().to_string()),
                        UserContent::ToolResult(t) => t
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                ToolResultContent::Text(t) => Some(t.text().to_string()),
                                _ => None,
                            })
                            .next(),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            });

            let chat_history: Vec<RigMessage> = if rig_messages.len() > 1 {
                rig_messages[..rig_messages.len() - 1].to_vec()
            } else {
                Vec::new()
            };

            (prompt, chat_history)
        }
    }

    /// Convert a single rig `StreamedAssistantContent` item to our `StreamChunk`.
    fn stream_content_to_chunk<T: StreamingResponseExt>(
        content: StreamedAssistantContent<T>,
        had_tool_calls: &mut bool,
    ) -> Option<Result<StreamChunk>> {
        match content {
            StreamedAssistantContent::Text(text) => Some(Ok(StreamChunk {
                choices: vec![StreamChoice {
                    delta: StreamDelta {
                        role: Some("assistant".to_string()),
                        content: Some(text.text().to_string()),
                        reasoning_content: None,
                        reasoning: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                    index: 0,
                }],
                usage: None,
            })),
            StreamedAssistantContent::ReasoningDelta { reasoning, .. } => Some(Ok(StreamChunk {
                choices: vec![StreamChoice {
                    delta: StreamDelta {
                        role: Some("assistant".to_string()),
                        content: None,
                        reasoning_content: Some(reasoning),
                        reasoning: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                    index: 0,
                }],
                usage: None,
            })),
            StreamedAssistantContent::Reasoning(reasoning) => Some(Ok(StreamChunk {
                choices: vec![StreamChoice {
                    delta: StreamDelta {
                        role: Some("assistant".to_string()),
                        content: None,
                        reasoning_content: Some(reasoning.display_text()),
                        reasoning: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                    index: 0,
                }],
                usage: None,
            })),
            StreamedAssistantContent::ToolCall { tool_call, .. } => {
                *had_tool_calls = true;
                let args = serde_json::to_string(&tool_call.function.arguments).unwrap_or_default();
                Some(Ok(StreamChunk {
                    choices: vec![StreamChoice {
                        delta: StreamDelta {
                            role: Some("assistant".to_string()),
                            content: None,
                            reasoning_content: None,
                            reasoning: None,
                            tool_calls: Some(vec![StreamToolCallDelta {
                                index: 0,
                                id: Some(tool_call.id),
                                type_: Some("function".to_string()),
                                function: Some(StreamToolCallFunctionDelta {
                                    name: Some(tool_call.function.name),
                                    arguments: Some(args),
                                }),
                            }]),
                        },
                        finish_reason: Some(FinishReason::ToolCalls),
                        index: 0,
                    }],
                    usage: None,
                }))
            }
            StreamedAssistantContent::ToolCallDelta { id, content, .. } => {
                *had_tool_calls = true;
                let (name, args) = match content {
                    ToolCallDeltaContent::Name(n) => (Some(n), None),
                    ToolCallDeltaContent::Delta(d) => (None, Some(d)),
                };
                Some(Ok(StreamChunk {
                    choices: vec![StreamChoice {
                        delta: StreamDelta {
                            role: Some("assistant".to_string()),
                            content: None,
                            reasoning_content: None,
                            reasoning: None,
                            tool_calls: Some(vec![StreamToolCallDelta {
                                index: 0,
                                id: Some(id),
                                type_: Some("function".to_string()),
                                function: Some(StreamToolCallFunctionDelta {
                                    name,
                                    arguments: args,
                                }),
                            }]),
                        },
                        finish_reason: None,
                        index: 0,
                    }],
                    usage: None,
                }))
            }
            StreamedAssistantContent::Final(response) => {
                let finish_reason = if *had_tool_calls {
                    Some(FinishReason::ToolCalls)
                } else {
                    Some(FinishReason::Stop)
                };

                let usage = response.extract_usage();

                Some(Ok(StreamChunk {
                    choices: vec![StreamChoice {
                        delta: StreamDelta {
                            role: None,
                            content: None,
                            reasoning_content: None,
                            reasoning: None,
                            tool_calls: None,
                        },
                        finish_reason,
                        index: 0,
                    }],
                    usage,
                }))
            }
        }
    }
}

/// If `content` is a JSON array of `[{"type": "text", "text": "..."}]` items,
/// extract and join the text strings; otherwise return `None`.
fn extract_text_from_array(content: &serde_json::Value) -> Option<String> {
    let arr = content.as_array()?;
    let parts: Vec<&str> = arr
        .iter()
        .filter_map(|item| {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                item.get("text").and_then(|v| v.as_str())
            } else {
                None
            }
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ChatStream — wraps rig's streaming response (type-erased)
// ─────────────────────────────────────────────────────────────────────────────

/// SSE event stream backed by rig's streaming completion response.
pub struct ChatStream {
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamChunk>> + Send>>,
}

impl ChatStream {
    /// Read the next parsed chunk from the stream.
    pub async fn next(&mut self) -> Option<Result<StreamChunk>> {
        self.stream.next().await
    }
}
