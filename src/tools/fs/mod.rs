pub mod file_delete;
pub mod file_outline;
pub mod file_read;
pub mod file_undo;
pub mod file_update;
pub mod file_write;
pub mod glob;
pub mod list_dir;

pub use file_delete::FileDelete;
pub use file_outline::FileOutline;
pub use file_read::FileRead;
pub use file_undo::FileUndo;
pub use file_update::FileUpdate;
pub use file_update::build_diff;
pub use file_update::build_line_diff;
pub use file_write::FileWrite;
pub use glob::GlobSearch;
pub use list_dir::ListDir;

/// Run `git diff -- <path>` and return the diff output, or `None` if it fails or is empty.
pub async fn run_git_diff(path: &str) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("diff")
        .arg("--")
        .arg(path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        None
    } else {
        Some(diff)
    }
}
