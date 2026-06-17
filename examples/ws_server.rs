//! WebSocket Server for controlling `my-code-agent` in headless mode.
//!
//! Accepts two types of WebSocket connections:
//! - **Agent connections** (port 8088): the agent connects here via `[ws_client]`
//! - **Client connections** (port 8089): external CLI tools connect here to send
//!   commands to the agent remotely
//!
//! Commands from the REPL or connected clients are forwarded to all connected
//! agents. Agent responses are broadcast back to all clients and displayed in
//! the server's terminal.
//!
//! # Usage
//!
//! ```bash
//! # Start the server (default agent port 8088, client port 8089)
//! cargo run --example ws_server
//!
//! # With custom ports
//! cargo run --example ws_server -- --agent-port 8088 --client-port 8089
//! ```

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio_tungstenite::accept_async;

// ── Types ────────────────────────────────────────────────────────────────────

type AgentId = u64;

/// Shared application state.
struct AppState {
    agents: HashMap<AgentId, AgentConnection>,
    next_id: u64,
    shutting_down: bool,
}

struct AgentConnection {
    tx: mpsc::UnboundedSender<String>,
    addr: String,
}

/// Events from agent connections forwarded to the display loop.
enum DisplayEvent {
    AgentConnected { agent_id: AgentId, addr: String },
    AgentDisconnected { agent_id: AgentId, addr: String },
    AgentResponse { agent_id: AgentId, addr: String, json: String },
    Log { message: String },
}

/// Parsed agent response (for pretty-printing on the server).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentResponse {
    Result {
        ok: bool,
        summary: String,
        full_response: Option<String>,
        error: Option<String>,
        #[allow(dead_code)]
        id: Option<String>,
    },
    History {
        messages: Vec<serde_json::Value>,
        #[allow(dead_code)]
        id: Option<String>,
    },
    Status {
        streaming: bool,
        message: Option<String>,
    },
    Pong {
        #[allow(dead_code)]
        id: Option<String>,
    },
    Error {
        message: String,
        #[allow(dead_code)]
        id: Option<String>,
    },
    /// Streaming text delta from the agent.
    TextDelta {
        delta: String,
    },
    /// Streaming reasoning delta.
    ReasoningDelta {
        #[allow(dead_code)]
        delta: String,
    },
    /// Tool call started.
    ToolCall {
        name: String,
        arguments: String,
    },
    /// Tool result received.
    ToolResult {
        name: String,
        content: String,
    },
}

// ── Args ─────────────────────────────────────────────────────────────────────

