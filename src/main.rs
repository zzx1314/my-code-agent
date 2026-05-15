use anyhow::Result;

use my_code_agent::app::lifecycle::run_app;
use my_code_agent::app::bootstrap::init_app;

#[tokio::main]
async fn main() -> Result<()> {
    let state = init_app().await?;

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
