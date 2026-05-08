mod clear;
mod connect;
mod help;
mod init;
mod load;
mod model;
mod plan;
mod quit;
mod save;
mod shell;
mod status;
mod think;
mod tokens;
mod undo;

use crate::app::App;
use crate::core::context_manager::ContextManager;

/// Handle commands (input starting with /)
/// Returns true if the command was handled, false if it should be sent to the LLM
pub(super) fn handle_command(app: &mut App, input: &str, context_manager: &mut ContextManager) -> bool {
    let command = input.trim().to_lowercase();

    match command.as_str() {
        "/help" => help::handle(app),
        "/quit" => quit::handle(app),
        "/clear" => clear::handle(app),
        "/save" => save::handle(app),
        "/load" => load::handle(app),
        "/status" => status::handle(app),
        "/tokens" => tokens::handle(app),
        "/connect" => connect::handle(app),
        "/think" => think::handle(app),
        "/model" => model::handle(app),
        "/init" => init::handle(app),
        "/undo" => undo::handle(app),
        "/shell" => shell::handle(app),
        cmd if cmd.starts_with("/plan") => plan::handle(app, input, context_manager),
        _ => {
            // Unknown command, send to the LLM for handling
            false
        }
    }
}
