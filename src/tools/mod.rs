pub mod code_review;
pub mod code_search;
pub mod confirmation;
pub mod file_delete;
pub mod file_outline;
pub mod file_read;
pub mod file_update;
pub mod file_write;
pub mod git_commit;
pub mod git_diff;
pub mod git_log;
pub mod git_status;
pub mod glob;
pub mod list_dir;
pub mod safety;
pub mod shell_exec;
pub mod undo_history;
pub mod file_undo;

pub use code_review::CodeReview;
pub use code_search::CodeSearch;
pub use file_delete::FileDelete;
pub use file_outline::FileOutline;
pub use file_read::FileRead;
pub use file_update::FileUpdate;
pub use file_update::build_diff;
pub use file_write::FileWrite;
pub use git_commit::GitCommit;
pub use git_diff::GitDiff;
pub use git_log::GitLog;
pub use git_status::GitStatus;
pub use glob::GlobSearch;
pub use list_dir::ListDir;
pub use safety::{
    is_dangerous_deletion, is_dangerous_git_command, is_dangerous_shell_command,
    is_dangerous_snippet_deletion,
};
pub use shell_exec::ShellExec;
pub use file_undo::FileUndo;

use crate::core::config::Config;
use crate::tools::confirmation::ConfirmationHandle;
use rig::tool::ToolDyn;

/// Returns all tools boxed as `Box<dyn ToolDyn>` for registration with the agent builder.
/// Config values are passed through to tool structs that need them.
pub fn all_tools(config: &Config) -> Vec<Box<dyn ToolDyn>> {
    // Use a disabled handle by default (no UI feedback channel).
    let handle = ConfirmationHandle::disabled();
    all_tools_with_handle(config, handle)
}

/// Returns all tools with a confirmation handle for user interaction.
/// The handle is used by tools to request user confirmation for dangerous operations.
pub fn all_tools_with_handle(
    config: &Config,
    confirmation_handle: ConfirmationHandle,
) -> Vec<Box<dyn ToolDyn>> {
    vec![
        Box::new(FileRead::from_config(config)),
        Box::new(FileOutline),
        Box::new(FileWrite),
        Box::new(FileUpdate),
        Box::new(FileDelete::new(confirmation_handle.clone())),
        Box::new(ShellExec::new(config.shell.default_timeout_secs, confirmation_handle.clone())),
        Box::new(CodeSearch),
        Box::new(CodeReview),
        Box::new(ListDir),
        Box::new(GlobSearch),
        Box::new(GitStatus),
        Box::new(GitDiff),
        Box::new(GitLog),
        Box::new(GitCommit::new(confirmation_handle)),
        Box::new(FileUndo),
    ]
}

/// Create MCP tools (Parallel Search MCP).
pub async fn create_mcp_tools(config: &Config) -> Vec<Box<dyn ToolDyn>> {
    let mut mcp_tools: Vec<Box<dyn ToolDyn>> = Vec::new();

    if !config.mcp.enabled {
        return mcp_tools;
    }

    let key = config
        .mcp
        .parallel_api_key
        .as_ref()
        .filter(|k| !k.is_empty())
        .cloned()
        .unwrap_or_else(|| std::env::var("PARALLEL_API_KEY").unwrap_or_default());

    let search_tool = crate::mcp::web_search_tool::ParallelWebSearch::new(&key);
    if search_tool.is_available() {
        tracing::info!("web_search tool added");
        mcp_tools.push(Box::new(search_tool) as Box<dyn ToolDyn>);
    }

    let fetch_tool = crate::mcp::web_search_tool::ParallelWebFetch::new(&key);
    if fetch_tool.is_available() {
        tracing::info!("web_fetch tool added");
        mcp_tools.push(Box::new(fetch_tool) as Box<dyn ToolDyn>);
    }

    mcp_tools
}
