use anyhow::Result;

use my_code_agent::app;
use my_code_agent::app::App;
use my_code_agent::app::conversion::convert_rig_to_app;
use my_code_agent::core::config::Config;
use my_code_agent::core::context_manager::ContextManager;
use my_code_agent::core::preamble::build_agent_with_confirmation;
use my_code_agent::core::session::SessionData;
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::tools::confirmation::ConfirmationHandle;
use my_code_agent::tools::create_mcp_tools;

use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let log_file = std::fs::File::create(".my-code-agent.log")
        .unwrap_or_else(|_| std::fs::File::create("/tmp/my-code-agent.log").unwrap());

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tui_markdown=off")),
        )
        .init();

    let config = Config::load();

    // Generate a unique session ID for undo history tracking
    let session_id = format!(
        "session_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    my_code_agent::tools::undo_history::set_session_id(session_id.clone());
    tracing::info!(session_id = %session_id, "Initialized session ID for undo tracking");

    let mut app_chat_history: Vec<(String, String)> = Vec::new();
    let mut token_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

    // Try to resume session if enabled
    if config.session.enabled {
        if let Some(load_result) = SessionData::load_default(config.session.save_file.as_deref()) {
            if let Ok(data) = load_result {
                app_chat_history = data
                    .chat_history
                    .into_iter()
                    .map(convert_rig_to_app)
                    .collect();
                token_usage = data.token_usage;
                last_reasoning = data.last_reasoning;
                let turns = app_chat_history.iter().filter(|(r, _)| r == "user").count();
                tracing::info!(
                    turns,
                    tokens = token_usage.total_tokens(),
                    "Resumed session"
                );
            }
        }
    }

    let mcp_tools = create_mcp_tools(&config).await;

    // Create confirmation channel for tool -> UI interaction
    let (confirmation_handle, confirmation_rx) = ConfirmationHandle::new();
    let agent = Arc::new(build_agent_with_confirmation(
        &config,
        mcp_tools,
        confirmation_handle,
    ));

    let context_manager = ContextManager::new(&config);

    let (interrupt_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    // Ctrl+C handler sends interrupt on broadcast channel
    let interrupt_tx_ctrlc = interrupt_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx_ctrlc.send(()).ok();
        }
    });

    // Enter alternate screen
    let mut terminal = app::event_handler::enter_terminal()?;

    // Create app
    let mut app = App::new(
        app_chat_history,
        token_usage,
        last_reasoning,
        config,
        agent,
        interrupt_tx,
    );
    app.confirmation_rx = Some(confirmation_rx);

    let mut context_manager = context_manager;

    // Main loop
    loop {
        if app.is_streaming {
            app.marquee_frame = app.marquee_frame.wrapping_add(1);
        } else {
            app.marquee_frame = 0;
        }

        app::event_handler::process_streaming_events(&mut app);
        app::event_handler::check_stream_result(&mut app);
        app::event_handler::check_init_result(&mut app);

        terminal.draw(|f| app::ui::ui(f, &mut app))?;

        app::event_handler::process_message_queue(&mut app, &mut context_manager);

        // Check for confirmation requests from tools
        if app.pending_confirmation.is_none() {
            if let Some(rx) = &mut app.confirmation_rx {
                if let Ok(req) = rx.try_recv() {
                    app.pending_confirmation = Some(crate::app::PendingConfirmation {
                        reason: req.reason,
                        detail: req.detail,
                        response_tx: req.response_tx,
                    });
                }
            }
        }

        if crossterm::event::poll(Duration::from_millis(100))? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => {
                    app::event_handler::handle_key_event(key, &mut app, &mut context_manager);
                }
                crossterm::event::Event::Mouse(mouse) => {
                    app::event_handler::handle_mouse_event(mouse, &mut app);
                }
                crossterm::event::Event::Paste(text) => {
                    app::event_handler::handle_paste_event(&text, &mut app);
                }
                _ => {}
            }
        }

        if app.should_exit {
            break;
        }
    }

    // Leave alternate screen
    app::event_handler::leave_terminal(&mut terminal)?;

    // Clean up undo history for current session if configured
    if app.config.session.cleanup_undo_history {
        match my_code_agent::tools::undo_history::clear_current_session_entries() {
            Ok(cleared) if cleared > 0 => {
                tracing::info!(
                    cleared,
                    "Cleaned up undo history for current session on exit"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "Failed to clean up undo history on exit");
            }
        }
    }

    // Auto-save a timestamped session to .sessions/ on exit (always, regardless of session.enabled)
    if !app.chat_history.is_empty() {
        use my_code_agent::core::session::SessionData;

        let rig_history: Vec<_> = app
            .chat_history
            .iter()
            .map(|(r, c)| match r.as_str() {
                "user" => rig::completion::Message::user(c.clone()),
                "assistant" => rig::completion::Message::assistant(c.clone()),
                _ => rig::completion::Message::user(c.clone()),
            })
            .collect();

        let name = my_code_agent::core::session::generate_session_name();
        let data = SessionData::with_name(
            rig_history,
            app.token_usage.clone(),
            app.last_reasoning.clone(),
            name.clone(),
        );
        match data.save_with_name(&name) {
            Ok(()) => {
                tracing::info!(name = %name, "Auto-saved session on exit");
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to auto-save session on exit");
            }
        }
    }

    // Save to default session file if session.enabled (for auto-resume)
    if app.config.session.enabled && !app.chat_history.is_empty() {
        use my_code_agent::core::session::SessionData;

        let rig_history: Vec<_> = app
            .chat_history
            .into_iter()
            .map(|(r, c)| match r.as_str() {
                "user" => rig::completion::Message::user(c),
                "assistant" => rig::completion::Message::assistant(c),
                _ => rig::completion::Message::user(c),
            })
            .collect();

        let data = SessionData::new(
            rig_history,
            app.token_usage.clone(),
            app.last_reasoning.clone(),
        );
        if let Err(e) = data.save_default(app.config.session.save_file.as_deref()) {
            tracing::error!(error = %e, "Failed to save session");
        }
    }

    Ok(())
}
