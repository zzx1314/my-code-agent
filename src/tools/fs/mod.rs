pub mod apply_patch;
pub mod file_delete;
pub mod propose_str_replace;
pub mod file_outline;
pub mod file_read;
pub mod file_undo;
pub mod file_update;
pub mod file_write;
pub mod glob;
pub mod list_dir;

pub use apply_patch::ApplyPatch;
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
pub use propose_str_replace::ProposeStrReplace;

use crate::core::context::tool_dedup::get_global_tool_dedup;
use crate::tools::infra::undo_history;

// ─────────────────────────────────────────────────────────────────────────────
// Shared tracking utilities for file write/delete operations
// ─────────────────────────────────────────────────────────────────────────────

/// Write content to a file and perform all post-write tracking:
/// - Record undo history (reads old content from disk for the undo entry)
/// - Write the new content to disk
/// - Invalidate the dedup cache
/// - Run git diff to show what changed
///
/// `old_content`: if `Some`, use it directly for the undo entry (avoids redundant read);
/// if `None`, reads from disk (returns `None` if the file didn't exist).
///
/// Returns `(bytes_written, git_diff_output)`.
pub async fn fs_write_with_tracking(
    path: &str,
    content: &str,
    operation: &str,
    old_content: Option<String>,
) -> Result<(usize, Option<String>), String> {
    // Read old content for undo (None if file doesn't exist), unless caller provided it
    let old = match old_content {
        Some(c) => Some(c),
        None => tokio::fs::read_to_string(path).await.ok(),
    };

    // Record undo
    let _ = undo_history::record_change(path, old, Some(content.to_string()), operation);

    // Write file
    let bytes = content.len();
    tokio::fs::write(path, content).await.map_err(|e| e.to_string())?;

    // Invalidate dedup cache
    invalidate_dedup_cache(path);

    // Run git diff
    let git_diff = run_git_diff(path).await;

    Ok((bytes, git_diff))
}

/// Delete a file and perform all tracking:
/// - Record undo history (with old content for potential restore)
/// - Invalidate the dedup cache
/// - Run git diff before deletion
///
/// Returns the git diff output (before deletion), or `None` if not in a git repo.
pub async fn fs_delete_with_tracking(path: &str) -> Result<Option<String>, String> {
    // Git diff before deletion
    let git_diff = run_git_diff(path).await;

    // Read old content for undo
    let old_content = tokio::fs::read_to_string(path).await.ok();

    // Record undo
    let _ = undo_history::record_change(path, old_content, None, "file_delete");

    // Delete file
    tokio::fs::remove_file(path).await.map_err(|e| e.to_string())?;

    // Invalidate dedup cache
    invalidate_dedup_cache(path);

    Ok(git_diff)
}

/// Invalidate the dedup cache for a given file path.
fn invalidate_dedup_cache(path: &str) {
    let dedup = get_global_tool_dedup();
    let mut guard = dedup.lock().unwrap();
    guard.invalidate_path(path);
}

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
