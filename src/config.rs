use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Color {
    Name(String),
    Index(u8),
    RGB([u8; 3]),
}

impl TryFrom<Color> for ratatui::style::Color {
    type Error = anyhow::Error;

    fn try_from(value: Color) -> std::result::Result<Self, Self::Error> {
        match value {
            Color::Name(name) => Ok(name
                .parse()
                .with_context(|| format!("Invalid color name: '{name}'"))?),
            Color::Index(i) => Ok(ratatui::style::Color::Indexed(i)),
            Color::RGB([r, g, b]) => Ok(ratatui::style::Color::Rgb(r, g, b)),
        }
    }
}

fn parse_modifiers(mods: Vec<String>) -> Result<ratatui::style::Modifier> {
    let mut res = ratatui::style::Modifier::empty();
    for s in mods {
        res |= match s.to_lowercase().replace("-", "_").as_str() {
            "bold" => ratatui::style::Modifier::BOLD,
            "dim" => ratatui::style::Modifier::DIM,
            "italic" => ratatui::style::Modifier::ITALIC,
            "underlined" => ratatui::style::Modifier::UNDERLINED,
            "slow_blink" => ratatui::style::Modifier::SLOW_BLINK,
            "rapid_blink" => ratatui::style::Modifier::RAPID_BLINK,
            "reversed" => ratatui::style::Modifier::REVERSED,
            "hidden" => ratatui::style::Modifier::HIDDEN,
            "crossed_out" => ratatui::style::Modifier::CROSSED_OUT,
            _ => bail!("Invalid modifier name: '{s}'"),
        }
    }
    Ok(res)
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    mods: Vec<String>,
}

impl TryFrom<Style> for ratatui::style::Style {
    type Error = anyhow::Error;

    fn try_from(value: Style) -> std::result::Result<Self, Self::Error> {
        Ok(ratatui::style::Style {
            fg: match value.fg {
                Some(c) => Some(c.try_into()?),
                None => None,
            },
            bg: match value.bg {
                Some(c) => Some(c.try_into()?),
                None => None,
            },
            add_modifier: parse_modifiers(value.mods)?,
            ..Default::default()
        })
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
struct Theme {
    bg: Color,
    fg: Color,
    find: Style,
    replace: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Index(0),
            fg: Color::Index(7),
            find: Style {
                fg: Some(Color::Index(2)),
                ..Default::default()
            },
            replace: Style {
                fg: Some(Color::Index(1)),
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    theme: Theme,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_valid() {
        let t = toml::toml! {
            [theme]
            bg = "black"
            fg = 6
            find.fg = [24, 48, 96]
            find.mods = ["crossed_out"]
        }
        .to_string();

        let c: Config = toml::from_str(&t).unwrap();
        assert_eq!(
            c,
            Config {
                theme: Theme {
                    bg: Color::Name("black".to_string()),
                    fg: Color::Index(6),
                    find: Style {
                        fg: Some(Color::RGB([24, 48, 96])),
                        mods: vec!["crossed_out".to_string()],
                        ..Default::default()
                    },
                    replace: Style {
                        fg: Some(Color::Index(1)),
                        ..Default::default()
                    },
                }
            }
        )
    }
}