struct Args {
    agent_port: u16,
    client_port: u16,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut agent_port = 8088u16;
    let mut client_port = 8089u16;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--agent-port" => {
                if let Some(p) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    agent_port = p;
                    i += 2;
                    continue;
                }
            }
            "--client-port" => {
                if let Some(p) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    client_port = p;
                    i += 2;
                    continue;
                }
            }
            "--help" | "-h" => {
                println!("Usage: ws-server [--agent-port <PORT>] [--client-port <PORT>]");
                println!("  Default agent port:  8088");
                println!("  Default client port: 8089");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    Args { agent_port, client_port }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = parse_args();
    let agent_addr = format!("0.0.0.0:{}", args.agent_port);
    let client_addr = format!("0.0.0.0:{}", args.client_port);

    // Shared state (agents + shutdown flag)
    let state = Arc::new(Mutex::new(AppState {
        agents: HashMap::new(),
        next_id: 1,
        shutting_down: false,
    }));

    // Channel for the server's own display loop
    let (display_tx, mut display_rx) = mpsc::unbounded_channel::<DisplayEvent>();

    // Broadcast channel: agent responses → relay to all connected clients
    let (resp_broadcast_tx, _) = broadcast::channel::<String>(256);

    // ── Bind listeners ────────────────────────────────────────────────────
    let agent_listener = TcpListener::bind(&agent_addr).await.unwrap_or_else(|e| {
        eprintln!("❌ Failed to bind agent port {agent_addr}: {e}");
        std::process::exit(1);
    });
    let client_listener = TcpListener::bind(&client_addr).await.unwrap_or_else(|e| {
        eprintln!("❌ Failed to bind client port {client_addr}: {e}");
        std::process::exit(1);
    });

    println!(
        "╔══════════════════════════════════════════════════════════╗\n\
         ║   My Code Agent — WebSocket Server                      ║\n\
         ║                                                        ║\n\
         ║   Agent port:  ws://{agent_addr:<28} ║\n\
         ║   Client port: ws://{client_addr:<28} ║\n\
         ║                                                        ║\n\
         ║   Start your agent with `[ws_client] enabled = true`    ║\n\
         ║   Connect clients with `cargo run --example ws_client`  ║\n\
         ║                                                        ║\n\
         ║   Type 'help' for server commands                       ║\n\
         ╚══════════════════════════════════════════════════════════╝"
    );

    // ── Spawn: accept agent connections ──────────────────────────────────
    let s1 = state.clone();
    let dt1 = display_tx.clone();
    let bt1 = resp_broadcast_tx.clone();
    tokio::spawn(async move { accept_agents(agent_listener, s1, dt1, bt1).await });

    // ── Spawn: accept client connections ─────────────────────────────────
    let s2 = state.clone();
    let dt2 = display_tx.clone();
    let bt2 = resp_broadcast_tx.clone();
    tokio::spawn(async move { accept_clients(client_listener, s2, dt2, bt2).await });

    // ── Spawn: stdin REPL ────────────────────────────────────────────────
    let s3 = state.clone();
    tokio::spawn(async move { repl_loop(s3, display_tx).await });

    // ── Display loop ─────────────────────────────────────────────────────
    while let Some(event) = display_rx.recv().await {
        match event {
            DisplayEvent::AgentResponse { agent_id, addr, json } => {
                if let Ok(response) = serde_json::from_str::<AgentResponse>(&json) {
                    print_response(agent_id, &addr, response);
                } else {
                    println!("  [agent #{agent_id} @ {addr}] (raw) {json}");
                }
            }
            DisplayEvent::AgentConnected { agent_id, addr } => {
                println!("  ✅ Agent #{agent_id} connected — {addr}");
            }
            DisplayEvent::AgentDisconnected { agent_id, addr } => {
                println!("  ❌ Agent #{agent_id} disconnected — {addr}");
            }
            DisplayEvent::Log { message } => {
                println!("  {message}");
            }
        }
    }
}

// ── Agent acceptor ──────────────────────────────────────────────────────────

async fn accept_agents(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    display_tx: mpsc::UnboundedSender<DisplayEvent>,
    resp_broadcast_tx: broadcast::Sender<String>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let peer_addr = peer.to_string();
                let state = state.clone();
                let display_tx = display_tx.clone();
                let resp_broadcast_tx = resp_broadcast_tx.clone();

                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(ws_stream) => {
                            let (ws_sink, mut ws_stream) = ws_stream.split();
                            let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<String>();

                            let agent_id = {
                                let mut s = state.lock().await;
                                let id = s.next_id;
                                s.next_id += 1;
                                s.agents.insert(id, AgentConnection {
                                    tx: cmd_tx,
                                    addr: peer_addr.clone(),
                                });
                                id
                            };

                            let _ = display_tx.send(DisplayEvent::AgentConnected {
                                agent_id,
                                addr: peer_addr.clone(),
                            });

                            // Forward commands (from REPL / clients) → agent
                            let mut ws_sink = ws_sink;
                            let cmd_fwd = tokio::spawn(async move {
                                while let Some(cmd_json) = cmd_rx.recv().await {
                                    if ws_sink
                                        .send(tokio_tungstenite::tungstenite::Message::text(cmd_json))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            });

                            // Forward agent responses → display + broadcast
                            loop {
                                tokio::select! {
                                    msg = ws_stream.next() => {
                                        match msg {
                                            Some(Ok(msg)) => {
                                                if msg.is_text() || msg.is_binary() {
                                                    let text = msg.to_text().unwrap_or("").to_string();
                                                    let _ = display_tx.send(
                                                        DisplayEvent::AgentResponse {
                                                            agent_id,
                                                            addr: peer_addr.clone(),
                                                            json: text.clone(),
                                                        },
                                                    );
                                                    // Broadcast to all connected clients
                                                    let _ = resp_broadcast_tx.send(text);
                                                }
                                            }
                                            Some(Err(e)) => {
                                                let _ = display_tx.send(DisplayEvent::Log {
                                                    message: format!("Agent #{} error: {}", agent_id, e),
                                                });
                                                break;
                                            }
                                            None => break,
                                        }
                                    }
                                }
                            }

                            cmd_fwd.abort();

                            let mut s = state.lock().await;
                            s.agents.remove(&agent_id);
                            drop(s);

                            let _ = display_tx.send(DisplayEvent::AgentDisconnected {
                                agent_id,
                                addr: peer_addr,
                            });
                        }
                        Err(e) => {
                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!("WebSocket handshake failed from {peer_addr}: {e}"),
                            });
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {e}");
                break;
            }
        }
    }
}

