//! WebSocket Server for controlling `my-code-agent` in headless mode.
//!
//! Implements **multi-agent routing**: each client is paired 1:1 with a dedicated
//! agent, ensuring complete session isolation. No client can see another client's
//! conversations or interfere with their sessions.
//!
//! Accepts two types of WebSocket connections:
//! - **Agent connections** (port 8088): agent instances connect here via `[ws_client]`
//! - **Client connections** (port 8089): external CLI/mobile tools connect here
//!
//! # Architecture
//!
//! Agents form a connection pool on port 8088. When a client connects on port 8089,
//! the server pairs it with an available agent. All commands from the client are
//! routed exclusively to that agent, and all responses from the agent are sent
//! exclusively back to that client.
//!
//! If no agent is available, the client waits in a pending queue. As soon as an
//! agent connects, it is immediately paired with a waiting client.
//!
//! # Usage
//!
//! ```bash
//! # Start the server
//! cargo run --example ws_server
//!
//! # With custom ports
//! cargo run --example ws_server -- --agent-port 8088 --client-port 8089
//! ```
//!
//! # REPL Commands
//!
//! | Command | Effect |
//! |---------|--------|
//! | `agents` | List connected agents and their pairing status |
//! | `ping` | Ping all unpaired agents |
//! | `prompt: <text>` | Send a prompt to the first unpaired agent |
//! | `command: <cmd>` | Execute a slash command on the first unpaired agent |
//! | `help` | Show this help |
//! | `exit` / `quit` | Shut down the server |

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::accept_async;

// ── Identifiers ──────────────────────────────────────────────────────────────

type AgentId = u64;
type ClientId = u64;

// ── Notification types ───────────────────────────────────────────────────────

/// Notification sent to an agent task about its pairing status.
enum AgentNotification {
    /// Agent has been paired with a client. Forward responses to this sender.
    Paired {
        client_resp_tx: mpsc::UnboundedSender<String>,
        client_addr: String,
        client_id: ClientId,
    },
    /// Agent has been unpaired (client disconnected).
    Unpaired,
    /// Agent should shut down.
    Shutdown,
}

/// Information sent to a pending client once an agent is available.
struct PairedWithAgent {
    /// Channel to send commands to the agent.
    cmd_tx: mpsc::UnboundedSender<String>,
    agent_id: AgentId,
}

// ── Shared state ─────────────────────────────────────────────────────────────

/// Thread-safe application state for agent-client pairing and routing.
struct AppState {
    /// All connected agents (keyed by AgentId).
    agents: HashMap<AgentId, AgentConnection>,
    /// FIFO queue of agent IDs that are ready to accept a client.
    agent_pool: VecDeque<AgentId>,
    /// Notification channels for each agent task.
    agent_notify: HashMap<AgentId, mpsc::UnboundedSender<AgentNotification>>,
    /// Clients waiting for an available agent.
    pending_clients: VecDeque<PendingClient>,
    /// Active client-to-agent pairings (for cleanup on disconnect).
    // Pairing is 1:1, but we store client info for display/cleanup.
    clients: HashMap<ClientId, ClientConnection>,
    next_agent_id: u64,
    next_client_id: u64,
    shutting_down: bool,
}

struct AgentConnection {
    /// Channel to send commands TO the agent's WebSocket.
    cmd_tx: mpsc::UnboundedSender<String>,
    addr: String,
}

struct PendingClient {
    /// Channel to forward agent responses TO this client.
    resp_tx: mpsc::UnboundedSender<String>,
    /// Oneshot to notify the client when an agent is assigned.
    pair_tx: tokio::sync::oneshot::Sender<PairedWithAgent>,
    addr: String,
    client_id: ClientId,
}

struct ClientConnection {
    addr: String,
    agent_id: AgentId,
    /// Channel to forward agent responses TO this client.
    #[allow(dead_code)]
    resp_tx: mpsc::UnboundedSender<String>,
}

// ── Display events ───────────────────────────────────────────────────────────

