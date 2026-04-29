use anyhow::Result;

use my_code_agent::core::config::Config;
use my_code_agent::core::context_manager::ContextManager;
use my_code_agent::core::preamble::build_agent;
use my_code_agent::core::session::SessionData;
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::tools::create_mcp_tools;
use my_code_agent::app::App;
use my_code_agent::app;
use my_code_agent::app::conversion::convert_rig_to_app;

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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tui_markdown=off"))
        )
        .init();

    let config = Config::load();

    let mut app_chat_history: Vec<(String, String)> = Vec::new();
    let mut token_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

    // Try to resume session if enabled
    if config.session.enabled {
        if let Some(load_result) = SessionData::load_default(config.session.save_file.as_deref()) {
            if let Ok(data) = load_result {
                app_chat_history = data.chat_history.into_iter().map(convert_rig_to_app).collect();
                token_usage = data.token_usage;
                last_reasoning = data.last_reasoning;
                let turns = app_chat_history.iter().filter(|(r, _)| r == "user").count();
                tracing::info!(turns, tokens = token_usage.total_tokens(), "Resumed session");
            }
        }
    }

    let mcp_tools = create_mcp_tools(&config).await;
    let agent = Arc::new(build_agent(&config, mcp_tools));

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

    let mut context_manager = context_manager;

    // Main loop
    loop {
        // 更新跑马灯帧计数器
        if app.is_streaming {
            app.marquee_frame = app.marquee_frame.wrapping_add(1);
        } else {
            app.marquee_frame = 0;
        }
        
        terminal.draw(|f| app::ui::ui(f, &mut app))?;

        // Check for completed stream result
        app::event_handler::check_stream_result(&mut app);

        // Poll streaming text events for live display
        app::event_handler::process_streaming_events(&mut app);

        if crossterm::event::poll(Duration::from_millis(100))? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => {
                    app::event_handler::handle_key_event(key, &mut app, &mut context_manager);
                }
                crossterm::event::Event::Mouse(mouse) => {
                    app::event_handler::handle_mouse_event(mouse, &mut app);
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

    // Save session if enabled
    if app.config.session.enabled && !app.chat_history.is_empty() {
        use my_code_agent::core::session::SessionData;
        
        let rig_history: Vec<_> = app.chat_history.into_iter()
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
