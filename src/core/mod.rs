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

// Core types
pub mod types;
