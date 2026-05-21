//! Custom TextArea — editable multi-line text input widget.
//!
//! Modeled after Codex's bottom_pane/textarea.rs but self-contained (no element
//! system, no Vim mode yet). Provides word wrapping with Unicode width awareness,
//! word-based navigation, a kill buffer (Ctrl+K/Ctrl+Y), and ratatui rendering
//! with cursor-line highlighting.
//!
//! ## Compatibility
//!
//! The public API deliberately mirrors the subset of `tui-textarea` that the
//! current codebase uses, so call‑site changes are mechanical.

use std::cell::RefCell;
use std::ops::Range;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Widget},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------------------
// Word‑piece splitting
// ---------------------------------------------------------------------------

const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/? ";

/// Whitespace that can be wrapped at (space, tab).
fn is_wrap_break(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

fn is_word_separator(ch: char) -> bool {
    WORD_SEPARATORS.contains(ch)
}

/// Split `run` into alternating separator / non‑separator pieces.
fn split_word_pieces(run: &str) -> Vec<(usize, &str)> {
    let mut pieces = Vec::new();
    for (segment_start, segment) in run.split_word_bound_indices() {
        let mut piece_start = 0;
        let mut chars = segment.char_indices();
        let Some((_, first_char)) = chars.next() else {
            continue;
        };
        let mut in_separator = is_word_separator(first_char);

        for (idx, ch) in chars {
            let is_separator = is_word_separator(ch);
            if is_separator == in_separator {
                continue;
            }
            pieces.push((segment_start + piece_start, &segment[piece_start..idx]));
            piece_start = idx;
            in_separator = is_separator;
        }
        pieces.push((segment_start + piece_start, &segment[piece_start..]));
    }
    pieces
}

// ---------------------------------------------------------------------------
// Wrapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct WrapCache {
    width: u16,
    /// Byte‑range of each visual line **without** newline characters.
    lines: Vec<Range<usize>>,
}

/// Compute visual line byte‑ranges for `text` given a `max_width` (in display
/// columns).  Wraps at word boundaries (spaces) and force‑breaks at the width
/// limit when a single word is wider than the area.  Explicit `\n` always
/// starts a new line.
pub(crate) fn compute_wrapped_ranges(text: &str, max_width: usize) -> Vec<Range<usize>> {
    if max_width == 0 {
        return vec![0..text.len()];
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut line_start = 0usize;

    while line_start < text.len() {
        let rest = &text[line_start..];
        let mut line_width = 0usize;
        let mut last_space_byte: Option<usize> = None; // absolute byte after the space(s)

        // Forward scan
        let mut line_ended = false;
        for (rel_byte, ch) in rest.char_indices() {
            let abs_byte = line_start + rel_byte;

            if ch == '\n' {
                // Explicit newline – line ends here (newline is not included)
                ranges.push(line_start..abs_byte);
                line_start = abs_byte + 1; // skip the '\n'
                line_ended = true;
                break;
            }

            let cw = ch.width().unwrap_or(1);

            if line_width > 0 && line_width + cw > max_width {
                // Need to wrap.
                if let Some(sp_byte) = last_space_byte {
                    // Wrap at the last whitespace boundary.
                    ranges.push(line_start..sp_byte);
                    line_start = sp_byte; // sp_byte already points past the space
                } else {
                    // Force‑break at this character boundary.
                    ranges.push(line_start..abs_byte);
                    line_start = abs_byte;
                }
                line_ended = true;
                break;
            }

            line_width += cw;

            // Remember break opportunity (space/tab, excluding \n).
            if is_wrap_break(ch) && ch != '\n' {
                last_space_byte = Some(abs_byte + ch.len_utf8());
            }
        }

        if !line_ended {
            // No break triggered – push the remaining text.
            ranges.push(line_start..text.len());
            break;
        }
    }

    if ranges.is_empty() {
        ranges.push(0..0);
    }
    ranges
}



// ---------------------------------------------------------------------------
// TextArea
// ---------------------------------------------------------------------------

/// Editable multi‑line text area with word wrap, a kill buffer, and ratatui
/// rendering support.
#[derive(Debug)]
pub struct TextArea {
    text: String,
    cursor_pos: usize,

    block: Option<Block<'static>>,
    cursor_line_style: Style,
    left_pad: u16,
    top_pad: u16,
    bot_pad: u16,

    wrap_cache: RefCell<Option<WrapCache>>,
    preferred_col: Option<usize>,

    kill_buffer: String,
}

/// Per‑frame state (scroll offset).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextAreaState {
    pub scroll: u16,
}

