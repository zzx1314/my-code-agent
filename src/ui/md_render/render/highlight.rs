//! Syntax highlighting engine for the TUI.
//!
//! Wraps [syntect] with the [two_face] grammar and theme bundles to provide
//! ~250-language syntax highlighting and 32 bundled color themes.  The module
//! owns four process-global singletons:
//!
//! | Singleton | Type | Purpose |
//! |---|---|---|
//! | `SYNTAX_SET` | `OnceLock<SyntaxSet>` | Grammar database, immutable after init |
//! | `THEME` | `OnceLock<RwLock<Theme>>` | Active color theme, swappable at runtime |
//! | `THEME_OVERRIDE` | `OnceLock<Option<String>>` | Persisted user preference (write-once) |
//! | `CODEX_HOME` | `OnceLock<Option<PathBuf>>` | Root for custom `.tmTheme` discovery |
//!
//! **Lifecycle:** call [`set_theme_override`] once at startup (after the final
//! config is resolved) to persist the user preference and seed the `THEME`
//! lock.  After that, [`set_syntax_theme`] and [`current_syntax_theme`] can
//! swap/snapshot the theme for live preview.  All highlighting functions read
//! the theme via `theme_lock()`.
//!
//! **Guardrails:** inputs exceeding 512 KB or 10 000 lines are rejected early
//! (returns `None`) to prevent pathological CPU/memory usage.  Callers must
//! fall back to plain unstyled text.

use ratatui::style::Color as RtColor;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::Color as SyntectColor;
use syntect::highlighting::FontStyle;
use syntect::highlighting::Highlighter;
use syntect::highlighting::Style as SyntectStyle;
use syntect::highlighting::Theme;
use syntect::highlighting::ThemeSet;
use syntect::parsing::Scope;
use syntect::parsing::SyntaxReference;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

// -- Global singletons -------------------------------------------------------

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<RwLock<Theme>> = OnceLock::new();
static THEME_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();
static CODEX_HOME: OnceLock<Option<PathBuf>> = OnceLock::new();

// Syntect/bat encode ANSI palette semantics in alpha:
// `a=0` => indexed ANSI palette via RGB payload, `a=1` => terminal default.
const ANSI_ALPHA_INDEX: u8 = 0x00;
const ANSI_ALPHA_DEFAULT: u8 = 0x01;
const OPAQUE_ALPHA: u8 = 0xFF;

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

// NOTE: We intentionally do NOT emit a runtime diagnostic when an ANSI-family
// theme (ansi, base16, base16-256) lacks the expected alpha-channel marker
// encoding.  If the upstream two_face/syntect theme format changes, the
// `ansi_themes_use_only_ansi_palette_colors` test will catch it at build
// time — long before it reaches users.  A runtime warning would be
// unactionable noise since users can't fix upstream themes.

/// Set the user-configured syntax theme override and codex home path.
///
/// Call this with the **final resolved config** (after onboarding, resume, and
/// fork reloads complete). The first call persists `name` and `codex_home` in
/// `OnceLock`s used by startup/default theme resolution.
///
/// Subsequent calls cannot change the persisted `OnceLock` values, but they
/// still update the runtime theme immediately for live preview flows.
///
/// Returns user-facing warnings for actionable configuration issues, such as
/// unknown/invalid theme names or duplicate override persistence.
pub(crate) fn set_theme_override(
    name: Option<String>,
    codex_home: Option<PathBuf>,
) -> Option<String> {
    let warning = validate_theme_name(name.as_deref(), codex_home.as_deref());
    let override_set_ok = THEME_OVERRIDE.set(name.clone()).is_ok();
    let codex_home_set_ok = CODEX_HOME.set(codex_home.clone()).is_ok();
    if THEME.get().is_some() {
        set_syntax_theme(resolve_theme_with_override(
            name.as_deref(),
            codex_home.as_deref(),
        ));
    }
    if !override_set_ok || !codex_home_set_ok {
        // This should never happen in practice — set_theme_override is only
        // called once at startup.  Keep as a debug breadcrumb in case a second
        // call site is added in the future.
        tracing::debug!("set_theme_override called more than once; OnceLock values unchanged");
    }
    warning
}

