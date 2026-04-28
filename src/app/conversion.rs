use rig::completion::Message as RigMessage;
use rig::message::UserContent;
use rig::completion::AssistantContent;

/// Extract text from UserContent
pub fn text_from_user_content(content: &UserContent) -> String {
    match content {
        UserContent::Text(s) => s.text.clone(),
        _ => String::new(),
    }
}

/// Extract text from AssistantContent
pub fn text_from_assistant_content(content: &AssistantContent) -> String {
    match content {
        AssistantContent::Text(s) => s.text.clone(),
        _ => String::new(),
    }
}

/// Convert RigMessage to (role, text) tuple
pub fn convert_rig_to_app(msg: RigMessage) -> (String, String) {
    match msg {
        RigMessage::User { content } => {
            let text = text_from_user_content(&content.first());
            ("user".to_string(), text)
        }
        RigMessage::Assistant { content, .. } => {
            let text = text_from_assistant_content(&content.first());
            ("assistant".to_string(), text)
        }
        _ => ("unknown".to_string(), String::new()),
    }
}

/// Convert application chat history to RigMessage vector
pub fn convert_app_to_rig(chat_history: &[(String, String)]) -> Vec<RigMessage> {
    chat_history
        .iter()
        .map(|(role, content)| match role.as_str() {
            "user" => RigMessage::user(content),
            "assistant" => RigMessage::assistant(content),
            _ => RigMessage::user(""),
        })
        .collect()
}