// ===== Construction & properties =====

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            block: None,
            cursor_line_style: Style::default(),
            left_pad: 2,
            top_pad: 1,
            bot_pad: 1,
            wrap_cache: RefCell::new(None),
            preferred_col: None,
            kill_buffer: String::new(),
        }
    }

    // --- Style / block ---

    pub fn set_block(&mut self, block: Block<'static>) {
        self.block = Some(block);
    }

    pub fn set_cursor_line_style(&mut self, style: Style) {
        self.cursor_line_style = style;
    }

    /// No‑op: retained for compatibility with call sites that previously
    /// called `tui-textarea`'s `set_cursor_style`.  The native terminal
    /// cursor (blinking bar) is our sole cursor indicator.
    pub fn set_cursor_style(&mut self, _style: Style) {}

    // --- Text access ---

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Return lines as owned `Vec<String>` (split by `'\n'`).
    pub fn lines(&self) -> Vec<String> {
        self.text.split('\n').map(|s| s.to_string()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    // --- Cursor ---

    /// Return `(line_index, character_index)` mimicking `tui-textarea`'s
    /// cursor API.
    pub fn cursor(&self) -> (usize, usize) {
        let byte_pos = self.cursor_pos.min(self.text.len());
        let before = &self.text[..byte_pos];
        let line_num = before.matches('\n').count();
        let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let char_col = before[last_newline..].chars().count();
        (line_num, char_col)
    }

    /// Return the byte offset of the cursor.
    pub fn cursor_byte_pos(&self) -> usize {
        self.cursor_pos
    }

    /// Set cursor by byte position.
    pub fn set_cursor(&mut self, byte_pos: usize) {
        self.cursor_pos = byte_pos.min(self.text.len());
        self.cursor_pos = self.clamp_char_boundary(self.cursor_pos);
        self.preferred_col = None;
    }

    /// Jump to `(row, col)` where row is line index, col is character index.
    /// Used in call sites that previously called `tui-textarea`'s
    /// `move_cursor(CursorMove::Jump(row, col))`.
    pub fn move_cursor(&mut self, row: u16, col: u16) {
        let row = row as usize;
        let col = col as usize;
        let lines: Vec<&str> = self.text.split('\n').collect();
        let row = row.min(lines.len().saturating_sub(1));
        let line_start: usize = lines[..row].iter().map(|l| l.len() + 1).sum();
        let line = lines[row];
        let char_target = col.min(line.chars().count());
        let byte_pos = line
            .char_indices()
            .nth(char_target)
            .map(|(i, _)| line_start + i)
            .unwrap_or(line_start + line.len());
        self.cursor_pos = byte_pos;
        self.preferred_col = None;
    }

    // ===== Internal helpers =====

    fn clamp_char_boundary(&self, pos: usize) -> usize {
        if pos >= self.text.len() {
            return self.text.len();
        }
        if self.text.is_char_boundary(pos) {
            pos
        } else {
            self.text
                .char_indices()
                .map(|(i, _)| i)
                .find(|&i| i >= pos)
                .unwrap_or(self.text.len())
        }
    }

    fn beginning_of_line(&self, pos: usize) -> usize {
        let p = pos.min(self.text.len());
        self.text[..p]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    fn end_of_line(&self, pos: usize) -> usize {
        let p = pos.min(self.text.len());
        let rest = &self.text[p..];
        p + rest.find('\n').unwrap_or(rest.len())
    }

    fn current_line_text(&self) -> &str {
        let bol = self.beginning_of_line(self.cursor_pos);
        let eol = self.end_of_line(self.cursor_pos);
        &self.text[bol..eol]
    }

    fn prev_atomic_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        self.text[..pos]
            .grapheme_indices(true)
            .rev()
            .next()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next_atomic_boundary(&self, pos: usize) -> usize {
        if pos >= self.text.len() {
            return self.text.len();
        }
        self.text[pos..]
            .grapheme_indices(true)
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.text.len())
    }

    // ===== Text manipulation =====

    pub fn insert_str(&mut self, s: &str) {
        self.insert_str_at(self.cursor_pos, s);
    }

    pub fn insert_str_at(&mut self, mut pos: usize, s: &str) {
        pos = pos.min(self.text.len());
        pos = self.clamp_char_boundary(pos);
        self.text.insert_str(pos, s);
        self.clear_wrap_cache();
        if pos <= self.cursor_pos {
            self.cursor_pos = self.cursor_pos.wrapping_add(s.len());
        }
        self.preferred_col = None;
    }

    // ===== Kill buffer =====

    fn kill(&mut self, s: &str) {
        self.kill_buffer = s.to_string();
    }

    fn yank(&mut self) {
        let buf = self.kill_buffer.clone();
        if !buf.is_empty() {
            self.insert_str(&buf);
        }
    }

    // ===== Deletion helpers =====

    fn delete_backward(&mut self, n: usize) {
        if self.cursor_pos == 0 || n == 0 {
            return;
        }
        let mut start = self.cursor_pos;
        for _ in 0..n {
            let prev = self.prev_atomic_boundary(start);
            if prev == start {
                break;
            }
            start = prev;
        }
        let _len = self.cursor_pos - start;
        self.text.replace_range(start..self.cursor_pos, "");
        self.cursor_pos = start;
        self.clear_wrap_cache();
        self.preferred_col = None;
    }

    fn delete_forward(&mut self, n: usize) {
        if self.cursor_pos >= self.text.len() || n == 0 {
            return;
        }
        let mut end = self.cursor_pos;
        for _ in 0..n {
            let next = self.next_atomic_boundary(end);
            if next == end {
                break;
            }
            end = next;
        }
        if end > self.cursor_pos {
            self.text
                .replace_range(self.cursor_pos..end, "");
            self.clear_wrap_cache();
            self.preferred_col = None;
        }
    }

    fn delete_backward_word(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let start = self.beginning_of_previous_word();
        let killed = self.text[start..self.cursor_pos].to_string();
        self.kill(&killed);
        self.text.replace_range(start..self.cursor_pos, "");
        self.cursor_pos = start;
        self.clear_wrap_cache();
        self.preferred_col = None;
    }

    fn delete_forward_word(&mut self) {
        if self.cursor_pos >= self.text.len() {
            return;
        }
        let end = self.end_of_next_word();
        if end > self.cursor_pos {
            let killed = self.text[self.cursor_pos..end].to_string();
            self.kill(&killed);
            self.text
                .replace_range(self.cursor_pos..end, "");
            self.clear_wrap_cache();
            self.preferred_col = None;
        }
    }

    fn kill_to_beginning_of_line(&mut self) {
        let bol = self.beginning_of_line(self.cursor_pos);
        if bol < self.cursor_pos {
            let killed = self.text[bol..self.cursor_pos].to_string();
            self.kill(&killed);
            self.text.replace_range(bol..self.cursor_pos, "");
            self.cursor_pos = bol;
            self.clear_wrap_cache();
            self.preferred_col = None;
        }
    }

    fn kill_to_end_of_line(&mut self) {
        let eol = self.end_of_line(self.cursor_pos);
        if eol > self.cursor_pos {
            let killed = self.text[self.cursor_pos..eol].to_string();
            self.kill(&killed);
            self.text
                .replace_range(self.cursor_pos..eol, "");
            self.clear_wrap_cache();
            self.preferred_col = None;
        }
    }

    // ===== Movement =====

    fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.prev_atomic_boundary(self.cursor_pos);
            self.preferred_col = None;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.cursor_pos = self.next_atomic_boundary(self.cursor_pos);
            self.preferred_col = None;
        }
    }

    fn move_cursor_up(&mut self) {
        let cache_lines: Vec<_> = self
            .wrap_cache
            .borrow()
            .as_ref()
            .map(|c| c.lines.clone())
            .unwrap_or_default();
        if !cache_lines.is_empty() {
            let line_idx = cache_lines
                .partition_point(|r| r.start <= self.cursor_pos)
                .saturating_sub(1);
            if line_idx > 0 {
                let target_line = &cache_lines[line_idx - 1];
                let target_col = self
                    .preferred_col
                    .unwrap_or_else(|| self.text[target_line.start..self.cursor_pos].width());
                self.move_to_col_on_line(target_line.start, target_line.end, target_col);
                self.preferred_col = Some(target_col);
                return;
            }
        }
        // Fallback: previous logical line
        let bol = self.beginning_of_line(self.cursor_pos);
        if bol > 0 {
            let prev_bol = self.beginning_of_line(bol - 1);
            let prev_eol = self.end_of_line(prev_bol);
            let target_col = self
                .preferred_col
                .unwrap_or_else(|| self.text[bol..self.cursor_pos].width());
            self.move_to_col_on_line(prev_bol, prev_eol, target_col);
            self.preferred_col = Some(target_col);
        } else {
            self.cursor_pos = 0;
        }
    }

    fn move_cursor_down(&mut self) {
        let cache_lines: Vec<_> = self
            .wrap_cache
            .borrow()
            .as_ref()
            .map(|c| c.lines.clone())
            .unwrap_or_default();
        if !cache_lines.is_empty() {
            let line_idx = cache_lines
                .partition_point(|r| r.start <= self.cursor_pos)
                .saturating_sub(1);
            if line_idx + 1 < cache_lines.len() {
                let target_line = &cache_lines[line_idx + 1];
                let target_col = self.preferred_col.unwrap_or_else(|| {
                    self.text[cache_lines[line_idx].start..self.cursor_pos].width()
                });
                self.move_to_col_on_line(target_line.start, target_line.end, target_col);
                self.preferred_col = Some(target_col);
                return;
            }
        }
        // Fallback: next logical line
        let bol = self.beginning_of_line(self.cursor_pos);
        let eol = self.end_of_line(bol);
        if eol < self.text.len() {
            let next_bol = eol + 1;
            let next_eol = self.end_of_line(next_bol);
            let target_col = self
                .preferred_col
                .unwrap_or_else(|| self.text[bol..self.cursor_pos].width());
            self.move_to_col_on_line(next_bol, next_eol, target_col);
            self.preferred_col = Some(target_col);
        }
    }

    fn move_to_col_on_line(&mut self, line_start: usize, line_end: usize, target_col: usize) {
        let mut width_so_far = 0usize;
        for (i, g) in self.text[line_start..line_end].grapheme_indices(true) {
            width_so_far += g.width();
            if width_so_far > target_col {
                self.cursor_pos = line_start + i;
                return;
            }
        }
        self.cursor_pos = line_end;
    }

    fn move_cursor_to_beginning_of_line(&mut self, move_up_at_bol: bool) {
        let bol = self.beginning_of_line(self.cursor_pos);
        if self.cursor_pos == bol && move_up_at_bol {
            self.move_cursor_up();
            self.cursor_pos = self.beginning_of_line(self.cursor_pos);
        } else {
            // Smart home: first non‑whitespace, then absolute start.
            let line_text = self.current_line_text();
            let indent = line_text
                .chars()
                .take_while(|c| c.is_whitespace())
                .count();
            let indent_end = bol + line_text[..indent].len();
            if self.cursor_pos > indent_end {
                self.cursor_pos = indent_end;
            } else {
                self.cursor_pos = bol;
            }
        }
        self.preferred_col = None;
    }

    fn move_cursor_to_end_of_line(&mut self, move_down_at_eol: bool) {
        let eol = self.end_of_line(self.cursor_pos);
        if self.cursor_pos == eol && move_down_at_eol && eol < self.text.len() {
            let next_bol = eol + 1;
            self.cursor_pos = self.end_of_line(next_bol);
        } else {
            self.cursor_pos = eol;
        }
        self.preferred_col = None;
    }

    // ===== Word navigation =====

    fn beginning_of_previous_word(&self) -> usize {
        let before = &self.text[..self.cursor_pos];
        if before.is_empty() {
            return 0;
        }
        let pieces = split_word_pieces(before);
        if pieces.is_empty() {
            return 0;
        }
        // Find the piece that contains (or is just before) the cursor.
        let mut idx = pieces.len();
        for (i, &(start, piece)) in pieces.iter().enumerate() {
            if start + piece.len() >= self.cursor_pos {
                idx = i;
                break;
            }
        }
        if idx == 0 {
            return pieces[0].0;
        }
        // Unless the piece just before the cursor is already a non-separator,
        // skip separators, then skip one word.
        let mut i = idx;
        if i > 0 && pieces[i.saturating_sub(1)].1.chars().all(|c| is_word_separator(c)) {
            i = i.saturating_sub(1);
        }
        if i > 0 {
            i = i.saturating_sub(1);
        }
        pieces[i].0
    }

    fn end_of_next_word(&self) -> usize {
        let after = &self.text[self.cursor_pos..];
        if after.is_empty() {
            return self.text.len();
        }
        let pieces = split_word_pieces(after);
        for &(start, piece) in &pieces {
            if piece.chars().any(|c| !is_word_separator(c)) {
                return self.cursor_pos + start + piece.len();
            }
        }
        self.text.len()
    }

    // ===== Wrapping =====

    /// Total display width used by borders (2 if block present, 0 otherwise).
    fn border_overhead(&self) -> u16 {
        if self.block.is_some() { 2 } else { 0 }
    }

    fn wrapped_lines(&self, width: u16) -> Vec<Range<usize>> {
        let border_ov = self.border_overhead();
        let content_width = (width.saturating_sub(border_ov)).max(1) as usize;
        if let Some(cache) = self.wrap_cache.borrow().as_ref() {
            if cache.width == width {
                return cache.lines.clone();
            }
        }
        let lines = compute_wrapped_ranges(&self.text, content_width);
        self.wrap_cache
            .replace(Some(WrapCache { width, lines: lines.clone() }));
        lines
    }

    fn clear_wrap_cache(&mut self) {
        self.wrap_cache.replace(None);
    }

    /// Height in terminal rows needed to display the full content at `width`.
    pub fn desired_height(&self, width: u16) -> u16 {
        self.wrapped_lines(width).len() as u16
    }

    // ===== Cursor screen position =====

    /// Screen (x, y) of the cursor given `area`, assuming default scroll=0.
    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.cursor_pos_with_state(area, TextAreaState::default())
    }

    /// Screen (x, y) of the cursor given `area` and scroll state.
    pub fn cursor_pos_with_state(
        &self,
        area: Rect,
        state: TextAreaState,
    ) -> Option<(u16, u16)> {
        let has_block = self.block.is_some();
        let (padding, inner_width) = if has_block {
            (1u16, area.width.saturating_sub(2).max(1))
        } else {
            (self.left_pad, area.width.max(1))
        };
        let lines = self.wrapped_lines(inner_width);
        let line_idx = lines
            .partition_point(|r| r.start <= self.cursor_pos)
            .saturating_sub(1)
            .min(lines.len().saturating_sub(1));
        let this_line = &lines[line_idx];
        let col = self.text[this_line.start..self.cursor_pos].width() as u16;

        let effective_scroll = if (lines.len() as u16) <= area.height {
            0
        } else {
            state
                .scroll
                .min(lines.len().saturating_sub(area.height as usize) as u16)
        };
        let screen_row = line_idx.saturating_sub(effective_scroll as usize) as u16;
        let border_adjust: u16 = if has_block { 2 } else { self.top_pad + self.bot_pad };
        if screen_row >= area.height.saturating_sub(border_adjust) {
            return None;
        }
        let x_offset = padding;
        let y_offset: u16 = if has_block { padding } else { self.top_pad };
        Some((area.x + x_offset + col, area.y + y_offset + screen_row))
    }

    // ===== Input routing =====

    /// Route a key event.  This implements the editing keybindings that the
    /// codebase relied on from `tui-textarea`.
    pub fn input(&mut self, event: KeyEvent) {
        if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        match (event.code, event.modifiers) {
            // Plain Enter is reserved for submission — the caller handles it.
            // Alt+Enter / Shift+Enter → newline
            (KeyCode::Enter, KeyModifiers::ALT)
            | (KeyCode::Enter, KeyModifiers::SHIFT)
            | (KeyCode::Enter, KeyModifiers::CONTROL) => {
                self.insert_str("\n");
            }

            // Backward/forward delete
            (KeyCode::Backspace, KeyModifiers::NONE) => self.delete_backward(1),
            (KeyCode::Delete, KeyModifiers::NONE) => self.delete_forward(1),

            // Arrow navigation
            (KeyCode::Left, KeyModifiers::NONE) => self.move_cursor_left(),
            (KeyCode::Right, KeyModifiers::NONE) => self.move_cursor_right(),
            (KeyCode::Up, KeyModifiers::NONE) => self.move_cursor_up(),
            (KeyCode::Down, KeyModifiers::NONE) => self.move_cursor_down(),

            // Line start / end
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.move_cursor_to_beginning_of_line(true);
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.move_cursor_to_end_of_line(true);
            }

            // Ctrl‑based navigation
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.move_cursor_to_beginning_of_line(true);
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.move_cursor_to_end_of_line(true);
            }

            // Ctrl+Left / Ctrl+Right → word navigation
            (KeyCode::Left, KeyModifiers::CONTROL) => {
                let pos = self.beginning_of_previous_word();
                self.set_cursor(pos);
            }
            (KeyCode::Right, KeyModifiers::CONTROL) => {
                let pos = self.end_of_next_word();
                self.set_cursor(pos);
            }

            // Ctrl+Backspace / Ctrl+Delete / Alt+Backspace → delete word
            (KeyCode::Backspace, KeyModifiers::CONTROL)
            | (KeyCode::Backspace, KeyModifiers::ALT) => {
                self.delete_backward_word();
            }
            (KeyCode::Delete, KeyModifiers::CONTROL)
            | (KeyCode::Delete, KeyModifiers::ALT) => {
                self.delete_forward_word();
            }

            // Kill / yank
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.kill_to_beginning_of_line(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.kill_to_end_of_line(),
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => self.yank(),
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.delete_backward_word(),

            // Printable characters (catch any modifier — IMEs may set non-zero
            // modifiers for CJK / composed characters on some terminals).
            (KeyCode::Char(c), _) => {
                if !c.is_ascii_control() {
                    self.insert_str(&c.to_string());
                }
            }

            _ => {} // unhandled
        }
    }
}


