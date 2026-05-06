use my_code_agent::app::App;
use my_code_agent::core::config::Config;
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::core::preamble::build_agent;
use std::sync::Arc;

fn make_app() -> App {
    let config = Config::default();
    let agent = Arc::new(build_agent(&config, vec![]));
    let (interrupt_tx, _) = tokio::sync::broadcast::channel(1);
    let token_usage = TokenUsage::with_config(&config);
    App::new(vec![], token_usage, String::new(), config, agent, interrupt_tx)
}

#[test]
fn test_message_queue_initially_empty() {
    let app = make_app();
    assert!(app.message_queue.is_empty());
}

#[test]
fn test_message_queue_push() {
    let mut app = make_app();
    app.message_queue.push("hello".to_string());
    app.message_queue.push("world".to_string());
    assert_eq!(app.message_queue.len(), 2);
    assert_eq!(app.message_queue[0], "hello");
    assert_eq!(app.message_queue[1], "world");
}

#[test]
fn test_message_queue_fifo_order() {
    let mut app = make_app();
    app.message_queue.push("first".to_string());
    app.message_queue.push("second".to_string());
    app.message_queue.push("third".to_string());

    let first = app.message_queue.remove(0);
    assert_eq!(first, "first");
    assert_eq!(app.message_queue.len(), 2);

    let second = app.message_queue.remove(0);
    assert_eq!(second, "second");

    let third = app.message_queue.remove(0);
    assert_eq!(third, "third");
    assert!(app.message_queue.is_empty());
}