/// Check whether a theme name resolves to a bundled theme or a custom
/// `.tmTheme` file.  Returns a user-facing warning when it does not.
pub(crate) fn validate_theme_name(name: Option<&str>, codex_home: Option<&Path>) -> Option<String> {
    let name = name?;
    let custom_theme_path_display = codex_home
        .map(|home| custom_theme_path(name, home).display().to_string())
        .unwrap_or_else(|| format!("$CODEX_HOME/themes/{name}.tmTheme"));
    // Bundled themes always resolve.
    if parse_theme_name(name).is_some() {
        return None;
    }
    // Custom themes must parse successfully; an unreadable/invalid file should
    // still surface a startup warning so users can diagnose configuration issues.
    if let Some(home) = codex_home {
        let custom_path = custom_theme_path(name, home);
        if custom_path.is_file() {
            if load_custom_theme(name, home).is_some() {
                return None;
            }
            return Some(format!(
                "Custom theme \"{name}\" at {custom_theme_path_display} could not \
                 be loaded (invalid .tmTheme format). Falling back to the default theme."
            ));
        }
    }
    Some(format!(
        "Theme \"{name}\" not found. Using the default theme. \
         To use a custom theme, place a .tmTheme file at \
         {custom_theme_path_display}."
    ))
}

/// Map a kebab-case theme name to the corresponding `EmbeddedThemeName`.
fn parse_theme_name(name: &str) -> Option<EmbeddedThemeName> {
    match name {
        "ansi" => Some(EmbeddedThemeName::Ansi),
        "base16" => Some(EmbeddedThemeName::Base16),
        "base16-eighties-dark" => Some(EmbeddedThemeName::Base16EightiesDark),
        "base16-mocha-dark" => Some(EmbeddedThemeName::Base16MochaDark),
        "base16-ocean-dark" => Some(EmbeddedThemeName::Base16OceanDark),
        "base16-ocean-light" => Some(EmbeddedThemeName::Base16OceanLight),
        "base16-256" => Some(EmbeddedThemeName::Base16_256),
        "catppuccin-frappe" => Some(EmbeddedThemeName::CatppuccinFrappe),
        "catppuccin-latte" => Some(EmbeddedThemeName::CatppuccinLatte),
        "catppuccin-macchiato" => Some(EmbeddedThemeName::CatppuccinMacchiato),
        "catppuccin-mocha" => Some(EmbeddedThemeName::CatppuccinMocha),
        "coldark-cold" => Some(EmbeddedThemeName::ColdarkCold),
        "coldark-dark" => Some(EmbeddedThemeName::ColdarkDark),
        "dark-neon" => Some(EmbeddedThemeName::DarkNeon),
        "dracula" => Some(EmbeddedThemeName::Dracula),
        "github" => Some(EmbeddedThemeName::Github),
        "gruvbox-dark" => Some(EmbeddedThemeName::GruvboxDark),
        "gruvbox-light" => Some(EmbeddedThemeName::GruvboxLight),
        "inspired-github" => Some(EmbeddedThemeName::InspiredGithub),
        "1337" => Some(EmbeddedThemeName::Leet),
        "monokai-extended" => Some(EmbeddedThemeName::MonokaiExtended),
        "monokai-extended-bright" => Some(EmbeddedThemeName::MonokaiExtendedBright),
        "monokai-extended-light" => Some(EmbeddedThemeName::MonokaiExtendedLight),
        "monokai-extended-origin" => Some(EmbeddedThemeName::MonokaiExtendedOrigin),
        "nord" => Some(EmbeddedThemeName::Nord),
        "one-half-dark" => Some(EmbeddedThemeName::OneHalfDark),
        "one-half-light" => Some(EmbeddedThemeName::OneHalfLight),
        "solarized-dark" => Some(EmbeddedThemeName::SolarizedDark),
        "solarized-light" => Some(EmbeddedThemeName::SolarizedLight),
        "sublime-snazzy" => Some(EmbeddedThemeName::SublimeSnazzy),
        "two-dark" => Some(EmbeddedThemeName::TwoDark),
        "zenburn" => Some(EmbeddedThemeName::Zenburn),
        _ => None,
    }
}

