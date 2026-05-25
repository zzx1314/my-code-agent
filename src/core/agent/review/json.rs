//! JSON processing utilities for code review responses.
//!
//! LLMs frequently produce malformed JSON (truncated, trailing commas,
//! invalid escape sequences, raw control chars in strings). This module
//! provides progressive repair strategies to handle these cases.

/// Extract a JSON object from an LLM response string.
///
/// Handles multiple formats:
/// 1. ```json ... ``` code blocks
/// 2. ``` ... ``` code blocks (looks for JSON-like content inside)
/// 3. Bare `{...}` objects using brace counting (handles nesting)
/// 4. If the entire string is valid JSON, returns it directly
pub fn extract_json_from_response(response: &str) -> anyhow::Result<String> {
    let response = response.trim();

    // Strategy 1: Try ```json ... ``` code block (most common with LLMs)
    if let Some(start) = response.find("```json") {
        let json_start = start + 7;
        if let Some(end) = response[json_start..].find("```") {
            return Ok(response[json_start..json_start + end].trim().to_string());
        }
    }

    // Strategy 2: Try ``` ... ``` code block without language specifier
    if let Some(start) = response.rfind("```") {
        let before = &response[..start];
        // Find matching opening ```
        if let Some(open) = before.rfind("```") {
            let inner = response[open + 3..start].trim();
            // Check if it looks like JSON (starts with { or [)
            if inner.starts_with('{') || inner.starts_with('[') {
                return Ok(inner.to_string());
            }
        }
    }

    // Strategy 3: Find outermost { ... } pair using string-aware brace counting
    // This handles nested braces properly and skips braces inside string literals.
    //
    // NOTE: `find('{')` returns a byte index, so we use `char_indices()` which
    // also returns byte indices (not `.chars().enumerate()` which returns char
    // indices). This is critical for correctness when multi-byte Unicode
    // characters (emoji, CJK, etc.) appear before the first `{`.
    if let Some(start) = response.find('{') {
        let mut depth = 0_i64;
        let mut json_start = None;
        let mut json_end = None;
        let mut in_string = false;
        let mut prev_was_escape = false;
        for (byte_i, ch) in response[start..].char_indices() {
            let i = start + byte_i;
            // Track string boundaries to skip braces inside strings
            if ch == '"' && !prev_was_escape {
                in_string = !in_string;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            if in_string {
                continue;
            }
            match ch {
                '{' => {
                    if depth == 0 {
                        json_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        json_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let (Some(js), Some(je)) = (json_start, json_end) {
            if je > js {
                return Ok(response[js..=je].to_string());
            }
        }
    }

    // Strategy 4: If response is itself valid JSON, return it
    if response.starts_with('{') && response.ends_with('}') {
        if serde_json::from_str::<serde_json::Value>(response).is_ok() {
            return Ok(response.to_string());
        }
    }

    anyhow::bail!(
        "Unable to extract JSON from response. Response was:\n{}",
        response.chars().take(500).collect::<String>()
    )
}

/// Sanitize a JSON string by fixing invalid escape sequences.
///
/// LLMs frequently produce JSON with invalid escapes like `\x`, `\uGGGG`,
/// or unescaped backslashes in Windows paths (`C:\Users\test`).
/// This function converts invalid `\X` → `\\X` so that `serde_json::from_str`
/// can parse the result without error.
///
/// Valid JSON escapes (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`)
/// are left untouched.
pub fn sanitize_json_escapes(json: &str) -> String {
    let mut result = String::with_capacity(json.len() + json.len() / 20);
    let mut chars = json.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }
        if !in_string || ch != '\\' {
            result.push(ch);
            continue;
        }

        // We're inside a string and just saw a backslash — check what follows
        let Some(next) = chars.peek() else {
            // Trailing backslash at end of string — escape it
            result.push_str("\\\\");
            break;
        };

        match next {
            // Valid JSON escape sequences — keep as-is
            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                result.push(ch);
                result.push(chars.next().unwrap());
            }
            // Unicode escape: must be \u followed by exactly 4 hex digits
            'u' => {
                // Peek ahead to check 4 hex digits
                let mut hex_valid = true;
                let mut hex_chars = Vec::new();
                for _ in 0..4 {
                    chars.next(); // consume 'u' on first iteration
                    if let Some(h) = chars.peek() {
                        if h.is_ascii_hexdigit() {
                            hex_chars.push(*h);
                        } else {
                            hex_valid = false;
                            break;
                        }
                    } else {
                        hex_valid = false;
                        break;
                    }
                }
                if hex_valid {
                    // Valid \uXXXX — keep entire sequence
                    result.push('\\');
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                } else {
                    // Invalid unicode escape — treat backslash as literal
                    result.push_str("\\\\");
                    result.push('u');
                    for h in &hex_chars {
                        result.push(*h);
                    }
                }
            }
            // Invalid escape — double the backslash to make it literal
            _ => {
                result.push_str("\\\\");
                result.push(chars.next().unwrap());
            }
        }
    }

    result
}

/// Escape raw control characters inside JSON string values.
///
/// LLMs frequently include multi-line content (code snippets, descriptions)
/// with raw newlines (`\n`, `\r`) or tabs inside JSON string values.
/// These are invalid in JSON and cause `serde_json` parse errors like
/// "expected `,` or `}` at line X column Y".
///
/// Replaces raw control characters with their JSON escape equivalents:
/// - `\x00-\x1F (except valid JSON whitespace in strings)` → `\uXXXX` or standard escapes
/// - Specifically: `\n` → `\\n`, `\r` → `\\r`, `\t` → `\\t`
///
/// Properly tracks string boundaries (respects escaped quotes).
pub fn escape_control_chars_in_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 32);
    let mut in_string = false;
    let mut prev_was_escape = false;

    for ch in s.chars() {
        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if in_string {
            match ch {
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\x08' => result.push_str("\\b"),
                '\x0C' => result.push_str("\\f"),
                // Other C0 control characters (U+0000-U+001F except the ones above)
                '\x00'..='\x07' | '\x0B' | '\x0E'..='\x1F' | '\x7F' => {
                    // These are extremely unlikely but escape them as \uXXXX just in case
                    let code = ch as u32;
                    result.push_str(&format!("\\u{:04x}", code));
                }
                _ => result.push(ch),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Remove trailing commas before `}` and `]` throughout JSON.
///
/// LLMs frequently output JSON with trailing commas in nested objects/arrays:
/// ```json
/// {"issues": [{"file": "x.rs", "line": 42,}], ...}
///                                   ^^ trailing comma
/// ```
///
/// `serde_json` rejects these by default. This function removes ALL trailing
/// commas throughout the JSON by scanning for `,` followed by optional
/// whitespace and then `}` or `]`, respecting string boundaries.
pub fn remove_trailing_commas_from_json(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if !in_string && ch == ',' {
            // Look ahead past whitespace for } or ]
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\n' || chars[j] == '\r')
            {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                // This is a trailing comma — skip it
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    result
}

/// Attempt to repair a truncated/malformed JSON string from LLM output.
///
/// LLM responses sometimes get cut off mid-JSON (typically hitting output token limits).
/// This function handles the common truncation patterns:
/// 1. Trailing comma before closing bracket (`...,` → `...}`)
/// 2. Unclosed string literals (appends `"`)
/// 3. Unbalanced braces `{}` and brackets `[]` (appends missing closers)
///
/// Uses proper string-aware tracking to avoid misinterpreting
/// braces/brackets inside string values.
pub fn repair_truncated_json(s: &str) -> String {
    let mut result = s.trim_end().to_string();

    if let Some('}' | ']') = result.chars().last() {
        let closer = result.pop().unwrap();
        result = result.trim_end_matches(',').to_string();
        result.push(closer);
    }

    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut stack: Vec<char> = Vec::new();

    for ch in result.chars() {
        if ch == '"' && !prev_was_escape {
            in_string = !in_string;
        }
        prev_was_escape = ch == '\\' && !prev_was_escape;

        if in_string {
            continue;
        }

        match ch {
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if let Some(&top) = stack.last() {
                    if top == ch {
                        stack.pop();
                    }
                }
            }
            _ => {}
        }
    }

    if in_string {
        result.push('"');
    }

    while let Some(closing) = stack.pop() {
        result.push(closing);
    }

    result
}

/// Try to parse a JSON string with progressive repair strategies.
///
/// Instead of deeply nested if-let-Err chains, uses a flat sequential
/// strategy pattern:
/// 1. Direct parse (fast path for already-valid JSON)
/// 2. Truncation repair (unclosed braces, unclosed strings)
/// 3. Trailing comma removal (common LLM output issue)
/// 4. Control character escaping (raw newlines/tabs in strings)
///
/// Each strategy is applied independently and returns early on success.
pub fn parse_json_with_fallback(s: &str) -> std::result::Result<serde_json::Value, String> {
    // Strategy 1: Direct parse (fast path — most responses are valid)
    if let Ok(v) = serde_json::from_str(s) {
        return Ok(v);
    }

    // Strategy 2: Repair truncation (unclosed braces, unclosed strings)
    let repaired = repair_truncated_json(s);
    if let Ok(v) = serde_json::from_str(&repaired) {
        return Ok(v);
    }

    // Strategy 3: Remove trailing commas throughout (common LLM artifact)
    let no_trailing = remove_trailing_commas_from_json(&repaired);
    if let Ok(v) = serde_json::from_str(&no_trailing) {
        return Ok(v);
    }

    // Strategy 4: Escape raw control characters in string values
    let escaped = escape_control_chars_in_strings(&no_trailing);
    serde_json::from_str(&escaped).map_err(|e| format!("all strategies exhausted: {e}"))
}
