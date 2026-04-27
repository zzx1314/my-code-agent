//! MCP (Model Context Protocol) module.
//!
//! This module provides integration with MCP servers, allowing the agent
//! to use tools provided by external MCP servers.

pub mod client;
pub mod types;
pub mod web_search_tool;

pub use client::McpHttpClient;
pub use types::*;
pub use web_search_tool::{ParallelWebSearch, ParallelWebFetch};