/// Build the expected path for a custom theme file.
fn custom_theme_path(name: &str, codex_home: &Path) -> PathBuf {
    codex_home.join("themes").join(format!("{name}.tmTheme"))
}

/// Try to load a custom `.tmTheme` file from `{codex_home}/themes/{name}.tmTheme`.
fn load_custom_theme(name: &str, codex_home: &Path) -> Option<Theme> {
    ThemeSet::get_theme(custom_theme_path(name, codex_home)).ok()
}

fn adaptive_default_theme_selection() -> (EmbeddedThemeName, &'static str) {
    match crate::ui::md_render::terminal_palette::default_bg() {
        Some(bg) if crate::ui::md_render::color::is_light(bg) => {
            (EmbeddedThemeName::CatppuccinLatte, "catppuccin-latte")
        }
        _ => (EmbeddedThemeName::CatppuccinMocha, "catppuccin-mocha"),
    }
}

fn adaptive_default_embedded_theme_name() -> EmbeddedThemeName {
    adaptive_default_theme_selection().0
}

/// Return the kebab-case name of the adaptive default syntax theme selected
/// from terminal background lightness.
pub(crate) fn adaptive_default_theme_name() -> &'static str {
    adaptive_default_theme_selection().1
}

/// Build the theme from current override/default-theme settings.
/// Extracted from the old `theme()` init closure so it can be reused.
fn resolve_theme_with_override(name: Option<&str>, codex_home: Option<&Path>) -> Theme {
    let ts = two_face::theme::extra();

    // Honor user-configured theme if valid.
    if let Some(name) = name {
        // 1. Try bundled theme by kebab-case name.
        if let Some(theme_name) = parse_theme_name(name) {
            return ts.get(theme_name).clone();
        }
        // 2. Try loading {CODEX_HOME}/themes/{name}.tmTheme from disk.
        if let Some(home) = codex_home
            && let Some(theme) = load_custom_theme(name, home)
        {
            return theme;
        }
        tracing::debug!("Theme \"{name}\" not recognized; using default theme");
    }

    ts.get(adaptive_default_embedded_theme_name()).clone()
}

/// Build the theme from current override/default-theme settings.
/// Extracted from the old `theme()` init closure so it can be reused.
fn build_default_theme() -> Theme {
    let name = THEME_OVERRIDE.get().and_then(|name| name.as_deref());
    let codex_home = CODEX_HOME
        .get()
        .and_then(|codex_home| codex_home.as_deref());
    resolve_theme_with_override(name, codex_home)
}

fn theme_lock() -> &'static RwLock<Theme> {
    THEME.get_or_init(|| RwLock::new(build_default_theme()))
}

/// Swap the active syntax theme at runtime (for live preview).
pub(crate) fn set_syntax_theme(theme: Theme) {
    let mut guard = match theme_lock().write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = theme;
}

