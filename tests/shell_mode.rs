use my_code_agent::app::App;
use my_code_agent::core::config::Config;
use my_code_agent::core::agent::preamble::{Agent, build_client, build_preamble};
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::tools::ToolRegistry;
use std::sync::Arc;

fn make_app(shell_mode: bool) -> App {
    let config = Config::default();
    let client = build_client(&config);
    let system_prompt = build_preamble();
    let tools = ToolRegistry::from_config(&config);
    let agent = Arc::new(Agent::new(client, system_prompt, tools));
    let (interrupt_tx, _) = tokio::sync::broadcast::channel(1);
    let token_usage = TokenUsage::with_config(&config);
    let mut app = App::new(
        vec![],
        token_usage,
        String::new(),
        config,
        agent,
        interrupt_tx,
    );
    app.shell_mode = shell_mode;
    app
}

#[test]
fn test_shell_mode_default_false() {
    let app = make_app(false);
    assert!(!app.shell_mode);
}

#[test]
fn test_shell_mode_can_be_enabled() {
    let app = make_app(true);
    assert!(app.shell_mode);
}

#[test]
fn test_shell_mode_toggle_via_command() {
    let mut app = make_app(false);
    assert!(!app.shell_mode);

    app.shell_mode = !app.shell_mode;
    assert!(app.shell_mode);

    app.shell_mode = !app.shell_mode;
    assert!(!app.shell_mode);
}
