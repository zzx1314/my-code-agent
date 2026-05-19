pub mod knowledge;

use std::sync::Arc;

use anyhow::Result;

use crate::core::config::Config;
use crate::core::context::context_manager::ContextManager;
use crate::core::agent::preamble::{Agent, build_client, build_preamble};
use crate::core::session::SessionData;
use crate::core::context::token_usage::TokenUsage;
use crate::tools::exec::confirmation::{ConfirmationHandle, ConfirmationRequest};
use crate::tools::create_mcp_tools;

pub struct InitState {
    pub config: Config,
    pub chat_history: Vec<crate::app::ChatEntry>,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub agent: Arc<Agent>,
    pub orchestrator: Arc<crate::core::agent::orchestrator::AgentOrchestrator>,
    pub confirmation_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ConfirmationRequest>>,
    pub interrupt_tx: tokio::sync::broadcast::Sender<()>,
    pub context_manager: ContextManager,
}

pub async fn init_app() -> Result<InitState> {
    // ── 1. Environment ──────────────────────────────────────────────────────
    let env_path = crate::core::paths::app_file(".env");
    if env_path.exists() {
        dotenv::from_path(&env_path).ok();
    } else {
        dotenv::dotenv().ok();
    }

    // ── 2. Logging ──────────────────────────────────────────────────────────
    // Split logs into 3 files:
    //   - app.log   : info+  — general application flow
    //   - error.log : error  — errors only for quick debugging
    //   - tools.log : all    — tool execution traces (target = tools::*)
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::Layer;

    let logs_dir = crate::core::paths::app_file("logs");
    if let Err(e) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("[logging] Failed to create log directory {logs_dir:?}: {e}");
    }

    const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

    // Manual rotation: shift backups (.log.3 → deleted, .log.2 → .log.3, .log.1 → .log.2, .log → .log.1)
    const MAX_BACKUPS: u32 = 3;
    fn rotate_if_large(path: &std::path::Path, max_bytes: u64) {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > max_bytes {
                // Shift existing backups: .log.N → .log.(N+1), drop the oldest
                for i in (1..MAX_BACKUPS).rev() {
                    let current = path.with_extension(format!("log.{i}"));
                    let next = path.with_extension(format!("log.{}", i + 1));
                    if current.exists() {
                        let _ = std::fs::rename(&current, &next);
                    }
                }
                // Rotate current → .log.1 (unconditional — rename() is safe if file is missing)
                let backup = path.with_extension("log.1");
                let _ = std::fs::rename(path, backup);
            }
        }
    }

    rotate_if_large(&logs_dir.join("app.log"), MAX_LOG_SIZE);
    rotate_if_large(&logs_dir.join("error.log"), MAX_LOG_SIZE);
    rotate_if_large(&logs_dir.join("tools.log"), MAX_LOG_SIZE);

    // Helper to open a log file with fallback to /tmp
    fn open_log(logs_dir: &std::path::Path, name: &str) -> std::fs::File {
        let primary_path = logs_dir.join(name);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&primary_path)
            .unwrap_or_else(|e| {
                eprintln!("[logging] Failed to open {primary_path:?}: {e} — falling back to /tmp");
                let fallback_dir = std::path::Path::new("/tmp/my-code-agent-logs");
                if let Err(dir_err) = std::fs::create_dir_all(fallback_dir) {
                    eprintln!("[logging] Failed to create fallback directory {fallback_dir:?}: {dir_err}");
                }
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(fallback_dir.join(name))
                    .unwrap()
            })
    }
    // Layer 1: app.log — info and above (main application log)
    let app_layer = tracing_subscriber::fmt::layer()
        .with_writer(open_log(&logs_dir, "app.log"))
        .with_ansi(false)
        .with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        );

    // Layer 2: error.log — errors only
    let error_layer = tracing_subscriber::fmt::layer()
        .with_writer(open_log(&logs_dir, "error.log"))
        .with_ansi(false)
        .with_filter(tracing_subscriber::EnvFilter::new("error"));

    // Layer 3: tools.log — all levels for tools::* modules
    // Use the crate's full module path for precise filtering
    let tools_layer = tracing_subscriber::fmt::layer()
        .with_writer(open_log(&logs_dir, "tools.log"))
        .with_ansi(false)
        .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
            let target = meta.target();
            target.starts_with("my_code_agent::tools") || target.contains("::tools::")
        }));

    tracing_subscriber::registry()
        .with(app_layer)
        .with(error_layer)
        .with(tools_layer)
        .init();

    // ── 3. Config ───────────────────────────────────────────────────────────
    let config = Config::load();

    // ── 4. Session ID for undo tracking ─────────────────────────────────────
    let session_id = format!(
        "session_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    crate::tools::infra::undo_history::set_session_id(session_id.clone());
    tracing::info!(session_id = %session_id, "Initialized session ID for undo tracking");

    // ── 5. Restore session ──────────────────────────────────────────────────
    let mut chat_history: Vec<crate::app::ChatEntry> = Vec::new();
    let mut token_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

    if config.session.enabled {
        if let Some(Ok(data)) =
            SessionData::load_default(config.session.save_file.as_deref())
        {
            // Restore from session Messages, preserving reasoning_content
            // and tool metadata for subsequent API round-trips.
            chat_history = data
                .chat_history
                .into_iter()
                .map(crate::app::ChatEntry::from_message)
                .collect();
            token_usage = data.token_usage;
            last_reasoning = data.last_reasoning;
            let turns = chat_history.iter().filter(|e| e.role == "user").count();
            tracing::info!(
                turns,
                tokens = token_usage.total_tokens(),
                "Resumed session"
            );
        }
    }

    // ── 6. Agent & tool registration ────────────────────────────────────────
    let mcp_tools = create_mcp_tools(&config).await;
    let (confirmation_handle, confirmation_rx) = ConfirmationHandle::new();

    let mut all_tools = crate::tools::ToolRegistry::from_config_and_handle(
        &config,
        confirmation_handle,
    );
    for tool in mcp_tools {
        all_tools.register_boxed(tool);
    }

    let client = build_client(&config);
    let system_prompt = build_preamble();
    let agent = Arc::new(Agent::new(client, system_prompt, all_tools));

    // ── 6b. Orchestrator (multi-agent coordination) ─────────────────────────
    let orchestrator = Arc::new(
        crate::core::agent::orchestrator::AgentOrchestrator::new(agent.clone(), &config),
    );

    // ── 7. Context manager & interrupt channel ──────────────────────────────
    let context_manager = ContextManager::new(&config);
    let (interrupt_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    let interrupt_tx_ctrlc = interrupt_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            let _ = interrupt_tx_ctrlc.send(());
        }
    });

    Ok(InitState {
        config,
        chat_history,
        token_usage,
        last_reasoning,
        agent,
        orchestrator,
        confirmation_rx: Some(confirmation_rx),
        interrupt_tx,
        context_manager,
    })
}
