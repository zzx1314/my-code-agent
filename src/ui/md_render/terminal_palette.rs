//! Minimal terminal palette helpers for syntax theme selection.

/// Terminal background RGB when known; `None` assumes a dark terminal.
pub fn default_bg() -> Option<(u8, u8, u8)> {
    None
}
