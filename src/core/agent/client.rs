use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use std::time::Duration;
use tracing::{debug, warn};

use crate::core::types::{Message, StreamChunk, ToolDefinition};

/// HTTP client for OpenAI-compatible Chat Completions API.
///
/// Replaces `rig::providers::openai::CompletionsClient` and
/// `rig::providers::openrouter::Client`.
#[derive(Clone)]
pub struct LlmClient {
    http_client: reqwest::Client,
    base_url: String,
    api_key: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    pub timeout_secs: u64,
    /// When true, adds `"reasoning": false` to the request body.
    reasoning_disabled: bool,
    /// When true, uses `max_completion_tokens` instead of `max_tokens` in the request body.
    /// OpenAI's o-series models require this field.
    use_completion_tokens: bool,
    /// Sampling temperature (0-2). None = use provider default.
    temperature: Option<f64>,
    /// Nucleus sampling threshold (0-1). None = use provider default.
    top_p: Option<f64>,
    /// Stop sequences. None = no stop.
    stop: Option<Vec<String>>,
    /// Frequency penalty (-2 to 2). None = use provider default.
    frequency_penalty: Option<f64>,
    /// Presence penalty (-2 to 2). None = use provider default.
    presence_penalty: Option<f64>,
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            http_client: reqwest::Client::new(),
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
            self.http_client = reqwest::Client::builder()
                .timeout(Duration::from_secs(secs))
                .build()
                .expect("Failed to build reqwest client with timeout");
            self.timeout_secs = secs;
        }
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// When set, uses `max_completion_tokens` instead of `max_tokens` in the request body.
    /// This is required by OpenAI's o-series models (o1, o3-mini, etc.).
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

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Only send Authorization header if an API key is configured.
        // Providers without auth (e.g. local Ollama) skip the header entirely,
        // avoiding 401 errors from empty/tokenless Bearer headers.
        if !self.api_key.is_empty() {
            let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .context("Invalid API key format")?;
            headers.insert(AUTHORIZATION, auth_value);
        }

        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_static("https://github.com/my-code-agent"),
        );
        headers.insert(
            "X-OpenRouter-Title",
            HeaderValue::from_static("My Code Agent"),
        );
        Ok(headers)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    /// Build the request body for a chat completions request.
    fn build_request_body(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
        stream: bool,
        reasoning_field: &str,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
        });

        if let Some(max_tokens) = self.max_tokens {
            let field = if self.use_completion_tokens {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            body[field] = serde_json::json!(max_tokens);
        }

        if !tool_definitions.is_empty() {
            // Convert to OpenAI API format: [{"type": "function", "function": {...}}]
            let tools: Vec<serde_json::Value> = tool_definitions
                .iter()
                .map(|td| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": td.name,
                            "description": td.description,
                            "parameters": td.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }

        // Optional sampling parameters (only included when explicitly set)
        if let Some(temp) = self.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(p) = self.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if let Some(ref stop) = self.stop {
            body["stop"] = serde_json::json!(stop);
        }
        if let Some(fp) = self.frequency_penalty {
            body["frequency_penalty"] = serde_json::json!(fp);
        }
        if let Some(pp) = self.presence_penalty {
            body["presence_penalty"] = serde_json::json!(pp);
        }

        if stream {
            body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        // Disable model reasoning when requested (e.g. for translation calls
        // where thinking output is unnecessary and wastes tokens).
        if self.reasoning_disabled {
            body["reasoning"] = serde_json::json!({"enabled": false});
        }

        // Transform reasoning_content field name if needed
        if reasoning_field != "reasoning_content" {
            if let Some(msgs) = body["messages"].as_array_mut() {
                for msg in msgs.iter_mut() {
                    if let Some(obj) = msg.as_object_mut() {
                        if let Some(reasoning) = obj.remove("reasoning_content") {
                            obj.insert(reasoning_field.to_string(), reasoning);
                        }
                    }
                }
            }
        }

        body
    }

    /// Send a streaming chat request and return an SSE event stream.
    pub async fn stream_chat(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
        reasoning_field: &str,
    ) -> Result<ChatStream> {
        let body = self.build_request_body(messages, tool_definitions, true, reasoning_field);
        let headers = self.headers()?;

        debug!(
            url = %self.chat_url(),
            model = %self.model,
            message_count = messages.len(),
            tool_count = tool_definitions.len(),
            stream = true,
            "Sending streaming chat request to LLM"
        );
        debug!(request_body = %body, "LLM request body");

        let response = self
            .http_client
            .post(&self.chat_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send chat request")?;

        let status = response.status();
        debug!(status = %status, "Received LLM response status");

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %text, "LLM API error");
            anyhow::bail!("Chat API error ({}): {}", status, text);
        }

        Ok(ChatStream {
            stream: Box::pin(response.bytes_stream()),
            buffer: Vec::new(),
        })
    }

    /// Send a non-streaming chat request (used internally as fallback).
    #[allow(dead_code)]
    pub async fn chat(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
        reasoning_field: &str,
    ) -> Result<serde_json::Value> {
        let body = self.build_request_body(messages, tool_definitions, false, reasoning_field);
        let headers = self.headers()?;

        debug!(
            url = %self.chat_url(),
            model = %self.model,
            message_count = messages.len(),
            tool_count = tool_definitions.len(),
            stream = false,
            "Sending non-streaming chat request to LLM"
        );
        debug!(request_body = %body, "LLM request body");

        let response = self
            .http_client
            .post(&self.chat_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send chat request")?;

        let status = response.status();
        debug!(status = %status, "Received LLM response status");

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %text, "LLM API error");
            anyhow::bail!("Chat API error ({}): {}", status, text);
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse chat response")?;

        debug!("Successfully parsed LLM response");
        Ok(json)
    }
}

