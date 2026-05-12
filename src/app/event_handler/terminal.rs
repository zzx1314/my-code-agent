use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
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

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    // Disable mouse tracking and bracketed paste
    let _ = write!(std::io::stdout(), "\x1b[?1000l\x1b[?1006l");
    let _ = write!(std::io::stdout(), "\x1b[?2004l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
