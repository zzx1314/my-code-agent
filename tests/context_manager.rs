use my_code_agent::core::config::Config;
use my_code_agent::core::context_manager::ContextManager;
use rig::completion::Message;
use rig::message::UserContent;
use rig::one_or_many::OneOrMany;

fn make_user_message(content: &str) -> Message {
    Message::User {
        content: OneOrMany::one(UserContent::Text(content.into())),
    }
}

fn make_assistant_message(content: &str) -> Message {
    Message::Assistant {
        id: None,
        content: OneOrMany::one(rig::completion::AssistantContent::Text(content.into())),
    }
}

#[test]
fn test_prune_messages() {
    let mut config = Config::default();
    config.context.window_size = 1000;
    config.context.warn_threshold_percent = 75;
    
    let manager = ContextManager::new(&config);
    
    let messages = vec![
        make_user_message("First message with some content"),
        make_assistant_message("First response with content"),
        make_user_message("Second message"),
        make_assistant_message("Second response"),
        make_user_message("Third message"),
        make_assistant_message("Third response"),
    ];
    
    let pruned = manager.prune_messages(&messages);
    
    assert!(!pruned.is_empty());
    assert!(pruned.len() <= messages.len());
}

#[test]
fn test_should_compact() {
    let mut config = Config::default();
    config.context.window_size = 1000;
    config.context.warn_threshold_percent = 75;
    config.context.critical_threshold_percent = 90;
    
    let manager = ContextManager::new(&config);
    
    assert!(!manager.should_compact(500));
    assert!(manager.should_compact(950));
}

#[test]
fn test_should_warn() {
    let mut config = Config::default();
    config.context.window_size = 1000;
    config.context.warn_threshold_percent = 75;
    config.context.critical_threshold_percent = 90;
    
    let manager = ContextManager::new(&config);
    
    assert!(!manager.should_warn(500));
    assert!(manager.should_warn(800));
    assert!(!manager.should_warn(950));
}