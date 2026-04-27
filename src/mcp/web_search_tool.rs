//! WebSearch tool adapter for MCP Brave Search server.
//!
//! This tool connects to the @modelcontextprotocol/server-brave-search
//! MCP server to provide web search capabilities.

use crate::mcp::McpClient;
use rig::completion::ToolDefinition as RigToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json;
use thiserror::Error;

/// Arguments for the web_search tool.
#[derive(Deserialize, Serialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default = "default_count")]
    pub count: u32,
}

fn default_count() -> u32 {
    10
}

/// Output of the web_search tool.
#[derive(Serialize)]
pub struct WebSearchOutput {
    pub results: String,
}

/// Errors for the WebSearch tool.
#[derive(Debug, Error)]
pub enum WebSearchError {
    #[error("WebSearch tool not available. Make sure BRAVE_API_KEY is set and npx is installed.")]
    NotAvailable,
    #[error("Web search failed: {0}")]
    SearchFailed(String),
    #[error("Search error occurred")]
    SearchError,
}

/// WebSearch tool that wraps the MCP Brave Search server.
pub struct WebSearch {
    client: Option<McpClient>,
}

impl WebSearch {
    /// Create a new WebSearch tool by connecting to the Brave Search MCP server.
    pub async fn new(brave_api_key: &str) -> anyhow::Result<Self> {
        // Check if npx is available
        let npx_check = tokio::process::Command::new("npx")
            .arg("--version")
            .output()
            .await;
        
        if npx_check.is_err() || !npx_check.unwrap().status.success() {
            eprintln!("✗ npx not found. Install Node.js to use MCP web search.");
            return Ok(Self { client: None });
        }

        // Set the API key in the environment
        unsafe {
            std::env::set_var("BRAVE_API_KEY", brave_api_key);
        }

        // Connect to the Brave Search MCP server
        match McpClient::connect(
            "npx",
            &["-y", "@modelcontextprotocol/server-brave-search"],
        )
        .await
        {
            Ok(client) => {
                eprintln!("✓ MCP Brave Search connected");
                Ok(Self {
                    client: Some(client),
                })
            }
            Err(e) => {
                eprintln!("✗ Failed to connect to MCP server: {}", e);
                Ok(Self { client: None })
            }
        }
    }

    /// Check if the tool is available.
    pub fn is_available(&self) -> bool {
        self.client.is_some()
    }
}

impl Tool for WebSearch {
    const NAME: &'static str = "web_search";
    type Error = WebSearchError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self, _prompt: String) -> RigToolDefinition {
        RigToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web using Brave Search. \
                Returns relevant web results with titles, URLs, and snippets. \
                Use this tool when you need up-to-date information from the internet, \
                current events, or facts not available in the local codebase."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to submit to Brave Search"
                    },
                    "count": {
                        "type": "number",
                        "description": "Number of results to return (default: 10)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let client = self.client.as_ref().ok_or(WebSearchError::NotAvailable)?;

        eprintln!("🔍 Searching web for: {}", args.query);

        let call_args = serde_json::json!({
            "query": args.query,
            "count": args.count
        });

        match client.call_tool("brave_web_search", call_args).await {
            Ok(result) => {
                if result.isError {
                    return Err(WebSearchError::SearchError);
                }
                // Format the results
                let mut output = String::new();
                for content in &result.content {
                    if let crate::mcp::types::Content::Text { text } = content {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                Ok(WebSearchOutput { results: output })
            }
            Err(e) => {
                eprintln!("✗ Web search failed: {}", e);
                Err(WebSearchError::SearchFailed(e.to_string()))
            }
        }
    }
}
