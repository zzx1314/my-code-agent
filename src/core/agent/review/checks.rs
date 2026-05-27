//! Deterministic code structure checks for code review.
//!
//! Checks that don't require an LLM call: file length limits,
//! test existence, and source file classification.

use std::path::Path;

/// Check if a file exceeds the maximum line threshold.
pub fn check_file_too_long(path: &Path, max_lines: usize) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let line_count = content.lines().count();
    line_count > max_lines
}

/// Determine if a file is a Rust source file that should have tests.
///
/// Returns `true` for Rust source files under `src/`. Returns `false` for
/// existing test files (under `tests/`, ending with `_test.rs` or `_spec.rs`),
/// config/doc files, and files from other languages.
///
/// This is deliberately limited to `.rs` files to avoid false positives from
/// other languages whose test conventions differ (TypeScript uses `*.test.ts`,
/// Python uses `test_*.py`, etc.).
pub fn is_source_file(path: &Path) -> bool {
    let file_str = path.to_string_lossy();

    // Only check Rust source files
    if !file_str.ends_with(".rs") {
        return false;
    }

    // Skip files that are already test files themselves
    if file_str.ends_with("_test.rs") || file_str.ends_with("_spec.rs") {
        return false;
    }

    // Skip files under tests/, test-data/, fixtures/, mocks/ directories
    let skip_dir_patterns = ["/tests/", "/test-data/", "/fixtures/", "/mocks/"];
    if skip_dir_patterns.iter().any(|p| file_str.contains(p)) {
        return false;
    }

    // Skip config/doc/generated files
    let skip_extensions = [".toml", ".lock", ".md"];
    if skip_extensions.iter().any(|ext| file_str.ends_with(ext)) {
        return false;
    }

    true
}

/// Check if a source file has either inline tests or a corresponding test file.
pub fn has_tests(path: &Path) -> bool {
    // Strategy 1: Check if the source file itself contains inline tests
    if has_inline_tests(path) {
        return true;
    }

    // Strategy 2: Check for a corresponding test file
    derive_test_path(path)
        .map(|test_path| test_path.exists())
        .unwrap_or(false)
}

/// Check if a source file contains inline test annotations.
fn has_inline_tests(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };

    // Check for Rust-style inline tests
    if content.contains("#[cfg(test)]") || content.contains("#[test]") {
        return true;
    }

    false
}

/// Derive a potential test file path from a source file path.
///
/// For `src/foo/bar.rs`, checks:
/// - `tests/foo/bar.rs` (mirrored path)
/// - `tests/test_bar.rs` (test_<stem>)
///
/// For `src/foo/bar/mod.rs`, additionally checks:
/// - `tests/foo/bar.rs` (parent-dir name for mod.rs)
///
/// For the root `src/lib.rs`, checks `tests/lib.rs` and `tests/test_lib.rs`.
fn derive_test_path(source_path: &Path) -> Option<std::path::PathBuf> {
    let file_str = source_path.to_string_lossy();

    // Strategy A: Strip "src/" prefix, replace with "tests/"
    if let Some(relative) = file_str.strip_prefix("src/") {
        let test_path = Path::new("tests").join(relative);
        if test_path.exists() {
            return Some(test_path);
        }

        // Strategy C: For mod.rs files under a subdirectory, try replacing
        // "mod.rs" with the parent directory name + ".rs".
        // E.g. src/core/skill/mod.rs → tests/core/skill.rs
        if relative.ends_with("/mod.rs") {
            if let Some(parent) = Path::new(relative).parent() {
                if let Some(dir_name) = parent.file_name() {
                    let alt_path = Path::new("tests")
                        .join(parent)
                        .with_file_name(format!("{}.rs", dir_name.to_str()?));
                    if alt_path.exists() {
                        return Some(alt_path);
                    }
                }
            }
        }
    }

    // Strategy B: Look for tests/test_<filename>
    let filename = source_path.file_stem()?.to_str()?;
    let test_file = format!("tests/test_{}.rs", filename);
    let test_path = Path::new(&test_file);
    if test_path.exists() {
        return Some(test_path.to_path_buf());
    }

    None
}
