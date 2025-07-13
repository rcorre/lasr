use std::{io::Cursor, path::Path};

use anyhow::Result;
use ratatui::text::{Line, Span};
use syntect::{
    easy::HighlightLines,
    highlighting::{self, Theme, ThemeSet},
    parsing::SyntaxSet,
};

const ANSI_THEME: &[u8] = include_bytes!("ansi.tmTheme");

pub struct Highlighter {
    syntax: SyntaxSet,
    theme: Theme,
}

impl Default for Highlighter {
    fn default() -> Self {
        let mut theme_cursor = Cursor::new(ANSI_THEME);
        Self {
            syntax: SyntaxSet::load_defaults_newlines(),
            theme: ThemeSet::load_from_reader(&mut theme_cursor).expect("Loading theme"),
        }
    }
}

impl Highlighter {
    pub fn highlight(&self, path: &Path, line: &str) -> Result<Line<'static>> {
        let syntax = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| self.syntax.find_syntax_by_extension(ext))
            .unwrap_or_else(|| self.syntax.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, &self.theme);
        let line = h.highlight_line(line, &self.syntax)?;
        Ok(to_line_widget(line))
    }
}

// Borrowed from https://github.com/sxyazi/yazi/pull/460/files
fn to_ansi_color(color: highlighting::Color) -> Option<ratatui::style::Color> {
    if color.a == 0 {
        // Themes can specify one of the user-configurable terminal colors by
        // encoding them as #RRGGBBAA with AA set to 00 (transparent) and RR set
        // to the 8-bit color palette number. The built-in themes ansi, base16,
        // and base16-256 use this.
        Some(match color.r {
            // For the first 8 colors, use the Color enum to produce ANSI escape
            // sequences using codes 30-37 (foreground) and 40-47 (background).
            // For example, red foreground is \x1b[31m. This works on terminals
            // without 256-color support.
            0x00 => ratatui::style::Color::Black,
            0x01 => ratatui::style::Color::Red,
            0x02 => ratatui::style::Color::Green,
            0x03 => ratatui::style::Color::Yellow,
            0x04 => ratatui::style::Color::Blue,
            0x05 => ratatui::style::Color::Magenta,
            0x06 => ratatui::style::Color::Cyan,
            0x07 => ratatui::style::Color::White,
            // For all other colors, use Fixed to produce escape sequences using
            // codes 38;5 (foreground) and 48;5 (background). For example,
            // bright red foreground is \x1b[38;5;9m. This only works on
            // terminals with 256-color support.
            //
            // TODO: When ansi_term adds support for bright variants using codes
            // 90-97 (foreground) and 100-107 (background), we should use those
            // for values 0x08 to 0x0f and only use Fixed for 0x10 to 0xff.
            n => ratatui::style::Color::Indexed(n),
        })
    } else if color.a == 1 {
        // Themes can specify the terminal's default foreground/background color
        // (i.e. no escape sequence) using the encoding #RRGGBBAA with AA set to
        // 01. The built-in theme ansi uses this.
        None
    } else {
        Some(ratatui::style::Color::Rgb(color.r, color.g, color.b))
    }
}

// Convert syntect highlighting to ANSI terminal colors
// See https://github.com/trishume/syntect/issues/309
// Borrowed from https://github.com/sxyazi/yazi/pull/460/files
fn to_line_widget(regions: Vec<(highlighting::Style, &str)>) -> Line<'static> {
    let mut line = Line::default();
    for (style, s) in regions {
        let mut modifier = ratatui::style::Modifier::empty();
        if style.font_style.contains(highlighting::FontStyle::BOLD) {
            modifier |= ratatui::style::Modifier::BOLD;
        }
        if style.font_style.contains(highlighting::FontStyle::ITALIC) {
            modifier |= ratatui::style::Modifier::ITALIC;
        }
        if style
            .font_style
            .contains(highlighting::FontStyle::UNDERLINE)
        {
            modifier |= ratatui::style::Modifier::UNDERLINED;
        }

        line.push_span(Span {
            content: s.to_string().into(),
            style: ratatui::style::Style {
                fg: to_ansi_color(style.foreground),
                // bg: Self::to_ansi_color(style.background),
                add_modifier: modifier,
                ..Default::default()
            },
        })
    }

    line
}