// ── Client acceptor ─────────────────────────────────────────────────────────

async fn accept_clients(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    display_tx: mpsc::UnboundedSender<DisplayEvent>,
    resp_broadcast_tx: broadcast::Sender<String>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let peer_addr = peer.to_string();
                let state = state.clone();
                let display_tx = display_tx.clone();
                let resp_broadcast_tx = resp_broadcast_tx.clone();

                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(ws_stream) => {
                            let (mut ws_sink, mut ws_stream) = ws_stream.split();

                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!("📡 Client connected — {peer_addr}"),
                            });

                            // Subscribe to agent response broadcasts
                            let mut resp_rx = resp_broadcast_tx.subscribe();

                            // Concurrently:
                            // 1. Forward client commands → agents
                            // 2. Forward agent responses → client
                            // 3. Handle broadcast lag (skip stale messages)
                            loop {
                                tokio::select! {
                                    // Client sent a command → forward to all agents
                                    msg = ws_stream.next() => {
                                        match msg {
                                            Some(Ok(msg)) => {
                                                if msg.is_text() || msg.is_binary() {
                                                    let text = msg.to_text().unwrap_or("").to_string();
                                                    let mut s = state.lock().await;
                                                    send_to_all_agents(&mut s, &text);
                                                }
                                            }
                                            Some(Err(e)) => {
                                                let _ = display_tx.send(DisplayEvent::Log {
                                                    message: format!("Client {peer_addr} error: {e}"),
                                                });
                                                break;
                                            }
                                            None => break,
                                        }
                                    }

                                    // Agent response → forward to client
                                    result = resp_rx.recv() => {
                                        match result {
                                            Ok(json) => {
                                                if ws_sink
                                                    .send(tokio_tungstenite::tungstenite::Message::text(json))
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                                // Client was too slow; skip stale messages
                                                tracing::debug!("Client {peer_addr} lagged by {n} messages");
                                            }
                                            Err(broadcast::error::RecvError::Closed) => break,
                                        }
                                    }
                                }
                            }

                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!("📡 Client disconnected — {peer_addr}"),
                            });
                        }
                        Err(e) => {
                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!("Client WS handshake failed from {peer_addr}: {e}"),
                            });
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("Client accept error: {e}");
                break;
            }
        }
    }
}

// ── REPL (unchanged) ─────────────────────────────────────────────────────────

