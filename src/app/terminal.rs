use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        cursor::Show,
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};
use std::io::Write as _;

/// Enter alternate screen, enable raw mode, and enable basic mouse tracking.
///
/// Basic mouse tracking (?1000h) reports button press/release events — including
/// scroll wheel — as `Event::Mouse(MouseEvent)` with `ScrollUp` / `ScrollDown`
/// kinds.  This cleanly separates wheel scrolling (mouse events → chat scroll)
/// from ↑/↓ key presses (key events → history navigation), eliminating the
/// scroll-wheel-induced history flicker that plagues the `?1007h` approach.
///
/// The trade-off is that mouse tracking interferes with the terminal's native
/// click-drag text selection.  The user can press **Alt+S** to temporarily
/// disable mouse tracking (`?1000l`), select text natively, and re-enable
/// it with another Alt+S.
pub fn enter_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Enable bracketed paste + basic mouse tracking
    let _ = write!(stdout, "\x1b[?2004h\x1b[?1000h");
    let _ = stdout.flush();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Enable basic mouse tracking (`?1000h`).
pub fn enable_mouse_tracking() {
    let _ = write!(std::io::stdout(), "\x1b[?1000h");
    let _ = std::io::stdout().flush();
}

/// Disable basic mouse tracking (`?1000l`).
///
/// This allows the terminal's native click-drag text selection to work.
/// Call [`enable_mouse_tracking`] to re-enable after selection is done.
pub fn disable_mouse_tracking() {
    let _ = write!(std::io::stdout(), "\x1b[?1000l");
    let _ = std::io::stdout().flush();
}

/// Query the terminal's default background color via OSC 11.
///
/// **Must be called after crossterm raw mode is enabled** (i.e. after
/// [`enter_terminal`]).  Raw mode disables `ICANON` so the OSC 11 response
/// (which ends with `ST` `\x1b\\`, not a newline) is immediately readable.
///
/// Uses `dup()` + `O_NONBLOCK` on a cloned stdin fd — the original fd 0 is
/// never touched, so crossterm's event stream is not affected.  No termios
/// manipulation: raw mode is already active from [`enter_terminal`].
pub fn query_terminal_bg_color() -> Option<(u8, u8, u8)> {
    use std::os::unix::io::{AsRawFd, FromRawFd};
    use std::time::{Duration, Instant};

    // ── Clone stdin so we can set non-blocking without affecting fd 0 ──
    let stdin_fd = std::io::stdin().as_raw_fd();
    let dup_fd = unsafe { libc::dup(stdin_fd) };
    if dup_fd == -1 {
        return None;
    }
    let _reader = unsafe { std::fs::File::from_raw_fd(dup_fd) };

    // Save original file status flags
    let original_flags = unsafe { libc::fcntl(dup_fd, libc::F_GETFL) };
    if original_flags == -1 {
        return None; // _reader dropped → closes dup_fd
    }
    // Set non-blocking so read_available returns immediately
    if unsafe { libc::fcntl(dup_fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) } == -1 {
        return None;
    }

    // ── Send OSC 11 query ──────────────────────────────────────────────
    let _ = write!(std::io::stdout(), "\x1b]11;?\x1b\\");
    let _ = std::io::stdout().flush();

    let mut buf = [0u8; 256];
    let mut response = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(500);

    // ── Read loop (non-blocking, poll-driven) ──────────────────────────
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        let mut pollfd = libc::pollfd {
            fd: dup_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe {
            libc::poll(&mut pollfd, 1, remaining.as_millis().min(100) as i32)
        };
        if ret <= 0 {
            continue;
        }

        if (pollfd.revents & libc::POLLIN) != 0 {
            let n = unsafe {
                libc::read(
                    dup_fd,
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            response.extend_from_slice(&buf[..n as usize]);
            // ST (ESC \) or BEL — both valid OSC terminators
            if response.ends_with(b"\x1b\\") || response.ends_with(b"\x07") {
                break;
            }
        }
    }

    // ── Drain any trailing bytes ───────────────────────────────────────
    let drain_end = Instant::now() + Duration::from_millis(100);
    loop {
        if Instant::now() >= drain_end {
            break;
        }
        let mut pollfd = libc::pollfd {
            fd: dup_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pollfd, 1, 30) };
        if ret <= 0 {
            break;
        }
        if (pollfd.revents & libc::POLLIN) != 0 {
            let n = unsafe {
                libc::read(
                    dup_fd,
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            response.extend_from_slice(&buf[..n as usize]);
            if response.ends_with(b"\x1b\\") || response.ends_with(b"\x07") {
                break;
            }
        }
    }

    // ── Restore original file status flags, then close dup_fd ──────────
    unsafe { libc::fcntl(dup_fd, libc::F_SETFL, original_flags) };
    // _reader drops here, closing dup_fd

    parse_osc_11_response(&response)
}

/// Parse an OSC 11 response into an 8‑bit RGB triple.
///
/// Expected format from the terminal:
///   `ESC ] 11 ; rgb:RRRR/GGGG/BBBB ST`   — 16‑bit hex components
///   `ESC ] 11 ; rgb:RR/GG/BB ST`         — 8‑bit hex components
///   `ESC ] 11 ; rgba:… ST`               — with alpha (ignored)
///
/// ST may be either `ESC \` (canonical) or BEL (`\x07`).
/// The response is prefixed by `ESC ] 11 ;` and suffixed by the terminator.
///
/// We first locate the `rgb:` / `rgba:` prefix, then find the first valid
/// OSC terminator after it, so trailing junk (including the terminator itself)
/// never reaches the hex parser.
fn parse_osc_11_response(raw: &[u8]) -> Option<(u8, u8, u8)> {
    // Find the start of the colour payload — "rgb:" or "rgba:" anywhere in
    // the buffer (there may be other DSR / probe responses mixed in).
    let s = std::str::from_utf8(raw).ok()?;
    let payload_start = s.find("rgb:").or_else(|| s.find("rgba:"))?;

    // Convert back to byte index (safe: str is valid UTF-8 view of raw)
    let start_byte = payload_start;
    let rest = &raw[start_byte..];

    // Find the OSC terminator (ST = ESC \  or BEL = \x07)
    let term_pos = rest
        .windows(2)
        .position(|w| w == b"\x1b\\")
        .or_else(|| rest.iter().position(|&b| b == 0x07))?;

    // The RGB payload is between "rgb:" and the terminator
    let payload = std::str::from_utf8(&rest[4..term_pos]).ok()?;

    let parts: Vec<&str> = payload.split('/').collect();
    if parts.len() < 3 {
        return None;
    }

    let parse_comp = |s: &str| -> Option<u8> {
        match s.len() {
            2 => u8::from_str_radix(s, 16).ok(),
            // 16-bit: right-shift 8 to get the 8‑bit value
            4 => u16::from_str_radix(s, 16).ok().map(|v| (v >> 8) as u8),
            _ => None,
        }
    };

    let r = parse_comp(parts[0])?;
    let g = parse_comp(parts[1])?;
    let b = parse_comp(parts[2])?;

    Some((r, g, b))
}

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    // Disable bracketed paste and mouse tracking
    // Reset cursor color to default (OSC 112) and ensure cursor is visible
    let _ = write!(std::io::stdout(), "\x1b]112\x1b\\");
    let _ = write!(std::io::stdout(), "\x1b[?2004l\x1b[?1000l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), Show)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
