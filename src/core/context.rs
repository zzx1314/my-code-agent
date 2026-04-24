use crate::core::config::Config;
use colored::*;
use std::path::Path;

/// A parsed `@file` reference found in user input.
#[derive(Debug)]
struct FileRef {
    /// The byte range in the original input where the `@path` appears (start, end).
    span: (usize, usize),
    /// The resolved file path (as written by the user, after stripping the leading `@`).
    path: String,
}

/// Status of a single file attachment.
#[derive(Debug)]
pub enum AttachStatus {
    Attached { lines: usize, truncated: bool },
    Error(String),
}

/// Result of expanding file references in user input.
#[derive(Debug)]
pub struct ExpandResult {
    /// The expanded input string (with @refs replaced by file contents).
    pub expanded: String,
    /// List of (path, status) for each file reference encountered.
    pub attachments: Vec<(String, AttachStatus)>,
}

/// Parses all `@filepath` references from the input string.
///
/// A file reference starts with `@` (preceded by whitespace or at the start)
/// and continues until a whitespace character or end of string. Trailing
/// punctuation characters (`:`, `,`, `;`, `!`, `?`, `)`, `]`, `}`) are
/// stripped from the path. Email-like patterns (`user@host`) are excluded.
fn parse_file_refs(input: &str) -> Vec<FileRef> {
    let mut refs = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{')) {
            let at_pos = i;
            let start = i + 1; // skip '@'
            let mut end = start;

            // Consume until whitespace or end
            while end < chars.len() && !chars[end].is_whitespace() {
                end += 1;
            }

            if end > start {
                let path_str: String = chars[start..end].iter().collect();
                // Strip trailing punctuation from the path
                let path_clean = path_str
                    .trim_end_matches([':', ',', ';', '!', '?', ')', ']', '}'])
                    .to_string();

                if !path_clean.is_empty() {
                    let byte_start = chars[..at_pos].iter().collect::<String>().len();
                    // Span covers only @path_clean so trailing punctuation remains in output
                    let byte_end = byte_start + 1 + path_clean.len(); // +1 for '@'

                    refs.push(FileRef {
                        span: (byte_start, byte_end),
                        path: path_clean,
                    });
                }
            }

            i = end;
        } else {
            i += 1;
        }
    }

    refs
}

/// Reads a file and formats its content as a fenced code block.
/// Truncates files exceeding the configured line/byte limits.
fn format_file_content(path: &str, max_lines: usize, max_bytes: usize) -> Result<(String, usize, bool), std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let total_lines = content.lines().count();
    let mut truncated = false;

    // Truncate by lines
    let mut display_content = if total_lines > max_lines {
        truncated = true;
        content
            .lines()
            .take(max_lines)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content
    };

    // Truncate by bytes if still too large
    if display_content.len() > max_bytes {
        truncated = true;
        display_content.truncate(max_bytes);
        // Avoid cutting in the middle of a line
        if let Some(last_newline) = display_content.rfind('\n') {
            display_content.truncate(last_newline);
        }
    }

    let truncation_notice = if truncated {
        format!(
            "\n... (file truncated, showing partial content of {} total lines)",
            total_lines
        )
    } else {
        String::new()
    };

    let formatted = format!(
        "<file path=\"{}\" lines=\"{}\">\n```{}\n{}{}\n```\n</file>",
        path, total_lines, extension, display_content, truncation_notice
    );

    Ok((formatted, total_lines, truncated))
}

/// Expands all `@filepath` references in the input string by replacing them
/// with the actual file contents. Files that cannot be read are replaced with
/// an error notice.
///
/// Returns an `ExpandResult` with the expanded string and attachment metadata.
/// The caller is responsible for printing status messages.
///
/// # Example
/// ```text
/// Input:  "explain @src/main.rs"
/// Output: "explain <file path=\"src/main.rs\" lines=\"373\">\n```rust\n...\n```\n</file>"
/// ```
pub fn expand_file_refs(input: &str, config: &Config) -> ExpandResult {
    let refs = parse_file_refs(input);

    if refs.is_empty() {
        return ExpandResult {
            expanded: input.to_string(),
            attachments: Vec::new(),
        };
    }

    let max_lines = config.files.attach_max_lines;
    let max_bytes = config.files.attach_max_bytes;
    let mut attachments = Vec::new();

    // Process refs in reverse order so byte offsets remain valid
    let mut result = input.to_string();
    for file_ref in refs.into_iter().rev() {
        let replacement = match format_file_content(&file_ref.path, max_lines, max_bytes) {
            Ok((content, lines, truncated)) => {
                attachments.push((file_ref.path.clone(), AttachStatus::Attached { lines, truncated }));
                content
            }
            Err(e) => {
                let msg = e.to_string();
                attachments.push((file_ref.path.clone(), AttachStatus::Error(msg.clone())));
                format!("<file path=\"{}\" error=\"{}\" />", file_ref.path, msg)
            }
        };
        result.replace_range(file_ref.span.0..file_ref.span.1, &replacement);
    }

    // Reverse so attachments are in left-to-right order
    attachments.reverse();

    ExpandResult {
        expanded: result,
        attachments,
    }
}

