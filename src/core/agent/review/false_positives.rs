//! False positive filtering for code review issues.
//!
//! Before sending issues to `build_report`, this module checks each issue
//! against a set of regex-like patterns that match common false-positive
//! signals from the review LLM.

use crate::core::types::review::{ReviewIssue, Severity};

/// Filter out known false-positive review issues using deterministic rules.
///
/// Patterns are grouped by category:
///
/// 1. **`block_in_place` / async panic** — LLMs see `tokio::task::block_in_place`
///    and claim it will panic, but this is a valid tokio utility.
/// 2. **`block_on` convention** — `futures::executor::block_on` is used deliberately
///    throughout this project; it's not a bug.
/// 3. **"Silent fallback" / "silently"** — LLM flags intentional fallback/default
///    patterns as error-masking, even when the fallback is correct.
/// 4. **Generic "consider error handling"** — Vague, non-actionable suggestions
///    without a specific scenario.
/// 5. **"Missing documentation" / "consider documenting"** — Low-value doc suggestions
///    for internal or self-explanatory code.
/// 6. **"Hardcoded" values** — Test fixtures, config defaults, or intentional
///    constants flagged as problematic.
/// 7. **`unwrap()` on safe operations** — When unwrap is provably safe (e.g., on
///    a `Receiver::try_recv` or a freshly-created value).
///
/// Each rule uses simple string matching (case-insensitive) on the issue's
/// combined title + description + file path. This is intentionally cheap
/// and deterministic — no regex overhead, no LLM calls.
pub fn filter_known_false_positives(issues: &[ReviewIssue]) -> Vec<ReviewIssue> {
    issues
        .iter()
        .filter(|issue| !is_known_false_positive(issue))
        .cloned()
        .collect()
}

/// Check a single issue against all known false-positive patterns.
fn is_known_false_positive(issue: &ReviewIssue) -> bool {
    // Build a combined text for pattern matching
    let title_lower = issue.title.to_lowercase();
    let desc_lower = issue.description.to_lowercase();
    let file_lower = issue.file.to_lowercase();

    // ── Rule 1: block_in_place async panic ──
    // LLMs frequently see `tokio::task::block_in_place` and claim it will
    // cause a panic in async context. In reality, `block_in_place` is a
    // valid tokio utility for running blocking code without blocking the
    // async runtime. The LLM confuses it with `block_on` in async context.
    if desc_lower.contains("block_in_place") || title_lower.contains("block_in_place") {
        if desc_lower.contains("async")
            || desc_lower.contains("panic")
            || desc_lower.contains("blocking")
        {
            return true;
        }
    }

    // ── Rule 2: futures::executor::block_on project convention ──
    // This project deliberately uses `futures::executor::block_on()` to
    // run async code from sync contexts (e.g., startup, tests, drop guards).
    // The LLM, seeing only the diff, flags it as a "blocking in sync context"
    // issue, which is the intended usage.
    if desc_lower.contains("block_on") || title_lower.contains("block_on") {
        if desc_lower.contains("async")
            || desc_lower.contains("blocking")
            || desc_lower.contains(".await")
            || desc_lower.contains("runtime")
        {
            return true;
        }
    }

    // ── Rule 3: "Silent fallback" / "silently swallows" ──
    // LLMs often flag intentional fallback/default patterns as "silently
    // masking errors". The word "silent" in a review context is a reliable
    // false-positive signal when paired with "fallback" or "default".
    let combined = format!("{} {}", desc_lower, title_lower);
    if (combined.contains("silent") || combined.contains("silently"))
        && (combined.contains("fallback")
            || combined.contains("default")
            || combined.contains("swallow"))
    {
        return true;
    }

    // ── Rule 4: Generic "consider error handling" ──
    // Vague suggestions without a specific error scenario. If the issue
    // just says to add error handling but doesn't describe what error
    // could occur or how, it's likely a default LLM template response.
    if (combined.contains("consider") || combined.contains("recommend"))
        && combined.contains("error handling")
        && !combined.contains("specific")
        && !combined.contains("scenario")
    {
        return true;
    }

    // ── Rule 5: "Missing documentation" ──
    // Low-value doc suggestions for internal/trivial code. If the title
    // or description says to add docs but the code is self-explanatory
    // (e.g., a simple getter, a test, or internal helper), it's noise.
    if combined.contains("documentation")
        || combined.contains("document")
        || combined.contains("docstring")
    {
        // Only filter if it's low severity — real doc gaps are worth noting
        if issue.severity == Severity::Low || issue.severity == Severity::Info {
            return true;
        }
    }

    // ── Rule 6: "Hardcoded" values ──
    // LLMs frequently flag string literals, URLs, or numbers as "hardcoded"
    // without considering whether they're test data, config defaults, or
    // intentional constants. Filter if the issue doesn't provide a specific
    // attack scenario or risk.
    if combined.contains("hardcoded") || combined.contains("hard-coded") {
        // Keep if it's about security (hardcoded credentials/API keys)
        if combined.contains("password")
            || combined.contains("secret")
            || combined.contains("credential")
            || combined.contains("api_key")
            || combined.contains("token")
        {
            // Don't filter — this is a real security concern
        } else {
            // Filter "hardcoded" for non-security items
            if issue.severity == Severity::Low || issue.severity == Severity::Medium {
                return true;
            }
        }
    }

    // ── Rule 7: Test file noise ──
    // In test files, certain patterns like "magic number", "unwrap",
    // "missing error handling" are typically intentional. Test code
    // often uses unwrap liberally and doesn't need production-level
    // error handling.
    if file_lower.contains("test")
        || file_lower.ends_with("_test.rs")
        || file_lower.ends_with("_spec.rs")
        || file_lower.ends_with(".test.ts")
        || file_lower.ends_with(".spec.ts")
    {
        if combined.contains("unwrap")
            || combined.contains("magic number")
            || combined.contains("error handling")
        {
            if issue.severity == Severity::Low || issue.severity == Severity::Info {
                return true;
            }
        }
    }

    false
}
