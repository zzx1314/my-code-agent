//! Parallel Search MCP tool adapter.
//! 
//! Uses https://search.parallel.ai/mcp for web search and fetch.

use crate::mcp::McpHttpClient;
use rig::completion::ToolDefinition as RigToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json;
use thiserror::Error;

#[derive(Deserialize, Serialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Deserialize, Serialize)]
pub struct WebFetchArgs {
    pub url: String,
}

#[derive(Serialize)]
pub struct WebSearchOutput {
    pub results: String,
}

#[derive(Debug, Error)]
pub enum WebSearchError {
    #[error("WebSearch tool not available. Set PARALLEL_API_KEY in config or .env")]
    NotAvailable,
    #[error("Web search failed: {0}")]
    SearchFailed(String),
    #[error("Search error occurred")]
    SearchError,
}

pub struct ParallelWebSearch {
    client: Option<McpHttpClient>,
}

impl ParallelWebSearch {
    pub fn new(api_key: &str) -> Self {
        let client = McpHttpClient::new("https://search.parallel.ai/mcp", Some(api_key));
        Self {
            client: Some(client),
        }
    }

    pub fn is_available(&self) -> bool {
        self.client.is_some()
    }
}

impl Tool for ParallelWebSearch {
    const NAME: &'static str = "web_search";
    type Error = WebSearchError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self, _prompt: String) -> RigToolDefinition {
        RigToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web using Parallel Search MCP. \
                Returns relevant web results with titles, URLs, and snippets."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "number",
                        "description": "Number of results (default: 10)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let client = self.client.as_ref().ok_or(WebSearchError::NotAvailable)?;

        let call_args = serde_json::json!({
            "query": args.query,
            "count": args.count
        });

        match client.call_tool("web_search", call_args).await {
            Ok(result) => {
                if result.is_error {
                    return Err(WebSearchError::SearchError);
                }
                let mut output = String::new();
                for content in &result.content {
                    if let crate::mcp::types::Content::Text { text } = content {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                Ok(WebSearchOutput { results: output })
            }
            Err(e) => Err(WebSearchError::SearchFailed(e.to_string()))
        }
    }
}

pub struct ParallelWebFetch {
    client: Option<McpHttpClient>,
}

impl ParallelWebFetch {
    pub fn new(api_key: &str) -> Self {
        let client = McpHttpClient::new("https://search.parallel.ai/mcp", Some(api_key));
        Self {
            client: Some(client),
        }
    }

    pub fn is_available(&self) -> bool {
        self.client.is_some()
    }
}

impl Tool for ParallelWebFetch {
    const NAME: &'static str = "web_fetch";
    type Error = WebSearchError;
    type Args = WebFetchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self, _prompt: String) -> RigToolDefinition {
        RigToolDefinition {
            name: Self::NAME.to_string(),
            description: "Extract content from a URL in markdown format."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let client = self.client.as_ref().ok_or(WebSearchError::NotAvailable)?;

        let call_args = serde_json::json!({
            "url": args.url
        });

        match client.call_tool("web_fetch", call_args).await {
            Ok(result) => {
                if result.is_error {
                    return Err(WebSearchError::SearchError);
                }
                let mut output = String::new();
                for content in &result.content {
                    if let crate::mcp::types::Content::Text { text } = content {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                Ok(WebSearchOutput { results: output })
            }
            Err(e) => Err(WebSearchError::SearchFailed(e.to_string()))
        }
    }
}