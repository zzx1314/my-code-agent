// Configuration
pub mod config;
pub mod paths;

// Agent: LLM connection, preamble, streaming
pub mod agent;

// Context: file attachment, caching, token tracking
pub mod context;

// Code parsing
pub mod parser;

// Session persistence
pub mod session;

// Core types, tool trait
pub mod types;
pub mod tool;

// agent submodules re-exported at top level
pub use agent::connection;
pub use agent::preamble;
pub use agent::stream_response as streaming;

// context submodules re-exported at top level
pub use context::context_cache;
pub use context::context_manager;
pub use context::file_cache;
pub use context::token_usage;
pub use context::tool_dedup;