/// Events forwarded to the server's display loop.
enum DisplayEvent {
    AgentConnected { agent_id: AgentId, addr: String },
    AgentDisconnected { agent_id: AgentId, addr: String },
    AgentResponse { agent_id: AgentId, addr: String, json: String },
    ClientConnected { client_id: ClientId, addr: String },
    ClientDisconnected { client_id: ClientId, addr: String },
    Pairing { agent_id: AgentId, client_id: ClientId, client_addr: String },
    Unpairing { agent_id: AgentId, client_id: ClientId },
    Log { message: String },
}

// ── Parsed agent response (for pretty-printing) ──────────────────────────────

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
    TextDelta { delta: String },
    ReasoningDelta {
        #[allow(dead_code)]
        delta: String,
    },
    ToolCall { name: String, arguments: String },
    ToolResult { name: String, content: String },
}

// ── CLI args ─────────────────────────────────────────────────────────────────

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
    Args {
        agent_port,
        client_port,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = parse_args();
    let agent_addr = format!("0.0.0.0:{}", args.agent_port);
    let client_addr = format!("0.0.0.0:{}", args.client_port);

    // Shared state
    let state = Arc::new(Mutex::new(AppState {
        agents: HashMap::new(),
        agent_pool: VecDeque::new(),
        agent_notify: HashMap::new(),
        pending_clients: VecDeque::new(),
        clients: HashMap::new(),
        next_agent_id: 1,
        next_client_id: 1,
        shutting_down: false,
    }));

    // Channel for the server's own display loop
    let (display_tx, mut display_rx) = mpsc::unbounded_channel::<DisplayEvent>();

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
        "╔══════════════════════════════════════════════════════════════╗\n\
         ║   My Code Agent — WebSocket Server                          ║\n\
         ║   Multi-Agent Routing (1:1 client-agent pairing)            ║\n\
         ║                                                            ║\n\
         ║   Agent port:  ws://{agent_addr:<36} ║\n\
         ║   Client port: ws://{client_addr:<36} ║\n\
         ║                                                            ║\n\
         ║   Start agents with `[ws_client] enabled = true`            ║\n\
         ║   Connect clients with `cargo run --example ws_client`      ║\n\
         ║                                                            ║\n\
         ║   Type 'help' for server commands                          ║\n\
         ╚══════════════════════════════════════════════════════════════╝"
    );

    // ── Spawn: accept agent connections ──────────────────────────────────
    let s1 = state.clone();
    let dt1 = display_tx.clone();
    tokio::spawn(async move { accept_agents(agent_listener, s1, dt1).await });

    // ── Spawn: accept client connections ─────────────────────────────────
    let s2 = state.clone();
    let dt2 = display_tx.clone();
    tokio::spawn(async move { accept_clients(client_listener, s2, dt2).await });

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
            DisplayEvent::ClientConnected { client_id, addr } => {
                println!("  📱 Client #{client_id} connected — {addr}");
            }
            DisplayEvent::ClientDisconnected { client_id, addr } => {
                println!("  📱 Client #{client_id} disconnected — {addr}");
            }
            DisplayEvent::Pairing {
                agent_id,
                client_id,
                client_addr,
            } => {
                println!("  🔗 Agent #{agent_id} ↔ Client #{client_id} ({client_addr})");
            }
            DisplayEvent::Unpairing { agent_id, client_id } => {
                println!("  🔓 Agent #{agent_id} unpaired from Client #{client_id}");
            }
            DisplayEvent::Log { message } => {
                println!("  {message}");
            }
        }
    }
}

// ── Agent acceptor ──────────────────────────────────────────────────────────

