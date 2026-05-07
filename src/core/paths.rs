use std::path::PathBuf;
use std::sync::OnceLock;

/// Directory name under the current working directory for all runtime files.
const APP_DIR_NAME: &str = ".mycode";

/// Environment variable that can override the application home directory.
/// If set, all runtime files (config, logs, sessions, undo history) will be
/// stored under this directory instead of `<cwd>/.mycode/`.
const APP_HOME_ENV: &str = "MY_CODE_AGENT_HOME";

/// Cached application base directory.
static APP_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Returns the application's base directory for all runtime files.
///
/// Resolution order:
/// 1. `MY_CODE_AGENT_HOME` environment variable (if set and non-empty)
/// 2. `<current working directory>/.mycode`
///
/// The result is cached after the first call.
pub fn app_dir() -> &'static PathBuf {
    APP_DIR.get_or_init(|| {
        // 1. Check environment variable
        if let Ok(dir) = std::env::var(APP_HOME_ENV) {
            if !dir.is_empty() {
                let path = PathBuf::from(dir);
                if !path.exists() {
                    let _ = std::fs::create_dir_all(&path);
                }
                return path;
            }
        }

        // 2. Default: <cwd>/.mycode
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let path = cwd.join(APP_DIR_NAME);
        if !path.exists() {
            let _ = std::fs::create_dir_all(&path);
        }
        path
    })
}

/// Returns the full path for a file in the application's base directory.
pub fn app_file(name: &str) -> PathBuf {
    app_dir().join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_file_returns_path_under_app_dir() {
        let path = app_file("test_config.toml");
        assert!(path.ends_with("test_config.toml"));
        assert_eq!(path.parent(), Some(app_dir().as_path()));
    }

    #[test]
    fn test_app_dir_ends_with_mycode() {
        let dir = app_dir();
        assert_eq!(dir.file_name(), Some(std::ffi::OsStr::new(".mycode")));
    }

    #[test]
    fn test_app_dir_is_consistent() {
        let dir1 = app_dir();
        let dir2 = app_dir();
        assert_eq!(dir1, dir2);
    }
}
