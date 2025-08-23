use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Theme {
    pub base: Style,
    pub find: Style,
    pub replace: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            base: Style {
                fg: Some(Color::White),
                ..Default::default()
            },
            find: Style {
                fg: Some(Color::Red),
                add_modifier: Modifier::CROSSED_OUT,
                ..Default::default()
            },
            replace: Style {
                fg: Some(Color::Green),
                add_modifier: Modifier::BOLD,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub theme: Theme,
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;

    #[test]
    fn test_config_valid() {
        let t = toml::toml! {
            [theme]
            base.fg = "6"
            find.fg = "#00FF00"
            find.add_modifier = "BOLD"
        }
        .to_string();

        let c: Config = toml::from_str(&t).unwrap();
        assert_eq!(
            c,
            Config {
                theme: Theme {
                    base: Style {
                        fg: Some(Color::Indexed(6)),
                        ..Default::default()
                    },
                    find: Style {
                        fg: Some(Color::Rgb(0, 255, 0)),
                        add_modifier: Modifier::BOLD,
                        ..Default::default()
                    },
                    replace: Style {
                        fg: Some(Color::Green),
                        add_modifier: Modifier::BOLD,
                        ..Default::default()
                    },
                }
            }
        )
    }
}
