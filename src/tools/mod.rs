pub mod code_search;
pub mod file_delete;
pub mod file_read;
pub mod file_update;
pub mod file_write;
pub mod glob;
pub mod list_dir;
pub mod safety;
pub mod shell_exec;

pub use code_search::CodeSearch;
pub use file_delete::FileDelete;
pub use file_read::FileRead;
pub use file_update::FileUpdate;
pub use file_update::build_diff;
pub use file_write::FileWrite;
pub use glob::GlobSearch;
pub use list_dir::ListDir;
pub use safety::{
    is_dangerous_deletion, is_dangerous_shell_command, is_dangerous_snippet_deletion,
};
pub use shell_exec::ShellExec;

use crate::core::config::Config;
use colored::Colorize;
use rig::tool::ToolDyn;

/// Returns all tools boxed as `Box<dyn ToolDyn>` for registration with the agent builder.
/// Config values are passed through to tool structs that need them.
pub fn all_tools(config: &Config) -> Vec<Box<dyn ToolDyn>> {
    let tools: Vec<Box<dyn ToolDyn>> = vec![
        Box::new(FileRead::from_config(config)),
        Box::new(FileWrite),
        Box::new(FileUpdate),
        Box::new(FileDelete),
        Box::new(ShellExec::from_config(config)),
        Box::new(CodeSearch),
        Box::new(ListDir),
        Box::new(GlobSearch),
    ];

    // Add MCP WebSearch tool if enabled
    if config.mcp.enabled {
        eprintln!(
            "{} MCP is enabled but WebSearch tool must be initialized at startup",
            "⚠".bright_yellow()
        );
    }

    tools
}

/// Create and add the WebSearch tool if MCP is enabled.
/// This should be called at startup with the config.
pub async fn create_mcp_tools(config: &Config) -> Vec<Box<dyn ToolDyn>> {
    let mut mcp_tools: Vec<Box<dyn ToolDyn>> = Vec::new();

    if !config.mcp.enabled {
        return mcp_tools;
    }

    // Get Brave API key from config or environment
    let brave_api_key = config
        .mcp
        .brave_api_key
        .clone()
        .or_else(|| std::env::var("BRAVE_API_KEY").ok());

    let api_key = match brave_api_key {
        Some(key) => key,
        None => {
            eprintln!(
                "{} BRAVE_API_KEY not set. MCP WebSearch will not be available.",
                "✗".bright_red()
            );
            return mcp_tools;
        }
    };

    // Create WebSearch tool
    match crate::mcp::web_search_tool::WebSearch::new(&api_key).await {
        Ok(tool) => {
            if tool.is_available() {
                eprintln!("{} MCP WebSearch tool added", "✓".bright_green());
                mcp_tools.push(Box::new(tool) as Box<dyn ToolDyn>);
            }
        }
        Err(e) => {
            eprintln!("{} Failed to create WebSearch tool: {}", "✗".bright_red(), e);
        }
    }

    mcp_tools
}