/// Accept incoming agent connections on the agent port.
async fn accept_agents(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    display_tx: mpsc::UnboundedSender<DisplayEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let peer_addr = peer.to_string();
                let state = state.clone();
                let display_tx = display_tx.clone();

                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(ws_stream) => {
                            let (ws_sink, mut ws_stream) = ws_stream.split();
                            let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<String>();
                            let (notify_tx, mut notify_rx) =
                                mpsc::unbounded_channel::<AgentNotification>();

                            // ── Register agent and try to pair ─────────────
                            let agent_id = {
                                let mut s = state.lock().await;
                                let id = s.next_agent_id;
                                s.next_agent_id += 1;
                                s.agents.insert(
                                    id,
                                    AgentConnection {
                                        cmd_tx: cmd_tx.clone(),
                                        addr: peer_addr.clone(),
                                    },
                                );
                                s.agent_notify.insert(id, notify_tx.clone());

                                // Try to pair with a waiting client immediately
                                if let Some(pending) = s.pending_clients.pop_front() {
                                    let client_id = pending.client_id;
                                    let client_addr = pending.addr.clone();
                                    let client_resp_tx = pending.resp_tx;

                                    // Track the client
                                    s.clients.insert(
                                        client_id,
                                        ClientConnection {
                                            addr: client_addr.clone(),
                                            agent_id: id,
                                            resp_tx: client_resp_tx.clone(),
                                        },
                                    );

                                    // Notify the client task (oneshot)
                                    let _ = pending.pair_tx.send(PairedWithAgent {
                                        cmd_tx: cmd_tx.clone(),
                                        agent_id: id,
                                    });

                                    // Notify the agent task about the pairing
                                    let _ = notify_tx.send(AgentNotification::Paired {
                                        client_resp_tx,
                                        client_addr: client_addr.clone(),
                                        client_id,
                                    });

                                    let _ = display_tx.send(DisplayEvent::Pairing {
                                        agent_id: id,
                                        client_id,
                                        client_addr,
                                    });
                                } else {
                                    // No waiting clients — add to pool
                                    s.agent_pool.push_back(id);
                                }
                                id
                            };

                            let _ = display_tx.send(DisplayEvent::AgentConnected {
                                agent_id,
                                addr: peer_addr.clone(),
                            });

                            // ── Agent read + notification loop ─────────────
                            let mut paired_client: Option<ClientId> = None;
                            let mut client_resp_tx: Option<mpsc::UnboundedSender<String>> = None;

                            // Forward commands (from client) → agent WebSocket
                            let mut ws_sink = ws_sink;
                            let cmd_fwd = tokio::spawn(async move {
                                while let Some(cmd_json) = cmd_rx.recv().await {
                                    if ws_sink
                                        .send(tokio_tungstenite::tungstenite::Message::text(
                                            cmd_json,
                                        ))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            });

                            // Main select loop
                            loop {
                                tokio::select! {
                                    // Agent sent a response via WebSocket
                                    msg = ws_stream.next() => {
                                        match msg {
                                            Some(Ok(msg)) if msg.is_text() || msg.is_binary() => {
                                                let text = msg.to_text().unwrap_or("").to_string();

                                                // Forward to paired client (1:1 routing)
                                                if let Some(ref tx) = client_resp_tx {
                                                    if tx.send(text.clone()).is_err() {
                                                        // Client disconnected — unpair this agent
                                                        let cid = paired_client.take().unwrap_or(0);
                                                        let _ = display_tx.send(
                                                            DisplayEvent::Unpairing {
                                                                agent_id,
                                                                client_id: cid,
                                                            },
                                                        );
                                                        client_resp_tx = None;

                                                        // Try to re-pair with a waiting client
                                                        try_repair_agent(
                                                            &state,
                                                            agent_id,
                                                            &notify_tx,
                                                            &display_tx,
                                                            &mut paired_client,
                                                            &mut client_resp_tx,
                                                        ).await;
                                                    }
                                                }

                                                // Always display on server for monitoring
                                                let _ = display_tx.send(
                                                    DisplayEvent::AgentResponse {
                                                        agent_id,
                                                        addr: peer_addr.clone(),
                                                        json: text,
                                                    },
                                                );
                                            }
                                            Some(Ok(_)) => {} // non-text message
                                            Some(Err(e)) => {
                                                let _ = display_tx.send(DisplayEvent::Log {
                                                    message: format!("Agent #{} error: {}", agent_id, e),
                                                });
                                                break;
                                            }
                                            None => break,
                                        }
                                    }

                                    // Pairing notification from coordinator
                                    note = notify_rx.recv() => {
                                        match note {
                                            Some(AgentNotification::Paired {
                                                client_resp_tx: resp_tx,
                                                client_addr,
                                                client_id,
                                            }) => {
                                                paired_client = Some(client_id);
                                                client_resp_tx = Some(resp_tx);
                                                let _ = display_tx.send(DisplayEvent::Log {
                                                    message: format!(
                                                        "Agent #{} now serving Client #{} ({})",
                                                        agent_id, client_id, client_addr
                                                    ),
                                                });
                                            }
                                            Some(AgentNotification::Unpaired) => {
                                                let cid = paired_client.take().unwrap_or(0);
                                                client_resp_tx = None;
                                                let _ = display_tx.send(DisplayEvent::Log {
                                                    message: format!(
                                                        "Agent #{} unpaired (was serving Client #{})",
                                                        agent_id, cid
                                                    ),
                                                });

                                                // Try to re-pair immediately
                                                try_repair_agent(
                                                    &state,
                                                    agent_id,
                                                    &notify_tx,
                                                    &display_tx,
                                                    &mut paired_client,
                                                    &mut client_resp_tx,
                                                ).await;
                                            }
                                            Some(AgentNotification::Shutdown) | None => break,
                                        }
                                    }
                                }
                            }

                            cmd_fwd.abort();

                            // ── Cleanup on disconnect ──────────────────────
                            let mut s = state.lock().await;
                            s.agents.remove(&agent_id);
                            s.agent_notify.remove(&agent_id);
                            s.agent_pool.retain(|id| *id != agent_id);
                            // If this agent was paired, remove the client
                            if let Some(cid) = paired_client {
                                s.clients.remove(&cid);
                                let _ = display_tx.send(DisplayEvent::Unpairing {
                                    agent_id,
                                    client_id: cid,
                                });
                            }
                            drop(s);

                            let _ = display_tx.send(DisplayEvent::AgentDisconnected {
                                agent_id,
                                addr: peer_addr,
                            });
                        }
                        Err(e) => {
                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!(
                                    "WebSocket handshake failed from {peer_addr}: {e}"
                                ),
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

/// Accept incoming client connections on the client port.
async fn accept_clients(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    display_tx: mpsc::UnboundedSender<DisplayEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let peer_addr = peer.to_string();
                let state = state.clone();
                let display_tx = display_tx.clone();

                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(ws_stream) => {
                            let (mut ws_sink, mut ws_stream) = ws_stream.split();

                            let _ = display_tx.send(DisplayEvent::Log {
                                message: format!("📡 Client connecting — {peer_addr}"),
                            });

                            // ── Try to pair with an agent (or wait) ───────
                            let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<String>();

                            let (cmd_tx, agent_id) = match pair_client_with_agent(
                                &state, resp_tx.clone(), &peer_addr, &display_tx,
                            )
                            .await
                            {
                                Some(PairedWithAgent { cmd_tx, agent_id }) => {
                                    (cmd_tx, agent_id)
                                }
                                None => {
                                    // Server shutting down while waiting
                                    let _ = ws_sink.close().await;
                                    return;
                                }
                            };

                            // ── Normal proxying loop ────────────────────────
                            loop {
                                tokio::select! {
                                    // Client sent a command → forward to paired agent
                                    msg = ws_stream.next() => {
                                        match msg {
                                            Some(Ok(msg))
                                                if msg.is_text() || msg.is_binary() =>
                                            {
                                                let text =
                                                    msg.to_text().unwrap_or("").to_string();
                                                if cmd_tx.send(text).is_err() {
                                                    // Agent disconnected
                                                    break;
                                                }
                                            }
                                            Some(Ok(_)) => {} // non-text
                                            Some(Err(e)) => {
                                                let _ = display_tx.send(
                                                    DisplayEvent::Log {
                                                        message: format!(
                                                            "Client WS error: {e}"
                                                        ),
                                                    },
                                                );
                                                break;
                                            }
                                            None => break,
                                        }
                                    }

                                    // Agent response → forward to client
                                    response = resp_rx.recv() => {
                                        match response {
                                            Some(json) => {
                                                if ws_sink
                                                    .send(
                                                        tokio_tungstenite::tungstenite::Message::text(
                                                            json,
                                                        ),
                                                    )
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            None => break, // agent disconnected
                                        }
                                    }
                                }
                            }

                            // ── Client disconnected — cleanup ──────────────
                            cleanup_client_pairing(
                                &state, agent_id, &peer_addr, &display_tx,
                            )
                            .await;

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

// ── Pairing helpers ─────────────────────────────────────────────────────────

/// Try to pair a client with an available agent immediately, or wait in the
/// pending queue. Returns `Some(PairedWithAgent)` on success, or `None` if
/// the server is shutting down.
async fn pair_client_with_agent(
    state: &Arc<Mutex<AppState>>,
    resp_tx: mpsc::UnboundedSender<String>,
    peer_addr: &str,
    display_tx: &mpsc::UnboundedSender<DisplayEvent>,
) -> Option<PairedWithAgent> {
    let (pair_tx, pair_rx) = tokio::sync::oneshot::channel::<PairedWithAgent>();

    let client_id = {
        let mut s = state.lock().await;

        if s.shutting_down {
            return None;
        }

        let cid = s.next_client_id;
        s.next_client_id += 1;

        if let Some(agent_id) = s.agent_pool.pop_front() {
            // ── Agent available — pair immediately ─────────────────────
            let cmd_tx = s.agents.get(&agent_id).map(|a| a.cmd_tx.clone());
            let notify_tx = s.agent_notify.get(&agent_id).cloned();

            if let (Some(cmd_tx), Some(notify_tx)) = (cmd_tx, notify_tx) {
                // Track the client
                s.clients.insert(
                    cid,
                    ClientConnection {
                        addr: peer_addr.to_string(),
                        agent_id,
                        resp_tx: resp_tx.clone(),
                    },
                );

                // Notify agent about the pairing
                let _ = notify_tx.send(AgentNotification::Paired {
                    client_resp_tx: resp_tx,
                    client_addr: peer_addr.to_string(),
                    client_id: cid,
                });

                let _ = display_tx.send(DisplayEvent::ClientConnected {
                    client_id: cid,
                    addr: peer_addr.to_string(),
                });
                let _ = display_tx.send(DisplayEvent::Pairing {
                    agent_id,
                    client_id: cid,
                    client_addr: peer_addr.to_string(),
                });

                return Some(PairedWithAgent { cmd_tx, agent_id });
            }
            // Agent was in pool but vanished — fall through to pending
        }

        // ── No agent available — enqueue as pending ────────────────────
        s.pending_clients.push_back(PendingClient {
            resp_tx,
            pair_tx,
            addr: peer_addr.to_string(),
            client_id: cid,
        });

        let _ = display_tx.send(DisplayEvent::ClientConnected {
            client_id: cid,
            addr: peer_addr.to_string(),
        });

        cid
    };

    // ── Wait for an agent to become available ────────────────────────────
    match pair_rx.await {
        Ok(info) => {
            // ClientConnected was already sent when enqueued as pending
            Some(info)
        }
        Err(_) => {
            // Server shutting down or sender dropped
            // Remove from pending if still there
            let mut s = state.lock().await;
            s.pending_clients.retain(|p| p.client_id != client_id);
            None
        }
    }
}

/// Clean up a client pairing when the client disconnects. Sends `Unpaired`
/// notification to the agent so it can re-pair with another client.
async fn cleanup_client_pairing(
    state: &Arc<Mutex<AppState>>,
    agent_id: AgentId,
    peer_addr: &str,
    display_tx: &mpsc::UnboundedSender<DisplayEvent>,
) {
    let mut s = state.lock().await;

    // Find the client paired with this agent
    let client_id = s
        .clients
        .iter()
        .find(|(_, conn)| conn.agent_id == agent_id)
        .map(|(id, _)| *id);

    if let Some(cid) = client_id {
        s.clients.remove(&cid);

        // Notify the agent it's unpaired
        if let Some(notify_tx) = s.agent_notify.get(&agent_id) {
            let _ = notify_tx.send(AgentNotification::Unpaired);
        }

        let _ = display_tx.send(DisplayEvent::ClientDisconnected {
            client_id: cid,
            addr: peer_addr.to_string(),
        });
        let _ = display_tx.send(DisplayEvent::Unpairing {
            agent_id,
            client_id: cid,
        });
    }
}

/// Try to re-pair an agent with a waiting client (called after unpairing).
async fn try_repair_agent(
    state: &Arc<Mutex<AppState>>,
    agent_id: AgentId,
    notify_tx: &mpsc::UnboundedSender<AgentNotification>,
    display_tx: &mpsc::UnboundedSender<DisplayEvent>,
    paired_client: &mut Option<ClientId>,
    client_resp_tx: &mut Option<mpsc::UnboundedSender<String>>,
) {
    let mut s = state.lock().await;

    if let Some(pending) = s.pending_clients.pop_front() {
        let cid = pending.client_id;
        let client_addr = pending.addr.clone();
        let resp_tx = pending.resp_tx;

        // Send interrupt to stop any ongoing generation from previous client
        if let Some(conn) = s.agents.get(&agent_id) {
            let _ = conn.cmd_tx.send(
                r#"{"type":"interrupt","id":"server"}"#.to_string(),
            );
        }

        // Track the client
        s.clients.insert(
            cid,
            ClientConnection {
                addr: client_addr.clone(),
                agent_id,
                resp_tx: resp_tx.clone(),
            },
        );

        // Get cmd_tx for the client (create dummy if agent vanished)
        let (dummy_tx, _) = mpsc::unbounded_channel::<String>();
        let cmd_tx = s
            .agents
            .get(&agent_id)
            .map(|a| a.cmd_tx.clone())
            .unwrap_or(dummy_tx);

        // Notify the client task (oneshot)
        let _ = pending.pair_tx.send(PairedWithAgent {
            cmd_tx,
            agent_id,
        });

        // Notify ourselves (the agent task) about the new pairing
        let _ = notify_tx.send(AgentNotification::Paired {
            client_resp_tx: resp_tx,
            client_addr: client_addr.clone(),
            client_id: cid,
        });

        let _ = display_tx.send(DisplayEvent::ClientConnected {
            client_id: cid,
            addr: client_addr.clone(),
        });
        let _ = display_tx.send(DisplayEvent::Pairing {
            agent_id,
            client_id: cid,
            client_addr,
        });

        // Clear current state — the Paired notification will re-set it
        *paired_client = None;
        *client_resp_tx = None;
    } else {
        // No waiting clients — return to pool
        s.agent_pool.push_back(agent_id);
    }
}

// ── REPL ─────────────────────────────────────────────────────────────────────

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

        let mut state_lock = state.lock().await;

        if state_lock.shutting_down {
            break;
        }

        match line.as_str() {
            "exit" | "quit" => {
                println!("  👋 Shutting down...");
                state_lock.shutting_down = true;
                // Notify all agents to shut down
                for (_, notify_tx) in &state_lock.agent_notify {
                    let _ = notify_tx.send(AgentNotification::Shutdown);
                }
                break;
            }
            "help" => {
                print_repl_help();
            }
            "agents" => {
                if state_lock.agents.is_empty() {
                    println!("  📭 No agents connected.");
                } else {
                    for (id, conn) in &state_lock.agents {
                        let is_paired = state_lock
                            .clients
                            .values()
                            .any(|c| c.agent_id == *id);
                        let is_in_pool = state_lock.agent_pool.contains(id);
                        let status = if is_paired {
                            "paired"
                        } else if is_in_pool {
                            "available"
                        } else {
                            "busy"
                        };
                        println!("  🤖 Agent #{id} — {addr} [{status}]", addr = conn.addr);
                    }
                    // Show pending clients
                    if !state_lock.pending_clients.is_empty() {
                        println!(
                            "  ⏳ {} client(s) waiting for an agent.",
                            state_lock.pending_clients.len()
                        );
                    }
                }
            }
            "clients" => {
                if state_lock.clients.is_empty() {
                    println!("  📭 No clients connected.");
                } else {
                    for (id, conn) in &state_lock.clients {
                        println!("  📱 Client #{id} — {addr} → Agent #{agent_id}", addr = conn.addr, agent_id = conn.agent_id);
                    }
                }
            }
            "ping" => {
                // Ping only unpaired agents
                let mut count = 0u64;
                for (id, conn) in &state_lock.agents {
                    let is_paired = state_lock.clients.values().any(|c| c.agent_id == *id);
                    if !is_paired {
                        let _ = conn.cmd_tx.send(r#"{"type":"ping","id":"cli"}"#.to_string());
                        count += 1;
                    }
                }
                println!("  🏓 Ping sent to {count} unpaired agent(s).");
            }
            "interrupt" => {
                // Interrupt all agents
                let mut count = 0u64;
                for (_id, conn) in &state_lock.agents {
                    let _ = conn
                        .cmd_tx
                        .send(r#"{"type":"interrupt","id":"cli"}"#.to_string());
                    count += 1;
                }
                println!("  ⏹️  Interrupt sent to {count} agent(s).");
            }
            "history" => {
                // Request history from all agents
                let mut count = 0u64;
                for (_id, conn) in &state_lock.agents {
                    let _ = conn
                        .cmd_tx
                        .send(r#"{"type":"get_history","id":"cli"}"#.to_string());
                    count += 1;
                }
                println!("  📜 History requested from {count} agent(s).");
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

                // Send to first unpaired agent
                let target = state_lock.agents.iter().find(|(id, _)| {
                    !state_lock.clients.values().any(|c| c.agent_id == **id)
                });

                if let Some((id, _conn)) = target {
                    let cmd = serde_json::json!({
                        "type": "prompt",
                        "text": text,
                        "id": format!("cli-{}", std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos())
                    });
                    if let Some(conn) = state_lock.agents.get(id) {
                        conn.cmd_tx.send(cmd.to_string()).ok();
                        println!("  📤 Prompt sent to Agent #{id}: {text:.80}");
                    }
                } else {
                    println!("  ⚠️  No unpaired agents available. All agents are serving clients.");
                }
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

                // Send to first unpaired agent
                let target = state_lock.agents.iter().find(|(id, _)| {
                    !state_lock.clients.values().any(|c| c.agent_id == **id)
                });

                if let Some((id, _conn)) = target {
                    let cmd = serde_json::json!({
                        "type": "command",
                        "cmd": cmd_text,
                        "id": "cli",
                    });
                    if let Some(conn) = state_lock.agents.get(id) {
                        conn.cmd_tx.send(cmd.to_string()).ok();
                        println!("  📤 Command sent to Agent #{id}: {cmd_text}");
                    }
                } else {
                    println!("  ⚠️  No unpaired agents available. All agents are serving clients.");
                }
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

fn print_repl_help() {
    println!(
        r#"  ── Available Commands ──────────────────────────────
  prompt: <text>     Send a prompt to the first unpaired agent
  command: <cmd>     Execute a slash command on an unpaired agent
  history            Fetch history from all agents
  interrupt          Interrupt all agents
  ping               Ping all unpaired agents
  agents             List connected agents with pairing status
  clients            List connected clients
  help               Show this help
  exit / quit        Shut down the server
  ───────────────────────────────────────────────────"#
    );
}

// ── Response display ────────────────────────────────────────────────────────

fn print_response(agent_id: AgentId, addr: &str, response: AgentResponse) {
    match response {
        AgentResponse::Result {
            ok,
            summary,
            full_response,
            error,
            ..
        } => {
            let icon = if ok { "✅" } else { "❌" };
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
            println!(
                "  📜 [agent #{agent_id} @ {addr}] History ({} messages):",
                messages.len()
            );
            for (i, msg) in messages.iter().enumerate() {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                let display = safe_truncate(content, 117);
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
            print!("{delta}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        AgentResponse::ReasoningDelta { delta: _ } => {}
        AgentResponse::ToolCall { name, arguments } => {
            let args_preview = safe_truncate(&arguments, 77);
            println!("  🛠  [agent #{agent_id} @ {addr}] Tool: {name}({args_preview})");
        }
        AgentResponse::ToolResult { name, content } => {
            let preview = content.lines().next().unwrap_or("").to_string();
            let preview = if preview.len() > 120 {
                let end = (0..=117.min(preview.len()))
                    .rev()
                    .find(|&i| preview.is_char_boundary(i))
                    .unwrap_or(0);
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

/// Truncate a string to at most `max` bytes, ensuring we never split a
/// multi-byte UTF-8 character (e.g. Chinese, emoji).
fn safe_truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = (0..=max.min(s.len()))
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}…", &s[..end])
}
