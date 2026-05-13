use tokio::sync::mpsc;

use crate::app::App;

/// Check if an /init result has arrived from the async task
pub fn check_init_result(app: &mut App) {
    if let Some(ref mut rx) = app.init_rx {
        match rx.try_recv() {
            Ok(result) => {
                app.chat_history.push(crate::app::ChatEntry::assistant(result.message));
                if let Some(new_agent) = result.new_agent {
                    app.agent = std::sync::Arc::new(new_agent);
                }
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.auto_scroll = true;
                app.scroll = u16::MAX;
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.auto_scroll = true;
            }
        }
    }
}
