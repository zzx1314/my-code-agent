use async_trait::async_trait;

use crate::core::types::ToolDefinition;

/// A tool that can be called by the LLM.
///
/// Analogous to `rig::tool::Tool` / `rig::tool::ToolDyn`, but simplified:
/// - `call` takes a `serde_json::Value` (already-parsed JSON arguments) and returns a `String`.
/// - Tools are registered in a [`ToolRegistry`] keyed by name.
#[async_trait]
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
/// Replaces `Vec<Box<dyn ToolDyn>>` and the ad-hoc tool collections in `tools/mod.rs`.
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
}
