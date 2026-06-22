//! Project type detection and file glob patterns.

use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) enum ProjectType {
    Rust,
    JavaScript,
    Python,
    Unknown,
}

pub(crate) fn detect_project_type() -> ProjectType {
    if Path::new("Cargo.toml").exists() {
        ProjectType::Rust
    } else if Path::new("package.json").exists() {
        ProjectType::JavaScript
    } else if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() {
        ProjectType::Python
    } else {
        ProjectType::Unknown
    }
}

pub(crate) fn project_type_label(pt: ProjectType) -> &'static str {
    match pt {
        ProjectType::Rust => "Rust",
        ProjectType::JavaScript => "JavaScript / TypeScript",
        ProjectType::Python => "Python",
        ProjectType::Unknown => "Unknown",
    }
}

pub(crate) fn source_globs(pt: ProjectType) -> &'static [&'static str] {
    match pt {
        ProjectType::Rust => &["**/*.rs"],
        ProjectType::JavaScript => &["**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx"],
        ProjectType::Python => &["**/*.py"],
        ProjectType::Unknown => &[
            "**/*.rs",
            "**/*.ts",
            "**/*.tsx",
            "**/*.js",
            "**/*.jsx",
            "**/*.py",
            "**/*.go",
            "**/*.java",
            "**/*.kt",
        ],
    }
}

pub(crate) fn test_globs(pt: ProjectType) -> &'static [&'static str] {
    match pt {
        ProjectType::Rust => &["tests/**/*.rs"],
        ProjectType::JavaScript => &[
            "**/*.test.ts",
            "**/*.test.tsx",
            "**/*.test.js",
            "**/*.test.jsx",
            "**/__tests__/**/*",
        ],
        ProjectType::Python => &["tests/**/*.py", "**/test_*.py"],
        ProjectType::Unknown => &[
            "tests/**/*.rs",
            "tests/**/*.py",
            "**/*.test.ts",
            "**/*.test.js",
            "**/test_*.py",
        ],
    }
}
