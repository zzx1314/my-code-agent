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

/// Create MCP tools (Parallel Search MCP).
pub async fn create_mcp_tools(config: &Config) -> Vec<Box<dyn ToolDyn>> {
    let mut mcp_tools: Vec<Box<dyn ToolDyn>> = Vec::new();

    if !config.mcp.enabled {
        return mcp_tools;
    }

    let parallel_api_key = config
        .mcp
        .parallel_api_key
        .clone()
        .or_else(|| std::env::var("PARALLEL_API_KEY").ok());

    if let Some(key) = parallel_api_key {
        let search_tool = crate::mcp::web_search_tool::ParallelWebSearch::new(&key);
        if search_tool.is_available() {
            eprintln!("{} web_search tool added", "✓".bright_green());
            mcp_tools.push(Box::new(search_tool) as Box<dyn ToolDyn>);
        }

        let fetch_tool = crate::mcp::web_search_tool::ParallelWebFetch::new(&key);
        if fetch_tool.is_available() {
            eprintln!("{} web_fetch tool added", "✓".bright_green());
            mcp_tools.push(Box::new(fetch_tool) as Box<dyn ToolDyn>);
        }
    } else {
        eprintln!(
            "{} PARALLEL_API_KEY not set. Set in config.toml or .env",
            "✗".bright_red()
        );
    }

    mcp_tools
}
