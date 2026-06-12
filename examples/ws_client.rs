//! WebSocket Client Tool — remote control for `my-code-agent`.
//!
//! Connects to the `ws-server`'s client port and provides an interactive REPL
//! for sending commands to the agent. All commands are forwarded through the
//! server to the connected agent, and responses are displayed in real-time.
//!
//! # Usage
//!
//! ```bash
//! # Connect to default server (ws://localhost:8089)
//! cargo run --example ws_client
//!
//! # Connect to custom address
//! cargo run --example ws_client -- --server ws://192.168.1.100:8089
//! ```
//!
//! # Commands
//!
//! | Input | Effect |
//! |---|---|
//! | `prompt: <text>` | Send a prompt to the agent |
//! | `command: <cmd>` | Execute a slash command (`/status`, `/tokens`) |
//! | `history` | Fetch the conversation history |
//! | `interrupt` | Interrupt the current streaming response |
//! | `ping` | Ping the agent |
//! | `help` | Show available commands |
//! | `exit` / `quit` / Ctrl+C | Disconnect and exit |

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

#[tokio::main]
async fn main() {
    let server_url = parse_args();
    println!("🔌 Connecting to {server_url}...");

    let ws_stream = match connect_async(&server_url).await {
        Ok((stream, _)) => stream,
        Err(e) => {
            eprintln!("❌ Failed to connect to {server_url}: {e}");
            std::process::exit(1);
        }
    };

    println!("✅ Connected. Type 'help' for commands.\n");

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // ── Spawn: forward stdin → server ────────────────────────────────────
    let stdin_task = tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;

        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);
        let mut lines = reader.lines();

        loop {
            // Print prompt without newline
            print!("> ");
            use std::io::Write;
            let _ = std::io::stdout().flush();

            let line = match lines.next_line().await {
                Ok(Some(line)) => line.trim().to_string(),
                Ok(None) => break,
                Err(_) => break,
            };

            if line.is_empty() {
                continue;
            }

            match line.as_str() {
                "exit" | "quit" => {
                    println!("  👋 Disconnecting...");
                    break;
                }
                "help" => {
                    print_help();
                    continue;
                }
                "ping" => {
                    let msg = serde_json::json!({"type": "ping", "id": "client"});
                    if ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
                "interrupt" => {
                    let msg = serde_json::json!({"type": "interrupt", "id": "client"});
                    if ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    println!("  ⏹️  Interrupt sent.");
                    continue;
                }
                "history" => {
                    let msg = serde_json::json!({"type": "get_history", "id": "client"});
                    if ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    println!("  📜 Requesting history...");
                    continue;
                }
                _ if line.starts_with("prompt:") || line.starts_with("prompt ") => {
                    let text = line
                        .strip_prefix("prompt:")
                        .or_else(|| line.strip_prefix("prompt "))
                        .unwrap_or("")
                        .trim();

                    if text.is_empty() {
                        println!("  ⚠️  Usage: prompt: <your message>");
                        continue;
                    }

                    let msg = serde_json::json!({
                        "type": "prompt",
                        "text": text,
                        "id": format!("client-{}", std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos())
                    });

                    if ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    println!("  📤 Prompt sent: {text:.80}");
                    continue;
                }
                _ if line.starts_with("command:") || line.starts_with("command ") => {
                    let cmd_text = line
                        .strip_prefix("command:")
                        .or_else(|| line.strip_prefix("command "))
                        .unwrap_or("")
                        .trim();

                    if cmd_text.is_empty() {
                        println!("  ⚠️  Usage: command: /status");
                        continue;
                    }

                    let msg = serde_json::json!({
                        "type": "command",
                        "cmd": cmd_text,
                        "id": "client",
                    });

                    if ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::text(msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    println!("  📤 Command sent: {cmd_text}");
                    continue;
                }
                _ => {
                    println!("  ❓ Unknown: {line}. Type 'help'.");
                    continue;
                }
            }
        }

        // Close the WebSocket gracefully
        let _ = ws_sink.close().await;
    });

    // ── Display: server responses → stdout ──────────────────────────────
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(msg) if msg.is_text() || msg.is_binary() => {
                let text = msg.to_text().unwrap_or("").to_string();
                // Pretty-print based on type
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let type_str = json["type"].as_str().unwrap_or("?");
                    match type_str {
                        "result" => {
                            let ok = json["ok"].as_bool().unwrap_or(false);
                            let icon = if ok { "✅" } else { "❌" };
                            // Display the full response body, not just the truncated summary.
                            // The `full_response` contains the agent's complete text output.
                            if let Some(body) = json["full_response"].as_str() {
                                if !body.is_empty() {
                                    // Print each line indented for readability
                                    for line in body.lines() {
                                        println!("  {line}");
                                    }
                                } else {
                                    // Fallback: use summary if full_response is empty
                                    let summary = json["summary"].as_str().unwrap_or("");
                                    println!("  {icon} {summary}");
                                }
                            } else {
                                let summary = json["summary"].as_str().unwrap_or("");
                                println!("  {icon} {summary}");
                            }
                            // Newline after streaming ends
                            println!();
                            if let Some(err) = json["error"].as_str() {
                                if !err.is_empty() {
                                    println!("     Error: {err}");
                                }
                            }
                        }
                        "history" => {
                            let msgs = json["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                            println!("  📜 History ({msgs} messages)");
                            if let Some(messages) = json["messages"].as_array() {
                                for (i, m) in messages.iter().enumerate() {
                                    let role = m["role"].as_str().unwrap_or("?");
                                    let content = m["content"].as_str().unwrap_or("");
                                    let display = if content.len() > 200 {
                                        format!("{}…", &content[..197])
                                    } else {
                                        content.to_string()
                                    };
                                    // Indent to align with "History" line
                                    println!("     [{i}] {role}: {display}");
                                }
                            }
                        }
                        "status" => {
                            let streaming = json["streaming"].as_bool().unwrap_or(false);
                            let icon = if streaming { "⏳" } else { "✓" };
                            let status = if streaming { "streaming" } else { "idle" };
                            if let Some(msg) = json["message"].as_str() {
                                if !msg.is_empty() {
                                    println!("  {icon} {msg}");
                                } else {
                                    println!("  {icon} Agent status: {status}");
                                }
                            } else {
                                println!("  {icon} Agent status: {status}");
                            }
                        }
                        "text_delta" => {
                            // Streaming text — print inline without newline
                            if let Some(delta) = json["delta"].as_str() {
                                use std::io::Write;
                                print!("{delta}");
                                let _ = std::io::stdout().flush();
                            }
                        }
                        "reasoning_delta" => {
                            // Reasoning — shown inline in dimmed style via markers
                            if let Some(delta) = json["delta"].as_str() {
                                // Only show first occurrence as a header to avoid noise
                                if !delta.is_empty() {
                                    // Print reasoning inline without newline
                                    use std::io::Write;
                                    print!("{delta}");
                                    let _ = std::io::stdout().flush();
                                }
                            }
                        }
                        "tool_call" => {
                            let name = json["name"].as_str().unwrap_or("?");
                            let args = json["arguments"].as_str().unwrap_or("");
                            let preview = if args.len() > 60 {
                                format!("{}…", &args[..57])
                            } else {
                                args.to_string()
                            };
                            println!("\n  🛠  Tool: {name}({preview})");
                        }
                        "tool_result" => {
                            let name = json["name"].as_str().unwrap_or("?");
                            let content = json["content"].as_str().unwrap_or("");
                            let preview = content.lines().next().unwrap_or("").to_string();
                            let preview = if preview.len() > 100 {
                                format!("{}…", &preview[..97])
                            } else {
                                preview
                            };
                            println!("  ✅ Tool result: {name} → {preview}");
                        }
                        "pong" => {
                            println!("  🏓 pong");
                        }
                        "error" => {
                            let msg = json["message"].as_str().unwrap_or("unknown error");
                            println!("  ⚠️  Server error: {msg}");
                        }
                        _ => {
                            // Fallback: pretty-print the JSON
                            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                                println!("{pretty}");
                            } else {
                                println!("{text}");
                            }
                        }
                    }
                } else {
                    println!("{text}");
                }
            }
            Ok(_) => {} // non-text messages
            Err(e) => {
                eprintln!("\n❌ Connection error: {e}");
                break;
            }
        }
    }

    stdin_task.abort();
    println!("\n🔌 Disconnected.");
}

fn parse_args() -> String {
    let args: Vec<String> = std::env::args().collect();
    let mut server_url = "ws://localhost:8089".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--server" | "-s" => {
                if let Some(url) = args.get(i + 1) {
                    server_url = url.clone();
                    i += 2;
                    continue;
                }
            }
            "--help" | "-h" => {
                println!("Usage: ws-client [--server <URL>]");
                println!("  Default server: ws://localhost:8089");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    server_url
}

fn print_help() {
    println!(
        r#"  ── Commands ──────────────────────────────────
  prompt: <text>     Send a prompt to the agent
  command: <cmd>     Execute a command (/status, /tokens)
  history            Fetch conversation history
  interrupt          Interrupt current response
  ping               Ping the agent
  help               Show this help
  exit / quit        Disconnect
  ─────────────────────────────────────────────"#
    );
}
