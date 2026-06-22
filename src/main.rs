use anyhow::Result;
use my_code_agent::app::bootstrap::init_app;
use my_code_agent::app::headless::run_headless;
use my_code_agent::app::lifecycle::run_app;

#[tokio::main]
async fn main() -> Result<()> {
    let state = init_app().await?;

    // ── Headless mode: WebSocket client enabled ───────────────────────────
    if state.config.ws_client.enabled {
        return run_headless(
            state.config,
            state.agent,
            state.chat_history,
            state.token_usage,
            state.context_manager,
            state.interrupt_tx,
        )
        .await;
    }

    // ── TUI mode (default) ────────────────────────────────────────────────
    run_app(
        state.chat_history,
        state.token_usage,
        state.last_reasoning,
        state.config,
        state.agent,
        state.orchestrator,
        state.interrupt_tx,
        state.confirmation_rx,
        state.context_manager,
    )
    .await
}