/// Clone the current syntax theme (e.g. to save for cancel-restore).
pub(crate) fn current_syntax_theme() -> Theme {
    match theme_lock().read() {
        Ok(theme) => theme.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Raw RGB background colors extracted from syntax theme diff/markup scopes.
///
/// These are theme-provided colors, not yet adapted for any particular color
/// depth.  [`diff_render`](crate::diff_render) converts them to ratatui
/// `Color` values via `color_from_rgb_for_level` after deciding whether to
/// emit truecolor or quantized ANSI-256.
///
/// Both fields are `None` when the active theme defines no relevant scope
/// backgrounds, in which case the diff renderer falls back to its hardcoded
/// palette.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiffScopeBackgroundRgbs {
    pub inserted: Option<(u8, u8, u8)>,
    pub deleted: Option<(u8, u8, u8)>,
}

/// Query the active syntax theme for diff-scope background colors.
///
/// Prefers `markup.inserted` / `markup.deleted` (the TextMate convention used
/// by most VS Code themes) and falls back to `diff.inserted` / `diff.deleted`
/// (used by some older `.tmTheme` files).
pub(crate) fn diff_scope_background_rgbs() -> DiffScopeBackgroundRgbs {
    let theme = current_syntax_theme();
    diff_scope_background_rgbs_for_theme(&theme)
}

/// Pure extraction helper, separated from the global theme singleton so tests
/// can pass arbitrary themes.
fn diff_scope_background_rgbs_for_theme(theme: &Theme) -> DiffScopeBackgroundRgbs {
    let highlighter = Highlighter::new(theme);
    let inserted = scope_background_rgb(&highlighter, "markup.inserted")
        .or_else(|| scope_background_rgb(&highlighter, "diff.inserted"));
    let deleted = scope_background_rgb(&highlighter, "markup.deleted")
        .or_else(|| scope_background_rgb(&highlighter, "diff.deleted"));
    DiffScopeBackgroundRgbs { inserted, deleted }
}

/// Extract the background color for a single TextMate scope, if defined.
fn scope_background_rgb(highlighter: &Highlighter<'_>, scope_name: &str) -> Option<(u8, u8, u8)> {
    let scope = Scope::new(scope_name).ok()?;
    let bg = highlighter.style_mod_for_stack(&[scope]).background?;
    Some((bg.r, bg.g, bg.b))
}

/// Query the active syntax theme for the first foreground style provided by the
/// supplied TextMate scopes.
pub(crate) fn foreground_style_for_scopes(scope_names: &[&str]) -> Option<Style> {
    let theme = current_syntax_theme();
    foreground_style_for_scopes_with_theme(&theme, scope_names)
}

fn foreground_style_for_scopes_with_theme(theme: &Theme, scope_names: &[&str]) -> Option<Style> {
    let highlighter = Highlighter::new(theme);
    scope_names.iter().find_map(|scope_name| {
        let scope = Scope::new(scope_name).ok()?;
        let fg = highlighter.style_mod_for_stack(&[scope]).foreground?;
        convert_syntect_color(fg).map(|fg| Style::default().fg(fg))
    })
}

/// Return the configured kebab-case theme name when it resolves; otherwise
/// return the adaptive auto-detected default theme name.
///
/// This intentionally reflects persisted configuration/default selection, not
/// transient runtime swaps applied via `set_syntax_theme`.
pub(crate) fn configured_theme_name() -> String {
    // Explicit user override?
    if let Some(Some(name)) = THEME_OVERRIDE.get() {
        if parse_theme_name(name).is_some() {
            return name.clone();
        }
        if let Some(Some(home)) = CODEX_HOME.get()
            && load_custom_theme(name, home).is_some()
        {
            return name.clone();
        }
    }
    adaptive_default_theme_name().to_string()
}

/// Resolve a theme name to a `Theme` (bundled or custom). Returns `None`
/// when the name is unknown and no matching `.tmTheme` file exists.
pub(crate) fn resolve_theme_by_name(name: &str, codex_home: Option<&Path>) -> Option<Theme> {
    let ts = two_face::theme::extra();
    // Bundled theme?
    if let Some(embedded) = parse_theme_name(name) {
        return Some(ts.get(embedded).clone());
    }
    // Custom .tmTheme file?
    if let Some(home) = codex_home
        && let Some(theme) = load_custom_theme(name, home)
    {
        return Some(theme);
    }
    None
}

/// A theme available in the picker, either bundled or loaded from a custom
/// `.tmTheme` file under `{CODEX_HOME}/themes/`.
pub(crate) struct ThemeEntry {
    /// Kebab-case identifier used for config persistence and theme resolution.
    pub name: String,
    /// `true` when this entry was discovered from a `.tmTheme` file on disk
    /// rather than the embedded two-face bundle.
    pub is_custom: bool,
}

/// List all available theme names: bundled themes + custom `.tmTheme` files
/// found in `{codex_home}/themes/`.
pub(crate) fn list_available_themes(codex_home: Option<&Path>) -> Vec<ThemeEntry> {
    let mut entries: Vec<ThemeEntry> = BUILTIN_THEME_NAMES
        .iter()
        .map(|name| ThemeEntry {
            name: name.to_string(),
            is_custom: false,
        })
        .collect();

    // Discover custom themes on disk, deduplicating against builtins.
    if let Some(home) = codex_home {
        let themes_dir = home.join("themes");
        if let Ok(read_dir) = std::fs::read_dir(&themes_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("tmTheme")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    let name = stem.to_string();
                    let is_valid_theme = ThemeSet::get_theme(&path).is_ok();
                    if is_valid_theme && !entries.iter().any(|e| e.name == name) {
                        entries.push(ThemeEntry {
                            name,
                            is_custom: true,
                        });
                    }
                }
            }
        }
    }

    // Keep picker ordering stable across platforms/filesystems while sorting
    // custom and bundled themes together, case-insensitively.
    entries.sort_by_cached_key(|entry| (entry.name.to_ascii_lowercase(), entry.name.clone()));

    entries
}

