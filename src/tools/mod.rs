// ── Sub-modules by category ─────────────────────────────────────────────────
pub mod exec;
pub mod fs;
pub mod git;
pub mod infra;
pub mod search;

// ── Re-exports from sub-modules ─────────────────────────────────────────────
pub use exec::*;
pub use fs::*;
pub use git::*;
pub use infra::*;
pub use search::*;

// Re-export specific items that need special handling
pub use exec::confirmation::ConfirmationHandle;
pub use exec::safety::{
    is_dangerous_deletion, is_dangerous_git_command, is_dangerous_shell_command,
    is_dangerous_snippet_deletion,
};

use crate::core::config::Config;
use crate::core::tool::ToolRegistry as CoreToolRegistry;
use crate::core::types::ToolDefinition;
use crate::tools::exec::confirmation::ConfirmationHandle as _ConfirmationHandle;

/// Our own Tool trait for tool implementations.
/// Replaces `rig::tool::Tool` / `rig::tool::ToolDyn`.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;
}

/// Wrap our Tool trait so it can be used with the core ToolRegistry.
struct ToolWrapper {
    inner: Box<dyn Tool>,
}

#[async_trait::async_trait]
impl crate::core::tool::Tool for ToolWrapper {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn definition(&self) -> ToolDefinition {
        self.inner.definition()
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        self.inner.call(args).await
    }
}

/// Extension: build a ToolRegistry from config and confirmation handle.
impl CoreToolRegistry {
    /// Create a new ToolRegistry with all built-in tools.
    pub fn from_config(config: &Config) -> Self {
        let handle = ConfirmationHandle::disabled();
        Self::from_config_and_handle(config, handle)
    }

    /// Create a new ToolRegistry with all built-in tools and a confirmation handle.
    pub fn from_config_and_handle(config: &Config, handle: _ConfirmationHandle) -> Self {
        let mut registry = CoreToolRegistry::new();

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(FileRead::from_config(config)),
            Box::new(FileOutline),
            Box::new(FileWrite),
            Box::new(FileUpdate),
            Box::new(FileDelete::new(handle.clone())),
            Box::new(ShellExec::new(config.shell.default_timeout_secs, handle.clone())),
            Box::new(CodeSearch),
            Box::new(CodeReview),
            Box::new(ListDir),
            Box::new(GlobSearch),
            Box::new(GitStatus),
            Box::new(GitDiff),
            Box::new(GitLog),
            Box::new(GitCommit::new(handle)),
            Box::new(FileUndo),
        ];

        for tool in tools {
            registry.register_dyn(tool);
        }

        registry
    }

    /// Register a tool using our local Tool trait.
    pub fn register_dyn(&mut self, tool: Box<dyn Tool>) {
        let wrapper = ToolWrapper { inner: tool };
        self.register(wrapper);
    }
}

/// Create MCP tools (Parallel Search MCP) returning boxed local Tool trait objects.
pub async fn create_mcp_tools(config: &Config) -> Vec<Box<dyn Tool>> {
    let mut mcp_tools: Vec<Box<dyn Tool>> = Vec::new();

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

    let search_tool = crate::tools::search::web_search::ParallelWebSearch::new(&key);
    if search_tool.is_available() {
        tracing::info!("web_search tool added");
        mcp_tools.push(Box::new(search_tool));
    }

    let fetch_tool = crate::tools::search::web_search::ParallelWebFetch::new(&key);
    if fetch_tool.is_available() {
        tracing::info!("web_fetch tool added");
        mcp_tools.push(Box::new(fetch_tool));
    }

    mcp_tools
}