/// SSE event stream from the Chat Completions API.
pub struct ChatStream {
    stream:
        std::pin::Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: Vec<u8>,
}

impl ChatStream {
    /// Parse the next SSE event from the stream.
    ///
    /// Returns `None` when the stream is exhausted (including after `data: [DONE]`).
    fn parse_next_chunk(&mut self) -> Option<String> {
        loop {
            if let Some(end) = self.buffer.windows(2).position(|w| w == b"\n\n") {
                let event_bytes = self.buffer[..end].to_vec();
                self.buffer.drain(..=end + 1);

                let event_str = String::from_utf8_lossy(&event_bytes);
                let mut data_lines = Vec::new();

                for line in event_str.lines() {
                    let line = line.trim();
                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            return None;
                        }
                        data_lines.push(data.to_string());
                    } else if line == "[DONE]" {
                        return None;
                    }
                }

                if !data_lines.is_empty() {
                    let combined = data_lines.join("\n");
                    return Some(combined);
                }
                continue;
            }

            break;
        }

        None
    }

    /// Read the next parsed SSE chunk from the stream.
    pub async fn next(&mut self) -> Option<Result<StreamChunk>> {
        // First check if we have a complete event in the buffer
        if let Some(data) = self.parse_next_chunk() {
            match serde_json::from_str::<StreamChunk>(&data) {
                Ok(chunk) => return Some(Ok(chunk)),
                Err(e) => {
                    tracing::error!(raw_sse = %data, error = %e, "Failed to parse SSE chunk");
                    return Some(Err(anyhow::anyhow!(
                        "Failed to parse SSE chunk: {} (raw: {})",
                        e,
                        &data[..data.len().min(200)]
                    )));
                }
            }
        }

        // Read more data from the stream
        loop {
            match self.stream.next().await {
                Some(Ok(bytes)) => {
                    self.buffer.extend_from_slice(&bytes);

                    if let Some(data) = self.parse_next_chunk() {
                        match serde_json::from_str::<StreamChunk>(&data) {
                            Ok(chunk) => return Some(Ok(chunk)),
                            Err(e) => {
                                tracing::error!(
                                    raw_sse = %data,
                                    error = %e,
                                    "Failed to parse SSE chunk"
                                );
                                return Some(Err(anyhow::anyhow!(
                                    "Failed to parse SSE chunk: {} (raw: {})",
                                    e,
                                    &data[..data.len().min(200)],
                                )));
                            }
                        }
                    }
                }
                Some(Err(e)) => return Some(Err(anyhow::anyhow!("Stream error: {}", e))),
                None => {
                    if !self.buffer.is_empty() {
                        let remaining = String::from_utf8_lossy(&self.buffer).to_string();
                        self.buffer.clear();

                        let mut data_lines = Vec::new();
                        for line in remaining.lines() {
                            let line = line.trim();
                            if let Some(data) = line.strip_prefix("data: ") {
                                let data = data.trim();
                                if data == "[DONE]" {
                                    return None;
                                }
                                data_lines.push(data.to_string());
                            } else if line == "[DONE]" {
                                return None;
                            }
                        }

                        if !data_lines.is_empty() {
                            let combined = data_lines.join("\n");
                            match serde_json::from_str::<StreamChunk>(&combined) {
                                Ok(chunk) => return Some(Ok(chunk)),
                                Err(e) => {
                                    tracing::error!(raw_sse = %combined, error = %e, "Failed to parse trailing SSE");
                                    return Some(Err(anyhow::anyhow!(
                                        "Failed to parse trailing SSE: {} (raw: {})",
                                        e,
                                        &combined[..combined.len().min(200)],
                                    )));
                                }
                            }
                        }
                    }
                    return None;
                }
            }
        }
    }
}