/// All 32 bundled theme names in kebab-case, ordered alphabetically.
const BUILTIN_THEME_NAMES: &[&str] = &[
    "1337",
    "ansi",
    "base16",
    "base16-256",
    "base16-eighties-dark",
    "base16-mocha-dark",
    "base16-ocean-dark",
    "base16-ocean-light",
    "catppuccin-frappe",
    "catppuccin-latte",
    "catppuccin-macchiato",
    "catppuccin-mocha",
    "coldark-cold",
    "coldark-dark",
    "dark-neon",
    "dracula",
    "github",
    "gruvbox-dark",
    "gruvbox-light",
    "inspired-github",
    "monokai-extended",
    "monokai-extended-bright",
    "monokai-extended-light",
    "monokai-extended-origin",
    "nord",
    "one-half-dark",
    "one-half-light",
    "solarized-dark",
    "solarized-light",
    "sublime-snazzy",
    "two-dark",
    "zenburn",
];

// -- Style conversion (syntect -> ratatui) ------------------------------------

/// Map a low ANSI palette index (0–7) to ratatui's named color variants,
/// falling back to `Indexed(n)` for indices 8–255.
///
/// Named variants are preferred over `Indexed(0)`…`Indexed(7)` because many
/// terminals apply bold/bright treatment differently for named vs indexed
/// colors, and ANSI themes expect the named behavior.
///
/// `clippy::disallowed_methods` is explicitly allowed here because this helper
/// intentionally constructs `ratatui::style::Color::Indexed`.
#[allow(clippy::disallowed_methods)]
fn ansi_palette_color(index: u8) -> RtColor {
    match index {
        0x00 => RtColor::Black,
        0x01 => RtColor::Red,
        0x02 => RtColor::Green,
        0x03 => RtColor::Yellow,
        0x04 => RtColor::Blue,
        0x05 => RtColor::Magenta,
        0x06 => RtColor::Cyan,
        // ANSI code 37 is "white", represented as `Gray` in ratatui.
        0x07 => RtColor::Gray,
        n => RtColor::Indexed(n),
    }
}

