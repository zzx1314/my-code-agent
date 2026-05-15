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
use crate::core::types::ToolDefinition;

/// A tool that can be called by the LLM.
///
/// Analogous to `rig::tool::Tool` / `rig::tool::ToolDyn`, but simplified:
/// - `call` takes a `serde_json::Value` (already-parsed JSON arguments) and returns a `String`.
/// - Tools are registered in a [`ToolRegistry`] keyed by name.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// The tool's name (must match the name in [`ToolDefinition`]).
    fn name(&self) -> &str;

    /// The tool's definition for API registration.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given JSON arguments.
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;
}

/// A registry of tools, keyed by name.
///
/// Provides lookup by name for tool execution during the agent loop.
#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.push(Box::new(tool));
    }

    /// Register a pre-boxed tool.
    pub fn register_boxed(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    /// Iterate over all registered tools.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Tool> {
        self.tools.iter().map(|t| t.as_ref())
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Get all tool definitions for API registration.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|t| t.definition()).collect()
    }

    /// Execute a tool by name with JSON arguments.
    pub async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String, String> {
        match self.get(name) {
            Some(tool) => tool.call(args).await,
            None => Err(format!("Tool not found: {}", name)),
        }
    }

    /// Create a new ToolRegistry with all built-in tools.
    pub fn from_config(config: &Config) -> Self {
        let handle = ConfirmationHandle::disabled();
        Self::from_config_and_handle(config, handle)
    }

    /// Create a new ToolRegistry with all built-in tools and a confirmation handle.
    pub fn from_config_and_handle(config: &Config, handle: ConfirmationHandle) -> Self {
        let mut registry = ToolRegistry::new();

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
            registry.register_boxed(tool);
        }

        registry
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
