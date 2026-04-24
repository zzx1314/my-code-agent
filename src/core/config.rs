use colored::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Serde default functions ──
// Each must be a visible fn() -> T matching the field type.

fn default_read_limit() -> usize { 200 }
fn default_attach_max_lines() -> usize { 500 }
fn default_attach_max_bytes() -> usize { 50 * 1024 }
fn default_window_size() -> u64 { 65_536 }
fn default_warn_threshold_percent() -> u64 { 75 }
fn default_critical_threshold_percent() -> u64 { 90 }
fn default_timeout_secs() -> u64 { 30 }
fn default_max_turns() -> usize { 10 }

/// Configuration file name (looked up in the current directory).
pub const CONFIG_FILE: &str = "config.toml";

/// Top-level configuration structure.
///
/// Loaded from `config.toml` in the project root. Missing fields use sensible defaults.
/// If the file doesn't exist, all defaults are used (no error).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    /// File reading / attachment settings.
    #[serde(default)]
    pub files: FileConfig,
    /// Token usage and context window settings.
    #[serde(default)]
    pub context: ContextConfig,
    /// Shell execution settings.
    #[serde(default)]
    pub shell: ShellConfig,
    /// Agent behavior settings.
    #[serde(default)]
    pub agent: AgentConfig,
    /// Session persistence settings.
    #[serde(default)]
    pub session: SessionConfig,
}

/// File reading and attachment limits.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileConfig {
    /// Maximum lines returned by `file_read` when `limit` is not specified.
    /// Default: 200.
    #[serde(default = "default_read_limit")]
    pub default_read_limit: usize,
    /// Maximum lines included from a single `@filepath` attachment.
    /// Default: 500.
    #[serde(default = "default_attach_max_lines")]
    pub attach_max_lines: usize,
    /// Maximum bytes included from a single `@filepath` attachment.
    /// Default: 51200 (50 KB).
    #[serde(default = "default_attach_max_bytes")]
    pub attach_max_bytes: usize,
}

/// Context window and token usage settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextConfig {
    /// Model context window size in tokens.
    /// Default: 65536 (64K).
    #[serde(default = "default_window_size")]
    pub window_size: u64,
    /// Percentage at which to warn about context usage.
    /// Default: 75.
    #[serde(default = "default_warn_threshold_percent")]
    pub warn_threshold_percent: u64,
    /// Percentage at which to issue a critical context warning.
    /// Default: 90.
    #[serde(default = "default_critical_threshold_percent")]
    pub critical_threshold_percent: u64,
}

/// Shell execution settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShellConfig {
    /// Default command timeout in seconds.
    /// Default: 30.
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,
}

/// Agent behavior settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    /// Maximum number of tool-call turns per response.
    /// Default: 10.
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
}

/// Session persistence settings.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SessionConfig {
    /// File path for session persistence.
    /// Default: `.session.json` (in the current directory).
    /// Set to `""` or omit to use the default.
    #[serde(default)]
    pub save_file: Option<String>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            default_read_limit: 200,
            attach_max_lines: 500,
            attach_max_bytes: 50 * 1024, // 50 KB
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            window_size: 65_536,
            warn_threshold_percent: 75,
            critical_threshold_percent: 90,
        }
    }
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
        }
    }
}

impl Config {
    /// Loads configuration from `config.toml` in the current directory.
    /// Returns defaults if the file doesn't exist. Prints a warning and
    /// uses defaults if the file contains invalid TOML.
    pub fn load() -> Self {
        Self::load_from(CONFIG_FILE)
    }

    /// Loads configuration from a specific path.
    /// Returns defaults if the file doesn't exist.
    pub fn load_from<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let config: Config = match toml::from_str(&content) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!(
                            "{} Error parsing {}: {}. Using defaults.",
                            "✗".bright_red(),
                            path.display(),
                            e
                        );
                        return Self::default();
                    }
                };
                println!(
                    "  {} {}",
                    "⚙".bright_cyan(),
                    format!("loaded: {} ", path.display()).dimmed()
                );
                config
            }
            Err(_) => Self::default(),
        }
    }
}
