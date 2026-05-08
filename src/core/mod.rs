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

// Task planning
pub mod plan;

// ── Backward-compatible re-exports ──────────────────────────────────────────
// These ensure that existing code using `crate::core::config`, `crate::core::context`,
// etc. continues to work without changes.

// config is still at crate::core::config
// paths is still at crate::core::paths
// parser is still at crate::core::parser
// session is still at crate::core::session

// plan submodules re-exported at top level
pub use plan::tracker as plan_tracker;
pub use plan::detect_task_plan;

// agent submodules re-exported at top level
pub use agent::connection;
pub use agent::preamble;
pub use agent::streaming;

// context submodules re-exported at top level
pub use context::context_cache;
pub use context::context_manager;
pub use context::file_cache;
pub use context::token_usage;
