use crate::core::types::ToolDefinition;
use crate::mcp::McpHttpClient;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json;
use async_trait::async_trait;

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
        true
    }
}

#[async_trait]
impl Tool for ParallelWebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search the web using Parallel Search MCP. \
                Use objective (what to find) and search_queries (keywords)."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to search for"
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

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: WebSearchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let client = self.client.as_ref().ok_or_else(|| {
            "WebSearch tool not available. Set PARALLEL_API_KEY in config or .env".to_string()
        })?;

        let keywords: Vec<String> = args
            .query
            .split_whitespace()
            .take(3)
            .map(String::from)
            .collect();
        let call_args = serde_json::json!({
            "objective": args.query,
            "search_queries": keywords,
            "session_id": "my-code-agent-session"
        });

        match client.call_tool("web_search", call_args).await {
            Ok(result) => {
                if result.is_error {
                    return Err("Search error occurred".to_string());
                }
                let mut output = String::new();
                for content in &result.content {
                    if let crate::mcp::types::Content::Text { text } = content {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                serde_json::to_string(&WebSearchOutput { results: output })
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(format!("Web search failed: {}", e)),
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
        true
    }
}

#[async_trait]
impl Tool for ParallelWebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Extract content from a URL in markdown format.".to_string(),
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

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: WebFetchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let client = self.client.as_ref().ok_or_else(|| {
            "WebSearch tool not available. Set PARALLEL_API_KEY in config or .env".to_string()
        })?;

        let call_args = serde_json::json!({
            "url": args.url
        });

        match client.call_tool("web_fetch", call_args).await {
            Ok(result) => {
                if result.is_error {
                    return Err("Search error occurred".to_string());
                }
                let mut output = String::new();
                for content in &result.content {
                    if let crate::mcp::types::Content::Text { text } = content {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                serde_json::to_string(&WebSearchOutput { results: output })
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(format!("Web search failed: {}", e)),
        }
    }
}