/// Decode a syntect foreground `Color` into a ratatui color, respecting the
/// alpha-channel encoding that bat's `ansi`, `base16`, and `base16-256` themes
/// use to signal ANSI palette semantics instead of true RGB.
///
/// Returns `None` when the color signals "use the terminal's default
/// foreground", allowing the caller to omit the foreground attribute entirely.
///
/// Passing a color from a standard RGB theme (alpha 0xFF) returns
/// `Some(Rgb(..))`, so this function is backward-compatible with non-ANSI
/// themes. Unexpected intermediate alpha values are treated as RGB.
///
/// `clippy::disallowed_methods` is explicitly allowed here because this helper
/// intentionally constructs `ratatui::style::Color::Rgb`.
#[allow(clippy::disallowed_methods)]
fn convert_syntect_color(color: SyntectColor) -> Option<RtColor> {
    match color.a {
        // Bat-compatible encoding used by `ansi`, `base16`, and `base16-256`:
        // alpha 0x00 means `r` stores an ANSI palette index, not RGB red.
        ANSI_ALPHA_INDEX => Some(ansi_palette_color(color.r)),
        // alpha 0x01 means "use terminal default foreground/background".
        ANSI_ALPHA_DEFAULT => None,
        OPAQUE_ALPHA => Some(RtColor::Rgb(color.r, color.g, color.b)),
        // Non-ANSI alpha values appear in some bundled themes; treat as plain RGB.
        _ => Some(RtColor::Rgb(color.r, color.g, color.b)),
    }
}

/// Convert a syntect `Style` to a ratatui `Style`.
///
/// Most themes produce RGB colors. The built-in `ansi`/`base16`/`base16-256`
/// themes encode ANSI palette semantics in the alpha channel, matching bat.
fn convert_style(syn_style: SyntectStyle) -> Style {
    let mut rt_style = Style::default();

    if let Some(fg) = convert_syntect_color(syn_style.foreground) {
        rt_style = rt_style.fg(fg);
    }
    // Intentionally skip background to avoid overwriting terminal bg.
    // If background support is added later, decode with `convert_syntect_color`
    // to reuse the same alpha-marker semantics as foreground.

    if syn_style.font_style.contains(FontStyle::BOLD) {
        rt_style.add_modifier |= Modifier::BOLD;
    }
    // Intentionally skip italic — many terminals render it poorly or not at all.
    // Intentionally skip underline — themes like Dracula use underline on type
    // scopes (entity.name.type, support.class) which produces distracting
    // underlines on type/module names in terminal output.

    rt_style
}

// -- Syntax lookup ------------------------------------------------------------

/// Try to find a syntect `SyntaxReference` for the given language identifier.
///
/// two-face's extended syntax set (~250 languages) resolves most names and
/// extensions directly.  We only patch the few aliases it cannot handle.
fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();

    // Aliases that two-face does not resolve on its own.
    let patched = match lang {
        "csharp" | "c-sharp" => "c#",
        "golang" => "go",
        "python3" => "python",
        "shell" => "bash",
        _ => lang,
    };

    // Try by token (matches file_extensions case-insensitively).
    if let Some(s) = ss.find_syntax_by_token(patched) {
        return Some(s);
    }
    // Try by exact syntax name (e.g. "Rust", "Python").
    if let Some(s) = ss.find_syntax_by_name(patched) {
        return Some(s);
    }
    // Try case-insensitive name match (e.g. "rust" -> "Rust").
    let lower = patched.to_ascii_lowercase();
    if let Some(s) = ss
        .syntaxes()
        .iter()
        .find(|s| s.name.to_ascii_lowercase() == lower)
    {
        return Some(s);
    }
    // Try raw input as file extension.
    if let Some(s) = ss.find_syntax_by_extension(lang) {
        return Some(s);
    }
    None
}

// -- Guardrail constants ------------------------------------------------------

/// Skip highlighting for inputs larger than 512 KB to avoid excessive memory
/// and CPU usage.  Callers fall back to plain unstyled text.
const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;

/// Skip highlighting for inputs with more than 10,000 lines.
const MAX_HIGHLIGHT_LINES: usize = 10_000;

/// Check whether an input exceeds the safe highlighting limits.
///
/// Callers that highlight content in a loop (e.g. per diff-line) should
/// pre-check the aggregate size with this function and skip highlighting
/// entirely when it returns `true`.
pub(crate) fn exceeds_highlight_limits(total_bytes: usize, total_lines: usize) -> bool {
    total_bytes > MAX_HIGHLIGHT_BYTES || total_lines > MAX_HIGHLIGHT_LINES
}

