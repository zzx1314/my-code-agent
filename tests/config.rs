use my_code_agent::core::config::Config;
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
    assert_eq!(
        parsed.files.default_read_limit,
        config.files.default_read_limit
    );
    assert_eq!(parsed.context.window_size, config.context.window_size);
    assert_eq!(parsed.agent.max_turns, config.agent.max_turns);
}
