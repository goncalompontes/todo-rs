//! The single styling/colour facility for the whole toolchain.
//!
//! Built on `anstyle` so the host and every plugin share one palette and one
//! colour-choice policy. Colours are disabled by config (`plain`), by the
//! `NO_COLOR` convention, on a dumb terminal, or when stdout is not a TTY.

use std::io::IsTerminal;

pub use anstyle::Style;
use anstyle::{AnsiColor, Color};

/// A colour-enablement policy plus painting helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    enabled: bool,
}

impl Palette {
    pub fn new(enabled: bool) -> Palette {
        Palette { enabled }
    }

    /// Decide whether to colour: `plain` wins, then `NO_COLOR`, `TERM=dumb`,
    /// and whether stdout is a terminal.
    pub fn detect(plain: bool) -> Palette {
        let enabled = !plain
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
            && std::io::stdout().is_terminal();
        Palette { enabled }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Paint `text` with `style`, or return it unchanged when disabled.
    pub fn paint(&self, style: Style, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.enabled {
            format!("{}{}{}", style.render(), text, style.render_reset())
        } else {
            text.to_string()
        }
    }

    /// The raw prefix for `style`, or `""` when disabled (for callers that
    /// build strings incrementally).
    pub fn prefix(&self, style: Style) -> String {
        if self.enabled {
            style.render().to_string()
        } else {
            String::new()
        }
    }

    pub fn reset(&self) -> &'static str {
        if self.enabled { "\x1b[0m" } else { "" }
    }

    // Convenience prefixes for callers that build strings incrementally.
    pub fn bold(&self) -> String {
        self.prefix(bold())
    }
    pub fn dim(&self) -> String {
        self.prefix(dim())
    }
    pub fn red(&self) -> String {
        self.prefix(red())
    }
    pub fn green(&self) -> String {
        self.prefix(green())
    }
    pub fn yellow(&self) -> String {
        self.prefix(yellow())
    }
    pub fn blue(&self) -> String {
        self.prefix(blue())
    }
    pub fn magenta(&self) -> String {
        self.prefix(magenta())
    }
    pub fn cyan(&self) -> String {
        self.prefix(cyan())
    }
    /// Alias for [`Palette::reset`] returning an owned string.
    pub fn rst(&self) -> String {
        self.reset().to_string()
    }

    /// Paint `text` with `style` (alias of [`Palette::paint`]).
    pub fn wrap(&self, style: Style, text: impl AsRef<str>) -> String {
        self.paint(style, text)
    }
}

pub fn bold() -> Style {
    Style::new().bold()
}
pub fn dim() -> Style {
    Style::new().dimmed()
}
pub fn red() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
}
pub fn green() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)))
}
pub fn yellow() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
}
pub fn blue() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue)))
}
pub fn magenta() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta)))
}
pub fn cyan() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
}

/// Colour for a task priority letter, matching todo.sh's default map.
pub fn priority(p: char) -> Style {
    match p {
        'A' => yellow(),
        'B' => green(),
        'C' => blue(),
        _ => Style::new(),
    }
}

/// Style for a `+project` / `@context` / `%group` token.
pub fn sigil(token: &str) -> Style {
    match token.chars().next() {
        Some('+') => blue(),
        Some('@') => magenta(),
        Some('%') => cyan(),
        _ => Style::new(),
    }
}