// ===== From impls =====

impl From<&str> for TextArea {
    fn from(s: &str) -> Self {
        let mut ta = Self::new();
        ta.insert_str(s);
        ta
    }
}

// ===== Rendering =====

impl Widget for &TextArea {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let has_block = self.block.is_some();
        let (padding, inner_width) = if has_block {
            (1u16, area.width.saturating_sub(2).max(1))
        } else {
            (self.left_pad, area.width.max(1))
        };
        let w_lines = self.wrapped_lines(inner_width);

        let cursor_idx = w_lines
            .partition_point(|r| r.start <= self.cursor_pos)
            .saturating_sub(1)
            .min(w_lines.len().saturating_sub(1));
        let content_height = w_lines.len() as u16;
        let view_height = if has_block {
            area.height.saturating_sub(2)
        } else {
            area.height.saturating_sub(self.top_pad + self.bot_pad)
        };

        // Compute auto-scroll offset so the cursor stays visible.
        let mut scroll: usize = 0;
        let view_height_usize = view_height as usize;
        let content_height_usize = content_height as usize;
        if content_height_usize > view_height_usize {
            if cursor_idx < scroll {
                scroll = cursor_idx;
            } else if cursor_idx >= scroll + view_height_usize {
                scroll = (cursor_idx - view_height_usize + 1)
                    .min(content_height_usize - view_height_usize);
            }
        }

