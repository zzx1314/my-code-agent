// Configuration
pub mod config;
pub mod paths;

// Agent: LLM connection, preamble, streaming
pub mod agent;

// Context: file attachment, caching, token tracking
pub mod context;

// Skills: reusable behavior packages
pub mod skill;

// Code parsing
pub mod parser;

// Session persistence
pub mod session;

// Core types
pub mod types;

// Chinese→English translation
pub mod translate;

// WebSocket client (headless mode)
pub mod ws_client;
