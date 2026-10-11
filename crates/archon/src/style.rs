//! Terminal styling: ANSI colours only when stdout is a terminal and
//! `NO_COLOR` is unset, and the width text is wrapped to.

use std::io::IsTerminal as _;

/// Used when the width cannot be measured (pipes, files).
pub const DEFAULT_WIDTH: usize = 100;
/// Narrower terminals still get this many columns rather than one word per line.
const MIN_WIDTH: usize = 40;
/// Wider terminals wrap here anyway: long lines are hard to read.
const MAX_WIDTH: usize = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Dim,
    Bold,
    Cyan,
    Yellow,
    Blue,
    Red,
    Green,
    Magenta,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Self::Dim => "2",
            Self::Bold => "1",
            Self::Cyan => "1;36",
            Self::Yellow => "1;33",
            Self::Blue => "1;34",
            Self::Red => "1;31",
            Self::Green => "1;32",
            Self::Magenta => "1;35",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    color: bool,
    pub hyperlinks: bool,
}

impl Palette {
    pub fn new(color: bool) -> Self {
        Self {
            color,
            hyperlinks: false,
        }
    }

    /// Colour when stdout is a terminal, `NO_COLOR` is unset or empty, and
    /// `TERM` is not `dumb` (<https://no-color.org>).
    pub fn detect() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let dumb = std::env::var_os("TERM").is_some_and(|t| t == "dumb");
        let terminal = std::io::stdout().is_terminal() && !no_color && !dumb;
        let term = std::env::var("TERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let known_terminal = supports_hyperlinks(&term, &program)
            || std::env::var_os("KITTY_WINDOW_ID").is_some()
            || std::env::var_os("WT_SESSION").is_some()
            || std::env::var("VTE_VERSION")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|v| v >= 5000);
        Self {
            color: terminal,
            hyperlinks: terminal && known_terminal,
        }
    }

    pub fn paint(&self, style: Style, text: &str) -> String {
        if !self.color || text.is_empty() {
            return text.to_owned();
        }
        format!("\x1b[{}m{text}\x1b[0m", style.code())
    }
}

fn supports_hyperlinks(term: &str, program: &str) -> bool {
    matches!(
        term,
        "xterm-kitty" | "xterm-ghostty" | "foot" | "foot-extra"
    ) || matches!(program, "iTerm.app" | "WezTerm" | "vscode" | "ghostty")
}

/// The column count to wrap at: the terminal's, else `$COLUMNS`, else
/// [`DEFAULT_WIDTH`], clamped to a readable range.
pub fn terminal_width() -> usize {
    let measured = terminal_size::terminal_size()
        .map(|(width, _)| usize::from(width.0))
        .or_else(|| std::env::var("COLUMNS").ok()?.trim().parse().ok());
    // One column spare so a full line never triggers the terminal's own wrap.
    let width = measured.map_or(DEFAULT_WIDTH, |w: usize| w.saturating_sub(1));
    width.clamp(MIN_WIDTH, MAX_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_palette_never_emits_escapes() {
        let plain = Palette::new(false);
        assert_eq!(plain.paint(Style::Red, "blocker"), "blocker");
        let colored = Palette::new(true);
        assert_eq!(colored.paint(Style::Red, "x"), "\x1b[1;31mx\x1b[0m");
        assert_eq!(colored.paint(Style::Red, ""), "");
    }

    #[test]
    fn hyperlinks_require_a_known_terminal() {
        assert!(supports_hyperlinks("xterm-kitty", ""));
        assert!(supports_hyperlinks("xterm-256color", "WezTerm"));
        assert!(!supports_hyperlinks("dumb", ""));
        assert!(!supports_hyperlinks("xterm", ""));
        assert!(!Palette::new(false).hyperlinks);
    }

    #[test]
    fn width_is_clamped_to_a_readable_range() {
        let width = terminal_width();
        assert!((MIN_WIDTH..=MAX_WIDTH).contains(&width));
    }
}