// -- Core highlighting --------------------------------------------------------

/// Core highlighter that accepts an explicit theme reference.
///
/// This keeps production behavior and test behavior on the same code path:
/// production callers pass the global theme lock, while tests can pass a
/// concrete theme without mutating process-global state.
fn highlight_to_line_spans_with_theme(
    code: &str,
    lang: &str,
    theme: &Theme,
) -> Option<Vec<Vec<Span<'static>>>> {
    // Empty input has nothing to highlight; fall back to the plain text path
    // which correctly produces a single empty Line.
    if code.is_empty() {
        return None;
    }

    // Bail out early for oversized inputs to avoid excessive resource usage.
    // Count actual lines (not newline bytes) to avoid an off-by-one when
    // the input does not end with a newline.
    if code.len() > MAX_HIGHLIGHT_BYTES || code.lines().count() > MAX_HIGHLIGHT_LINES {
        return None;
    }

    let syntax = find_syntax(lang)?;
    let mut h = HighlightLines::new(syntax, theme);
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();

    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, syntax_set()).ok()?;
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (style, text) in ranges {
            // Strip trailing line endings (LF and CR) since we handle line
            // breaks ourselves.  CRLF inputs would otherwise leave a stray \r.
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                continue;
            }
            spans.push(Span::styled(text.to_string(), convert_style(style)));
        }
        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }
        lines.push(spans);
    }

    Some(lines)
}

/// Parse `code` using syntect for `lang` and return per-line styled spans.
/// Each inner Vec represents one source line.  Returns None when the language
/// is not recognized or the input exceeds safety limits.
fn highlight_to_line_spans(code: &str, lang: &str) -> Option<Vec<Vec<Span<'static>>>> {
    let theme_guard = match theme_lock().read() {
        Ok(theme_guard) => theme_guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    highlight_to_line_spans_with_theme(code, lang, &theme_guard)
}

// -- Public API ---------------------------------------------------------------

/// Highlight code in any supported language, returning styled ratatui `Line`s.
///
/// Falls back to plain unstyled text when the language is not recognized or the
/// input exceeds safety guardrails.  Callers can always render the result
/// directly -- the fallback path produces equivalent plain-text lines.
///
/// Used by `markdown_render` for fenced code blocks and by `exec_cell` for bash
/// command highlighting.
pub(crate) fn highlight_code_to_lines(code: &str, lang: &str) -> Vec<Line<'static>> {
    if let Some(line_spans) = highlight_to_line_spans(code, lang) {
        line_spans.into_iter().map(Line::from).collect()
    } else {
        // Fallback: plain text, one Line per source line.
        // Use `lines()` instead of `split('\n')` to avoid a phantom trailing
        // empty element when the input ends with '\n' (as pulldown-cmark emits).
        let mut result: Vec<Line<'static>> =
            code.lines().map(|l| Line::from(l.to_string())).collect();
        if result.is_empty() {
            result.push(Line::from(String::new()));
        }
        result
    }
}

/// Backward-compatible wrapper for bash highlighting used by exec cells.
pub(crate) fn highlight_bash_to_lines(script: &str) -> Vec<Line<'static>> {
    highlight_code_to_lines(script, "bash")
}

/// Highlight code and return per-line styled spans for diff integration.
///
/// Returns `None` when the language is unrecognized or the input exceeds
/// guardrails.  The caller (`diff_render`) uses this signal to fall back to
/// plain diff coloring.
///
/// Each inner `Vec<Span>` corresponds to one source line.  Styles are derived
/// from the active theme but backgrounds are intentionally omitted so the
/// terminal's own background shows through.
pub(crate) fn highlight_code_to_styled_spans(
    code: &str,
    lang: &str,
) -> Option<Vec<Vec<Span<'static>>>> {
    highlight_to_line_spans(code, lang)
}
