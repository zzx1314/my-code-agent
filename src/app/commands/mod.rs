mod clear;
pub mod compact;
mod connect;
mod help;
mod init;
mod load;
mod model;
mod plan;
mod quit;
pub mod review;
mod save;
mod shell;
mod skill;
mod status;
mod think;
mod tokens;
mod undo;

use crate::app::App;
use crate::core::context::context_manager::ContextManager;

/// Handle commands (input starting with /)
/// Returns true if the command was handled, false if it should be sent to the LLM
pub fn handle_command(
    app: &mut App,
    input: &str,
    context_manager: &mut ContextManager,
) -> bool {
    let command = input.trim().to_lowercase();

    // Check if any skill has a matching command first
    if let Some(skill_cfg) = app.skill_manager.find_by_command(&command) {
        let skill_cfg = skill_cfg.clone();
        if !app.skill_manager.is_active(&skill_cfg.name) {
            app.skill_manager.activate(&skill_cfg.name);
            app.status_messages.push(format!(
                "✅ Skill '{}' activated via {}",
                skill_cfg.name,
                skill_cfg.command.as_ref().unwrap()
            ));
        }
        return true;
    }

    match command.as_str() {
        "/help" => help::handle(app),
        "/quit" => quit::handle(app),
        "/clear" => clear::handle(app),
        "/save" => save::handle(app),
        "/load" => load::handle(app),
        "/status" => status::handle(app),
        "/tokens" => tokens::handle(app),
        cmd if cmd.starts_with("/connect") => connect::handle(app, input),
        "/think" => think::handle(app),
        "/model" => model::handle(app),
        "/init" => init::handle(app),
        "/undo" => undo::handle(app),
        "/shell" => shell::handle(app),
        cmd if cmd.starts_with("/compact") => compact::handle(app, input, context_manager),
        cmd if cmd.starts_with("/plan") => plan::handle(app, input, context_manager),
        cmd if cmd.starts_with("/review") => review::handle(app, input, context_manager),
        cmd if cmd.starts_with("/skill") => skill::handle(app, input),
        _ => {
            // Unknown command, send to the LLM for handling
            false
        }
    }
}
