use std::mem::MaybeUninit;

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

/// Enter alternate screen and enable raw mode
///
/// Instead of crossterm's `EnableMouseCapture` (which enables `?1003h` any-event
/// tracking and blocks native terminal text selection), we manually enable only
/// `?1000h` (basic button tracking) + `?1006h` (SGR extended coordinates).
///
/// This allows:
/// - Mouse scroll wheel events for scrolling chat history
/// - Shift+drag for native terminal text selection & copy in most terminals
pub fn enter_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Basic mouse tracking (button press/release + scroll) with SGR encoding.
    // Intentionally NOT enabling ?1002h (button-event motion) or ?1003h (any-event motion)
    // so that terminals can handle Shift+drag for native text selection.
    let _ = write!(stdout, "\x1b[?1000h\x1b[?1006h");
    // Bracketed paste mode
    let _ = write!(stdout, "\x1b[?2004h");
    let _ = stdout.flush();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Query the terminal's default background color via OSC 11.
///
/// Cooked mode buffers tty input until a newline — OSC 11 responses end with
/// `ST` (`\x1b\\`), so they'd never reach our `read()`.  We temporarily
/// switch stdin to raw mode, poll for up to ~500 ms, read the response (if
/// any), then restore the original termios.  Any leftover bytes are flushed
/// with a final drain so they won't leak into the TUI later.
pub fn query_terminal_bg_color() -> Option<(u8, u8, u8)> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    use std::time::{Duration, Instant};

    let fd = std::io::stdin().as_raw_fd();

    // ── Save termios & set raw mode ────────────────────────────────────
    let mut orig_termios = MaybeUninit::<libc::termios>::uninit();
    // SAFETY: tcgetattr is safe; we check the return value.
    if unsafe { libc::tcgetattr(fd, orig_termios.as_mut_ptr()) } != 0 {
        return None;
    }
    let orig_termios = unsafe { orig_termios.assume_init() };

    let mut raw = orig_termios;
    // cfmakeraw sets: ~ECHO | ~ICANON | ~ISIG | ~IEXTEN on c_lflag,
    // ~OPOST on c_oflag, and a few iflag/cflag bits.  We do it manually
    // to avoid needing &mut on `raw` for cfmakeraw's C ABI.
    raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
    raw.c_oflag &= !libc::OPOST;
    raw.c_cflag |= libc::CS8;
    raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;
    // SAFETY: tcsetattr with TCSAFLUSH discards pending unread data.
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
        return None;
    }
    // Drop the raw reference so we can use orig_termios again later.
    let _ = raw;

    // ── Send OSC 11 query ──────────────────────────────────────────────
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "\x1b]11;?\x1b\\");
    let _ = stdout.flush();

    let mut buf = [0u8; 256];
    let mut response = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(500);

    // ── Read loop ──────────────────────────────────────────────────────
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ret = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().min(100) as i32) };
        if ret <= 0 {
            continue;
        }

        if (pollfd.revents & libc::POLLIN) != 0 {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast::<libc::c_void>(), buf.len()) };
            if n <= 0 {
                break;
            }
            response.extend_from_slice(&buf[..n as usize]);
            if response.ends_with(b"\x1b\\") || response.ends_with(b"\x07") {
                break;
            }
        }
    }

    // ── Drain pass ─────────────────────────────────────────────────────
    let drain_end = Instant::now() + Duration::from_millis(100);
    loop {
        if Instant::now() >= drain_end {
            break;
        }
        let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ret = unsafe { libc::poll(&mut pollfd, 1, 30) };
        if ret <= 0 {
            break;
        }
        if (pollfd.revents & libc::POLLIN) != 0 {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast::<libc::c_void>(), buf.len()) };
            if n <= 0 {
                break;
            }
            response.extend_from_slice(&buf[..n as usize]);
            if response.ends_with(b"\x1b\\") || response.ends_with(b"\x07") {
                break;
            }
        }
    }

    // ── Restore original termios ───────────────────────────────────────
    unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &orig_termios) };

    parse_osc_11_response(&response)
}

/// Parse an OSC 11 response into an 8‑bit RGB triple.
///
/// Expected format: `ESC ] 11 ; rgb:RRRR/GGGG/BBBB ST`
/// Where `RRRR` / `GGGG` / `BBBB` are 16‑bit hex values.
/// Some terminals also send `rgba:RRRR/GGGG/BBBB/AAAA`.
fn parse_osc_11_response(raw: &[u8]) -> Option<(u8, u8, u8)> {
    let s = std::str::from_utf8(raw).ok()?;
    let color_start = s.find("rgb:")?;
    let color_part = &s[color_start + 4..];

    let parts: Vec<&str> = color_part.split('/').collect();
    if parts.len() < 3 {
        return None;
    }

    let r = u16::from_str_radix(parts[0], 16).ok()?;
    let g = u16::from_str_radix(parts[1], 16).ok()?;
    let b = u16::from_str_radix(parts[2], 16).ok()?;

    Some(((r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8))
}

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    // Disable mouse tracking and bracketed paste
    // Reset cursor color to default (OSC 112) and ensure cursor is visible
    let _ = write!(std::io::stdout(), "\x1b]112\x1b\\");
    let _ = write!(std::io::stdout(), "\x1b[?1000l\x1b[?1006l");
    let _ = write!(std::io::stdout(), "\x1b[?2004l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), Show)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
