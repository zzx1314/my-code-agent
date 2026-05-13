use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::time::Duration;

use crate::core::types::{Message, StreamChunk, ToolDefinition};

/// HTTP client for OpenAI-compatible Chat Completions API.
///
/// Replaces `rig::providers::openai::CompletionsClient` and
/// `rig::providers::openrouter::Client`.
pub struct LlmClient {
    http_client: reqwest::Client,
    base_url: String,
    api_key: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    pub timeout_secs: u64,
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
        }
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

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .context("Invalid API key format")?;
        headers.insert(AUTHORIZATION, auth_value);
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
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
        });

        if let Some(max_tokens) = self.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
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
        }

        body
    }

    /// Send a streaming chat request and return an SSE event stream.
    pub async fn stream_chat(
        &self,
        messages: &[Message],
        tool_definitions: &[ToolDefinition],
    ) -> Result<ChatStream> {
        let body = self.build_request_body(messages, tool_definitions, true);
        let headers = self.headers()?;

        let response = self
            .http_client
            .post(&self.chat_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send chat request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
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
    ) -> Result<serde_json::Value> {
        let body = self.build_request_body(messages, tool_definitions, false);
        let headers = self.headers()?;

        let response = self
            .http_client
            .post(&self.chat_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send chat request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Chat API error ({}): {}", status, text);
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse chat response")?;

        Ok(json)
    }
}

/// SSE event stream from the Chat Completions API.
pub struct ChatStream {
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
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
                    return Some(Err(anyhow::anyhow!("Failed to parse SSE chunk: {} (raw: {})", e, &data[..data.len().min(200)])))
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
                                )))
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
                                    )))
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
