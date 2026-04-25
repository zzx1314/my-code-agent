use crate::core::config::Config;
use crate::core::file_cache::FileCache;
use colored::*;
use std::path::Path;

/// A parsed `@file` reference found in user input.
#[derive(Debug)]
pub struct FileRef {
    /// The byte range in the original input where the `@path[:offset]` appears (start, end).
    pub span: (usize, usize),
    /// The resolved file path (as written by the user, after stripping the leading `@` and any `:offset` suffix).
    pub path: String,
    /// Line offset to start reading from (0-indexed). Parsed from `@path:N` syntax.
    pub offset: usize,
}

/// Status of a single file attachment.
#[derive(Debug)]
pub enum AttachStatus {
    Attached {
        /// Total lines in the file.
        lines: usize,
        /// Whether the output was truncated.
        truncated: bool,
        /// Line offset applied (0-indexed). 0 means reading from the start.
        offset: usize,
        /// Number of lines actually shown (after offset and truncation).
        shown: usize,
    },
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
/// and continues until a whitespace character or end of string. An optional
/// `:N` suffix specifies a line offset (0-indexed), e.g. `@src/main.rs:50`
/// starts reading from line 50. Trailing punctuation characters (`,`, `;`,
/// `!`, `?`, `)`, `]`, `}`) are stripped from the path. A trailing bare `:`
/// (not followed by digits) is also stripped. Email-like patterns (`user@host`)
/// are excluded.
pub fn parse_file_refs(input: &str) -> Vec<FileRef> {
    let mut refs = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@'
            && (i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{'))
        {
            let at_pos = i;
            let start = i + 1; // skip '@'
            let mut end = start;

            // Consume until whitespace or end
            while end < chars.len() && !chars[end].is_whitespace() {
                end += 1;
            }

            if end > start {
                let token: String = chars[start..end].iter().collect();
                // Strip trailing punctuation (but NOT `:digits` which is an offset)
                let token_trimmed = token
                    .trim_end_matches([',', ';', '!', '?', ')', ']', '}'])
                    .to_string();

                // Check for `:N` offset suffix
                let (path_clean, offset) = if let Some(colon_pos) = token_trimmed.rfind(':') {
                    let after_colon = &token_trimmed[colon_pos + 1..];
                    if after_colon.chars().all(|c| c.is_ascii_digit()) && !after_colon.is_empty() {
                        // `:N` offset suffix found
                        let path_part = &token_trimmed[..colon_pos];
                        let offset_val: usize = after_colon.parse().unwrap_or(0);
                        (path_part.to_string(), offset_val)
                    } else {
                        // Bare `:` or non-digit suffix — strip it like other punctuation
                        (token_trimmed.trim_end_matches(':').to_string(), 0)
                    }
                } else {
                    (token_trimmed.clone(), 0)
                };

                if !path_clean.is_empty() {
                    let byte_start = chars[..at_pos].iter().collect::<String>().len();
                    // Span covers @token_trimmed so trailing punctuation remains in output
                    let byte_end = byte_start + 1 + token_trimmed.len(); // +1 for '@'

                    refs.push(FileRef {
                        span: (byte_start, byte_end),
                        path: path_clean,
                        offset,
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

/// Reads a file with optional caching and formats its content as a fenced code block.
/// Starts reading from `offset` (0-indexed line number) and truncates if the
/// remaining content exceeds the configured line/byte limits.
fn format_file_content_with_cache(
    path: &str,
    offset: usize,
    max_lines: usize,
    max_bytes: usize,
    file_cache: Option<&mut FileCache>,
) -> Result<(String, usize, bool, usize), std::io::Error> {
    let content = if let Some(cache) = file_cache {
        if let Some((cached_content, _)) = cache.read_file(path, 0, usize::MAX) {
            cached_content
        } else {
            std::fs::read_to_string(path)?
        }
    } else {
        std::fs::read_to_string(path)?
    };

    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    // Apply offset: skip the first `offset` lines
    let start = offset.min(total_lines);
    let remaining_lines = &all_lines[start..];

    let mut truncated = false;

    // Truncate by line count
    let mut display_content = if remaining_lines.len() > max_lines {
        truncated = true;
        remaining_lines
            .iter()
            .take(max_lines)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        remaining_lines.join("\n")
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

    let shown_lines = display_content.lines().count();
    let next_offset = start + shown_lines;

    let truncation_notice = if truncated {
        if offset > 0 {
            format!(
                "\n... (showing lines {}–{} of {} total. Use @{}:{} to read the next chunk)",
                start + 1,
                next_offset,
                total_lines,
                path,
                next_offset
            )
        } else {
            format!(
                "\n... (file truncated: showing {} of {} total lines. Use @{}:{} or the file_read tool with offset={} to read the rest)",
                shown_lines, total_lines, path, next_offset, next_offset
            )
        }
    } else if offset > 0 && shown_lines > 0 {
        format!(
            "\n... (showing lines {}–{} of {} total, end of file)",
            start + 1,
            next_offset,
            total_lines
        )
    } else if offset > 0 {
        // Offset beyond file end — no lines to show
        format!(
            "\n... (offset {} is beyond end of file with {} lines)",
            offset, total_lines
        )
    } else {
        String::new()
    };

    let offset_attr = if offset > 0 {
        format!(" offset=\"{}\"", offset)
    } else {
        String::new()
    };

    let formatted = format!(
        "<file path=\"{}\" lines=\"{}\"{}>\n```{}\n{}{}\n```\n</file>",
        path, total_lines, offset_attr, extension, display_content, truncation_notice
    );

    Ok((formatted, total_lines, truncated, shown_lines))
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
    expand_file_refs_with_cache(input, config, None)
}

/// Expands all `@filepath` references with optional file caching.
pub fn expand_file_refs_with_cache(
    input: &str,
    config: &Config,
    mut file_cache: Option<&mut FileCache>,
) -> ExpandResult {
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
        let cache_for_call = file_cache.as_mut().map(|c| c as &mut FileCache);
        let replacement = match format_file_content_with_cache(
            &file_ref.path,
            file_ref.offset,
            max_lines,
            max_bytes,
            cache_for_call,
        ) {
            Ok((content, lines, truncated, shown)) => {
                attachments.push((
                    file_ref.path.clone(),
                    AttachStatus::Attached {
                        lines,
                        truncated,
                        offset: file_ref.offset,
                        shown,
                    },
                ));
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
            AttachStatus::Attached {
                lines,
                truncated,
                offset,
                shown,
            } => {
                if *offset > 0 && *shown == 0 {
                    // Offset beyond file end
                    println!(
                        "  {} {}",
                        "📎".bright_cyan(),
                        format!(
                            "attached: {}:{} (offset beyond end of file with {} lines)",
                            path, offset, lines
                        )
                        .dimmed()
                    );
                } else if *offset > 0 {
                    if *truncated {
                        println!(
                            "  {} {}",
                            "📎".bright_cyan(),
                            format!(
                                "attached: {}:{} (lines {}–{} of {}, truncated)",
                                path,
                                offset,
                                offset + 1,
                                offset + shown,
                                lines
                            )
                            .dimmed()
                        );
                    } else {
                        println!(
                            "  {} {}",
                            "📎".bright_cyan(),
                            format!(
                                "attached: {}:{} (lines {}–{} of {})",
                                path,
                                offset,
                                offset + 1,
                                offset + shown,
                                lines
                            )
                            .dimmed()
                        );
                    }
                } else if *truncated {
                    println!(
                        "  {} {}",
                        "📎".bright_cyan(),
                        format!(
                            "attached: {} ({} of {} lines, truncated)",
                            path, shown, lines
                        )
                        .dimmed()
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
