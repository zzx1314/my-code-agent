use colored::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration file name (looked up in the current directory).
pub const CONFIG_FILE: &str = "config.toml";

/// Top-level configuration structure.
///
/// Loaded from `config.toml` in the project root. Missing fields use sensible defaults.
/// If the file doesn't exist, all defaults are used (no error).
#[derive(Debug, Clone, Deserialize, Serialize)]
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
}

/// File reading and attachment limits.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileConfig {
    /// Maximum lines returned by `file_read` when `limit` is not specified.
    /// Default: 200.
    pub default_read_limit: usize,
    /// Maximum lines included from a single `@filepath` attachment.
    /// Default: 500.
    pub attach_max_lines: usize,
    /// Maximum bytes included from a single `@filepath` attachment.
    /// Default: 51200 (50 KB).
    pub attach_max_bytes: usize,
}

/// Context window and token usage settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextConfig {
    /// Model context window size in tokens.
    /// Default: 65536 (64K).
    pub window_size: u64,
    /// Percentage at which to warn about context usage.
    /// Default: 75.
    pub warn_threshold_percent: u64,
    /// Percentage at which to issue a critical context warning.
    /// Default: 90.
    pub critical_threshold_percent: u64,
}

/// Shell execution settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShellConfig {
    /// Default command timeout in seconds.
    /// Default: 30.
    pub default_timeout_secs: u64,
}

/// Agent behavior settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    /// Maximum number of tool-call turns per response.
    /// Default: 10.
    pub max_turns: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            files: FileConfig::default(),
            context: ContextConfig::default(),
            shell: ShellConfig::default(),
            agent: AgentConfig::default(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.files.default_read_limit, 200);
        assert_eq!(config.files.attach_max_lines, 500);
        assert_eq!(config.files.attach_max_bytes, 50 * 1024);
        assert_eq!(config.context.window_size, 65_536);
        assert_eq!(config.context.warn_threshold_percent, 75);
        assert_eq!(config.context.critical_threshold_percent, 90);
        assert_eq!(config.shell.default_timeout_secs, 30);
        assert_eq!(config.agent.max_turns, 10);
    }

    #[test]
    fn test_load_missing_file_uses_defaults() {
        let config = Config::load_from("/nonexistent/config.toml");
        assert_eq!(config.files.default_read_limit, 200);
        assert_eq!(config.agent.max_turns, 10);
    }

    #[test]
    fn test_load_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[files]\ndefault_read_limit = 100\n[context]\nwindow_size = 128000\n[shell]\ndefault_timeout_secs = 60\n[agent]\nmax_turns = 15\n",
        )
        .unwrap();

        let config = Config::load_from(&path);
        assert_eq!(config.files.default_read_limit, 100);
        assert_eq!(config.files.attach_max_lines, 500); // default
        assert_eq!(config.context.window_size, 128_000);
        assert_eq!(config.shell.default_timeout_secs, 60);
        assert_eq!(config.agent.max_turns, 15);
    }

    #[test]
    fn test_load_partial_toml_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[files]\ndefault_read_limit = 50\n").unwrap();

        let config = Config::load_from(&path);
        assert_eq!(config.files.default_read_limit, 50);
        assert_eq!(config.files.attach_max_lines, 500); // default
        assert_eq!(config.context.window_size, 65_536); // default
        assert_eq!(config.shell.default_timeout_secs, 30); // default
    }

    #[test]
    fn test_load_invalid_toml_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is not valid toml [[[[").unwrap();

        let config = Config::load_from(&path);
        assert_eq!(config.files.default_read_limit, 200); // default
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.files.default_read_limit, config.files.default_read_limit);
        assert_eq!(parsed.context.window_size, config.context.window_size);
        assert_eq!(parsed.agent.max_turns, config.agent.max_turns);
    }
}
