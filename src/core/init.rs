//! Application initialization: dotenv, logging, config, session restore, agent setup.

use std::sync::Arc;

use anyhow::Result;

use crate::app::conversion::convert_rig_to_app;
use crate::core::config::Config;
use crate::core::context_manager::ContextManager;
use crate::core::preamble::{Agent, build_agent_with_confirmation};
use crate::core::session::SessionData;
use crate::core::token_usage::TokenUsage;
use crate::tools::confirmation::{ConfirmationHandle, ConfirmationRequest};
use crate::tools::create_mcp_tools;

/// All state produced during initialization, handed off to the event loop.
pub struct InitState {
    pub config: Config,
    pub chat_history: Vec<(String, String)>,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub agent: Arc<Agent>,
    pub confirmation_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ConfirmationRequest>>,
    pub interrupt_tx: tokio::sync::broadcast::Sender<()>,
    pub context_manager: ContextManager,
}

/// Perform full application initialization:
/// 1. Load `.env`
/// 2. Set up tracing / logging
/// 3. Load config
/// 4. Generate session ID for undo tracking
/// 5. Restore session (if enabled)
/// 6. Build agent with MCP tools & confirmation channel
/// 7. Create context manager & interrupt channel
pub async fn init_app() -> Result<InitState> {
    // ── 1. Environment ──────────────────────────────────────────────────────
    let env_path = crate::core::paths::app_file(".env");
    if env_path.exists() {
        dotenv::from_path(&env_path).ok();
    } else {
        dotenv::dotenv().ok();
    }

    // ── 2. Logging ──────────────────────────────────────────────────────────
    let log_path = crate::core::paths::app_file(".my-code-agent.log");
    let log_file = std::fs::File::create(&log_path)
        .unwrap_or_else(|_| std::fs::File::create("/tmp/my-code-agent.log").unwrap());

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
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
    crate::tools::undo_history::set_session_id(session_id.clone());
    tracing::info!(session_id = %session_id, "Initialized session ID for undo tracking");

    // ── 5. Restore session ──────────────────────────────────────────────────
    let mut chat_history: Vec<(String, String)> = Vec::new();
    let mut token_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

    if config.session.enabled {
        if let Some(Ok(data)) =
            SessionData::load_default(config.session.save_file.as_deref())
        {
            chat_history = data
                .chat_history
                .into_iter()
                .map(convert_rig_to_app)
                .collect();
            token_usage = data.token_usage;
            last_reasoning = data.last_reasoning;
            let turns = chat_history.iter().filter(|(r, _)| r == "user").count();
            tracing::info!(
                turns,
                tokens = token_usage.total_tokens(),
                "Resumed session"
            );
        }
    }

    // ── 6. Agent & confirmation channel ─────────────────────────────────────
    let mcp_tools = create_mcp_tools(&config).await;
    let (confirmation_handle, confirmation_rx) = ConfirmationHandle::new();
    let agent = Arc::new(build_agent_with_confirmation(
        &config,
        mcp_tools,
        confirmation_handle,
    ));

    // ── 7. Context manager & interrupt channel ──────────────────────────────
    let context_manager = ContextManager::new(&config);
    let (interrupt_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    // Ctrl+C handler sends interrupt on broadcast channel
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
        confirmation_rx: Some(confirmation_rx),
        interrupt_tx,
        context_manager,
    })
}
