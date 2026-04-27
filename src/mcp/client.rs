//! MCP Client implementation using stdio transport.
//!
//! This client spawns an MCP server as a child process and communicates
//! via JSON-RPC 2.0 over stdin/stdout.

use crate::mcp::types::*;
use anyhow::{Context, Result};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, oneshot};

type ResponseMap = Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>;

/// MCP Client that communicates with an MCP server via stdio.
/// This is wrapped in an Arc<Mutex<...>> for sharing across threads.
pub struct McpClientInner {
    stdin: Option<tokio::process::ChildStdin>,
    request_id: u64,
    pending_requests: ResponseMap,
}

impl McpClientInner {
    fn new(stdin: tokio::process::ChildStdin, pending_requests: ResponseMap) -> Self {
        Self {
            stdin: Some(stdin),
            request_id: 1,
            pending_requests,
        }
    }

    /// Send a JSON-RPC request and wait for response.
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let id = self.request_id;
        self.request_id += 1;

        let request = JsonRpcRequest::new(id, method, params);
        let request_str = serde_json::to_string(&request)?;
        
        eprintln!("[mcp] → {}", request_str);

        // Create a oneshot channel for the response
        let (tx, rx) = oneshot::channel();
        self.pending_requests.lock().await.insert(id, tx);

        // Write to stdin
        if let Some(stdin) = &mut self.stdin {
            stdin.write_all(request_str.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        } else {
            anyhow::bail!("stdin not available");
        }

        // Wait for response with timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => {
                self.pending_requests.lock().await.remove(&id);
                anyhow::bail!("Response channel closed")
            }
            Err(_) => {
                self.pending_requests.lock().await.remove(&id);
                anyhow::bail!("Request timed out")
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let notification_str = serde_json::to_string(&notification)?;

        eprintln!("[mcp] → (notification) {}", notification_str);

        if let Some(stdin) = &mut self.stdin {
            stdin.write_all(notification_str.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }

        Ok(())
    }

    /// Initialize the MCP connection.
    async fn initialize(&mut self) -> Result<InitializeResult> {
        let params = InitializeParams {
            protocolVersion: "2024-11-05".to_string(),
            capabilities: ClientCapabilities {
                roots: None,
                sampling: None,
            },
            clientInfo: ClientInfo {
                name: "my-code-agent".to_string(),
                version: "0.1.0".to_string(),
            },
        };

        let response = self
            .send_request("initialize", Some(serde_json::to_value(params)?))
            .await?;

        let result: InitializeResult = serde_json::from_value(response)?;

        // Send initialized notification
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// List all available tools from the MCP server.
    async fn list_tools(&mut self) -> Result<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = ListToolsParams { cursor: cursor.clone() };
            let response = self
                .send_request("tools/list", Some(serde_json::to_value(params)?))
                .await?;

            let result: ListToolsResult = serde_json::from_value(response)?;
            all_tools.extend(result.tools);

            match result.nextCursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => break,
            }
        }

        Ok(all_tools)
    }

    /// Call a tool on the MCP server.
    async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        let response = self
            .send_request("tools/call", Some(serde_json::to_value(params)?))
            .await?;

        let result: CallToolResult = serde_json::from_value(response)?;
        Ok(result)
    }
}

/// Public wrapper for McpClient that uses Arc<Mutex<McpClientInner>> for sharing.
pub struct McpClient {
    inner: Arc<Mutex<McpClientInner>>,
}

impl McpClient {
    /// Start an MCP server and create a client connection.
    pub async fn connect(command: &str, args: &[&str]) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {} {:?}", command, args))?;

        let stdin = child.stdin.take().context("Failed to open stdin")?;
        let stdout = child.stdout.take().context("Failed to open stdout")?;

        let pending_requests: ResponseMap = Arc::new(Mutex::new(HashMap::new()));

        // Spawn task to read responses from stdout
        let pending_clone = pending_requests.clone();
        tokio::spawn(async move {
            let reader = TokioBufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                eprintln!("[mcp] ← {}", line);

                // Parse as JSON-RPC response
                if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    let id = match &response.id {
                        serde_json::Value::Number(n) => n.as_u64(),
                        _ => None,
                    };

                    if let Some(id) = id {
                        let value = if let Some(result) = response.result {
                            result
                        } else if let Some(error) = response.error {
                            serde_json::json!({"error": {"code": error.code, "message": error.message}})
                        } else {
                            continue;
                        };

                        // Send response to waiting request
                        if let Some(tx) = pending_clone.lock().await.remove(&id) {
                            let _ = tx.send(value);
                        }
                    }
                }
            }
        });

        // Also handle stderr
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = TokioBufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[mcp:stderr] {}", line);
                }
            });
        }

        let inner = Arc::new(Mutex::new(McpClientInner::new(stdin, pending_requests)));

        // Initialize the connection
        {
            let mut client = inner.lock().await;
            client.initialize().await?;
        }

        // Keep child alive - in production you'd want to store this properly
        tokio::spawn(async move {
            let _child = child;
            // Wait for child to exit or just keep it alive
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });

        Ok(Self { inner })
    }

    /// List all available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut client = self.inner.lock().await;
        client.list_tools().await
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        let mut client = self.inner.lock().await;
        client.call_tool(name, arguments).await
    }
}