        // Render the block (borders + title) if present.
        if let Some(block) = &self.block {
            block.clone().render(area, buf);
        }

        // Y offset for text: top_pad (no-block) or 1 (block).
        let y_origin = if has_block { area.y + 1 } else { area.y + self.top_pad };

        // Render text lines.
        for i in 0..view_height {
            let line_idx = scroll + i as usize;
            if line_idx >= w_lines.len() {
                // bg_paragraph already filled the background — nothing more to do.
                break;
            }
            let range = &w_lines[line_idx];
            let text_slice = &self.text[range.start..range.end];

            let y = y_origin + i as u16;
            let base_style = if line_idx == cursor_idx {
                self.cursor_line_style
            } else {
                Style::default()
            };

            // Fill background for the cursor line only (cursor-line highlight).
            // Other lines keep bg_paragraph's background.
            if !has_block && line_idx == cursor_idx {
                for col in area.x..area.x + area.width {
                    if let Some(cell) = buf.cell_mut((col, y)) {
                        cell.set_style(base_style);
                        cell.set_char(' ');
                    }
                }
            }

            let start_col = area.x + padding;
            let max_col = if has_block {
                area.x + area.width - padding
            } else {
                area.x + area.width
            };
            let mut col = start_col;
            let apply_style = line_idx == cursor_idx;
            for ch in text_slice.chars() {
                let cw = ch.width().unwrap_or(1) as u16;
                if col + cw > max_col {
                    break;
                }
                if let Some(cell) = buf.cell_mut((col, y)) {
                    cell.set_char(ch);
                    if apply_style {
                        cell.set_style(base_style);
                    }
                }
                // For wide characters (CJK, emoji), clear the adjacent cells.
                if cw > 1 {
                    for offset in 1..cw {
                        if let Some(next_cell) = buf.cell_mut((col + offset, y)) {
                            next_cell.set_char(' ');
                            if apply_style {
                                next_cell.set_style(base_style);
                            }
                        }
                    }
                }
                col += cw;
            }
        }
    }
}