async fn repl_loop(state: Arc<Mutex<AppState>>, display_tx: mpsc::UnboundedSender<DisplayEvent>) {
    use tokio::io::AsyncBufReadExt;

    let stdin = tokio::io::stdin();
    let reader = tokio::io::BufReader::new(stdin);
    let mut lines = reader.lines();

    loop {
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

        let mut state = state.lock().await;

        if state.shutting_down {
            break;
        }

        match line.as_str() {
            "exit" | "quit" => {
                println!("  👋 Shutting down...");
                state.shutting_down = true;
                break;
            }
            "help" => {
                print_help();
            }
            "agents" => {
                if state.agents.is_empty() {
                    println!("  📭 No agents connected.");
                } else {
                    for (id, conn) in &state.agents {
                        println!("  🤖 Agent #{id} — {addr}", addr = conn.addr);
                    }
                }
            }
            "ping" => {
                send_to_all_agents(&mut state, r#"{"type":"ping"}"#);
            }
            "interrupt" => {
                send_to_all_agents(&mut state, r#"{"type":"interrupt","id":"cli"}"#);
                println!("  ⏹️  Interrupt sent to {} agent(s).", state.agents.len());
            }
            "history" => {
                send_to_all_agents(&mut state, r#"{"type":"get_history","id":"cli"}"#);
                println!("  📜 History requested from {} agent(s).", state.agents.len());
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

                let cmd = serde_json::json!({
                    "type": "prompt",
                    "text": text,
                    "id": format!("cli-{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos())
                });

                send_to_all_agents(&mut state, &cmd.to_string());
                println!("  📤 Prompt sent to {} agent(s): {text:.80}", state.agents.len());
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

                let cmd = serde_json::json!({
                    "type": "command",
                    "cmd": cmd_text,
                    "id": "cli",
                });

                send_to_all_agents(&mut state, &cmd.to_string());
                println!("  📤 Command sent to {} agent(s): {cmd_text}", state.agents.len());
            }
            _ => {
                println!("  ❓ Unknown command: {line}. Type 'help' for available commands.");
            }
        }
    }

    let _ = display_tx.send(DisplayEvent::Log {
        message: "REPL shutting down.".to_string(),
    });
}

fn send_to_all_agents(state: &mut AppState, json: &str) {
    if state.agents.is_empty() {
        return;
    }
    state.agents.retain(|_id, conn| conn.tx.send(json.to_string()).is_ok());
}

// ── Response display ────────────────────────────────────────────────────────

fn print_response(agent_id: AgentId, addr: &str, response: AgentResponse) {
    match response {
        AgentResponse::Result { ok, summary, full_response, error, .. } => {
            let icon = if ok { "✅" } else { "❌" };
            // Print the full response body; fall back to summary if empty
            if let Some(ref body) = full_response {
                if !body.is_empty() {
                    for line in body.lines() {
                        println!("  [agent #{agent_id} @ {addr}] {line}");
                    }
                } else {
                    println!("  {icon} [agent #{agent_id} @ {addr}] {summary}");
                }
            } else {
                println!("  {icon} [agent #{agent_id} @ {addr}] {summary}");
            }
            if let Some(err) = error {
                println!("  {icon} [agent #{agent_id} @ {addr}] Error: {err}");
            }
        }
        AgentResponse::History { messages, .. } => {
            println!("  📜 [agent #{agent_id} @ {addr}] History ({} messages):", messages.len());
            for (i, msg) in messages.iter().enumerate() {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                let display = if content.len() > 120 {
                    format!("{}…", &content[..117])
                } else {
                    content.to_string()
                };
                println!("     [{i}] {role}: {display}");
            }
        }
        AgentResponse::Status { streaming, message } => {
            let icon = if streaming { "⏳" } else { "✓" };
            let status = if streaming { "streaming" } else { "idle" };
            println!("  {icon} [agent #{agent_id} @ {addr}] Status: {status}");
            if let Some(msg) = message {
                println!("     {msg}");
            }
        }
        AgentResponse::TextDelta { delta } => {
            // Print inline without newline for smooth streaming display
            print!("{delta}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        AgentResponse::ReasoningDelta { delta: _ } => {
            // Reasoning deltas are typically not displayed on the server,
            // but forwarded to clients via the broadcast channel
        }
        AgentResponse::ToolCall { name, arguments } => {
            let args_preview = if arguments.len() > 80 {
                format!("{}…", &arguments[..77])
            } else {
                arguments.clone()
            };
            println!("  🛠  [agent #{agent_id} @ {addr}] Tool: {name}({args_preview})");
        }
        AgentResponse::ToolResult { name, content } => {
            let preview = content.lines().next().unwrap_or("").to_string();
            let preview = if preview.len() > 120 {
                let mut end = 117.min(preview.len());
                while end > 0 && !preview.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &preview[..end])
            } else {
                preview
            };
            println!("  ✅ [agent #{agent_id} @ {addr}] Result: {name} → {preview}");
        }
        AgentResponse::Pong { .. } => {
            println!("  🏓 [agent #{agent_id} @ {addr}] pong");
        }
        AgentResponse::Error { message, .. } => {
            println!("  ⚠️  [agent #{agent_id} @ {addr}] Error: {message}");
        }
    }
}

fn print_help() {
    println!(
        r#"  ── Available Commands ──────────────────────────────
  prompt: <text>     Send a prompt to the connected agent(s)
  command: <cmd>     Execute a slash command (/status, /tokens, /clear)
  history            Fetch the full conversation history
  interrupt          Interrupt the current streaming response
  ping               Ping all connected agents
  agents             List connected agents
  help               Show this help
  exit / quit        Shut down the server
  ───────────────────────────────────────────────────"#
    );
}