/// Prints attachment status messages to the terminal.
pub fn print_attachments(attachments: &[(String, AttachStatus)]) {
    for (path, status) in attachments {
        match status {
            AttachStatus::Attached { lines, truncated } => {
                if *truncated {
                    println!(
                        "  {} {}",
                        "📎".bright_cyan(),
                        format!("attached: {} ({} lines, truncated)", path, lines).dimmed()
                    );
                } else {
                    println!(
                        "  {} {}",
                        "📎".bright_cyan(),
                        format!("attached: {} ({} lines)", path, lines).dimmed()
                    );
                }
            }
            AttachStatus::Error(msg) => {
                println!(
                    "  {} {}",
                    "⚠".bright_yellow(),
                    format!("could not read {}: {}", path, msg).dimmed()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_simple_ref() {
        let refs = parse_file_refs("explain @src/main.rs");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/main.rs");
    }

    #[test]
    fn test_parse_multiple_refs() {
        let refs = parse_file_refs("compare @src/main.rs and @src/lib.rs");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "src/main.rs");
        assert_eq!(refs[1].path, "src/lib.rs");
    }

    #[test]
    fn test_parse_trailing_punctuation() {
        let refs = parse_file_refs("look at @src/main.rs,");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/main.rs");
    }

    #[test]
    fn test_parse_trailing_punctuation_variants() {
        assert_eq!(parse_file_refs("@foo.rs:")[0].path, "foo.rs");
        assert_eq!(parse_file_refs("@foo.rs;")[0].path, "foo.rs");
        assert_eq!(parse_file_refs("@foo.rs!")[0].path, "foo.rs");
        assert_eq!(parse_file_refs("@foo.rs?")[0].path, "foo.rs");
        // @ inside opening brackets IS treated as a ref
        assert_eq!(parse_file_refs("(@foo.rs)")[0].path, "foo.rs");
        assert_eq!(parse_file_refs("[@foo.rs]")[0].path, "foo.rs");
        assert_eq!(parse_file_refs("{@foo.rs}")[0].path, "foo.rs");
    }

    #[test]
    fn test_parse_no_refs() {
        let refs = parse_file_refs("just a normal message");
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_parse_email_not_treated_as_ref() {
        let refs = parse_file_refs("send to user@example.com");
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_parse_at_start() {
        let refs = parse_file_refs("@README.md what is this");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
    }

    #[test]
    fn test_parse_dot_in_extension() {
        let refs = parse_file_refs("check @src/main.rs and @Cargo.toml");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "src/main.rs");
        assert_eq!(refs[1].path, "Cargo.toml");
    }

    #[test]
    fn test_expand_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let config = Config::default();
        let rel_path = file_path.to_str().unwrap();
        let input = format!("read @{}", rel_path);
        let result = expand_file_refs(&input, &config);
        assert!(result.expanded.contains("hello world"));
        assert!(result.expanded.contains("<file"));
        assert_eq!(result.attachments.len(), 1);
    }

    #[test]
    fn test_expand_missing_file() {
        let config = Config::default();
        let result = expand_file_refs("read @nonexistent/file.txt", &config);
        assert!(result.expanded.contains("error="));
        assert!(result.expanded.contains("<file"));
        assert_eq!(result.attachments.len(), 1);
    }

    #[test]
    fn test_expand_truncates_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("big.txt");
        let content: String = (0..600).map(|i| format!("line {}\n", i)).collect();
        fs::write(&file_path, &content).unwrap();

        let config = Config::default();
        let rel_path = file_path.to_str().unwrap();
        let input = format!("read @{}", rel_path);
        let result = expand_file_refs(&input, &config);
        assert!(result.expanded.contains("file truncated"));
        assert_eq!(result.attachments.len(), 1);
        match &result.attachments[0].1 {
            AttachStatus::Attached { truncated, .. } => assert!(truncated),
            _ => panic!("expected Attached status"),
        }
    }

    #[test]
    fn test_expand_no_refs_unchanged() {
        let config = Config::default();
        let result = expand_file_refs("just a normal message", &config);
        assert_eq!(result.expanded, "just a normal message");
        assert!(result.attachments.is_empty());
    }
}