// ===== Tests (private-internals only) =====

#[cfg(test)]
mod tests {
    use super::*;

    fn ta(text: &str) -> TextArea {
        TextArea::from(text)
    }

    #[test]
    fn delete_backward_basic() {
        let mut t = ta("hello");
        t.set_cursor(5);
        t.delete_backward(2);
        assert_eq!(t.text(), "hel");
    }

    #[test]
    fn delete_forward_basic() {
        let mut t = ta("hello");
        t.set_cursor(0);
        t.delete_forward(2);
        assert_eq!(t.text(), "llo");
    }

    #[test]
    fn wrapped_lines_produces_ranges() {
        let t = ta("hello world here");
        let ranges = compute_wrapped_ranges("hello world here", 6);
        assert!(ranges.len() >= 2);
        assert!(ranges.iter().all(|r| r.start <= r.end));
    }

    #[test]
    fn word_navigation() {
        let mut t = ta("alpha beta gamma");
        t.set_cursor(0);
        let end = t.end_of_next_word();
        assert_eq!(&t.text()[..end], "alpha");
        t.set_cursor(end);
        let end2 = t.end_of_next_word();
        assert_eq!(&t.text()[end..end2], " beta");
    }

    #[test]
    fn kill_and_yank() {
        let mut t = ta("hello world");
        t.set_cursor(5);
        t.kill_to_end_of_line();
        assert_eq!(t.text(), "hello");
        t.set_cursor(5);
        t.yank();
        assert_eq!(t.text(), "hello world");
    }

    #[test]
    fn empty_text_renders_one_line() {
        let t = TextArea::new();
        let ranges = compute_wrapped_ranges("", 10);
        assert_eq!(ranges, vec![0..0]);
    }

    #[test]
    fn cjk_text() {
        let mut t = ta("你好世界");
        let ranges = compute_wrapped_ranges("你好世界", 4);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&t.text()[ranges[0].start..ranges[0].end], "你好");
        assert_eq!(&t.text()[ranges[1].start..ranges[1].end], "世界");
    }

    #[test]
    fn cjk_cursor_movement() {
        let mut t = ta("你好世界");
        t.set_cursor(0);
        t.move_cursor_right();
        assert_eq!(t.cursor_byte_pos(), 3, "你好 → 你(3 bytes), cursor at 好");

        t.move_cursor_left();
        assert_eq!(t.cursor_byte_pos(), 0);

        t.move_cursor_to_end_of_line(false);
        assert!(t.text()[t.cursor_byte_pos()..].is_empty());
    }
}


