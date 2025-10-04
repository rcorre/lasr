use std::{collections::HashMap, fmt::Display, str::FromStr};

use anyhow::{Context, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Noop,
    Exit,
    Confirm,
    ToggleSearchReplace,
    ToggleIgnoreCase,
    ToggleMultiLine,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteChar,
    DeleteCharBackward,
    DeleteWord,
    DeleteToEndOfLine,
    DeleteLine,
    ScrollDown,
    ScrollUp,
    ScrollTop,
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct Key {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[cfg(test)]
impl Key {
    fn new(code: KeyCode, modifiers: KeyModifiers) -> Key {
        Key { code, modifiers }
    }

    fn char(c: char, modifiers: KeyModifiers) -> Key {
        Key::new(KeyCode::Char(c), modifiers)
    }
}

impl From<KeyEvent> for Key {
    fn from(value: KeyEvent) -> Self {
        Self {
            code: value.code,
            modifiers: value.modifiers,
        }
    }
}

impl TryFrom<String> for Key {
    type Error = anyhow::Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let s = s.to_lowercase();
        let (modifiers, code) = match s.rsplit_once("-") {
            Some((mod_str, code)) => {
                let mut modifiers = KeyModifiers::empty();
                for m in mod_str.split('-') {
                    modifiers |= match m {
                        "c" => KeyModifiers::CONTROL,
                        "a" => KeyModifiers::ALT,
                        _ => {
                            bail!("Unknown modifier '{m}'");
                        }
                    }
                }
                (modifiers, code)
            }
            None => (KeyModifiers::empty(), s.as_str()),
        };
        let code = match code {
            "backspace" => KeyCode::Backspace,
            "enter" => KeyCode::Enter,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "esc" => KeyCode::Esc,
            _ => {
                if code.len() == 1 && code.is_ascii() {
                    KeyCode::Char(code.chars().next().unwrap())
                } else if let Some(f) = s.strip_prefix("f") {
                    // Fn-key
                    let f: u8 = f
                        .parse()
                        .with_context(|| format!("'{code}' is not a valid F-key"))?;
                    KeyCode::F(f)
                } else {
                    bail!("Unknown key code '{code}'");
                }
            }
        };

        Ok(Key { code, modifiers })
    }
}

#[test]
fn test_key_from_string() {
    assert_eq!(
        Key::char('x', KeyModifiers::empty()),
        "x".to_string().try_into().unwrap(),
    );

    assert_eq!(
        Key::char('y', KeyModifiers::CONTROL),
        "c-y".to_string().try_into().unwrap(),
    );

    assert_eq!(
        Key::char('z', KeyModifiers::ALT),
        "a-z".to_string().try_into().unwrap(),
    );

    assert_eq!(
        Key::char('a', KeyModifiers::CONTROL | KeyModifiers::ALT),
        "c-a-a".to_string().try_into().unwrap(),
    );

    assert!(Key::try_from("x-a-a".to_string()).is_err());
}

impl From<Key> for String {
    fn from(val: Key) -> Self {
        let mut s = String::new();
        if val.modifiers.contains(KeyModifiers::CONTROL) {
            s += "c-";
        }
        if val.modifiers.contains(KeyModifiers::ALT) {
            s += "a-";
        }
        match val.code {
            KeyCode::Backspace => s += "backspace",
            KeyCode::Enter => s += "enter",
            KeyCode::Left => s += "left",
            KeyCode::Right => s += "right",
            KeyCode::Up => s += "up",
            KeyCode::Down => s += "down",
            KeyCode::Home => s += "home",
            KeyCode::End => s += "end",
            KeyCode::PageUp => s += "pageup",
            KeyCode::PageDown => s += "pagedown",
            KeyCode::Tab => s += "tab",
            KeyCode::BackTab => s += "backtab",
            KeyCode::Delete => s += "delete",
            KeyCode::Insert => s += "insert",
            KeyCode::Esc => s += "esc",
            KeyCode::F(f) => s += format!("f{f}").as_str(),
            KeyCode::Char(c) => s.push(c.to_ascii_lowercase()),
            _ => unimplemented!(),
        };

        s
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from(*self))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct KeyMap(HashMap<Key, Action>);

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
                fg: Some(Color::Reset),
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

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub theme: Theme,
    pub keys: HashMap<Key, Action>,
    pub auto_pairs: bool,
    pub threads: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            keys: [
                ("enter", Action::Confirm),
                ("esc", Action::Exit),
                ("c-c", Action::Exit),
                ("tab", Action::ToggleSearchReplace),
                ("c-s", Action::ToggleIgnoreCase),
                ("c-l", Action::ToggleMultiLine),
                ("left", Action::CursorLeft),
                ("c-b", Action::CursorLeft),
                ("right", Action::CursorRight),
                ("c-f", Action::CursorRight),
                ("home", Action::CursorHome),
                ("c-a", Action::CursorHome),
                ("end", Action::CursorEnd),
                ("c-e", Action::CursorEnd),
                ("backspace", Action::DeleteCharBackward),
                ("c-h", Action::DeleteCharBackward),
                ("c-d", Action::DeleteChar),
                ("c-w", Action::DeleteWord),
                ("c-k", Action::DeleteToEndOfLine),
                ("c-u", Action::DeleteLine),
                ("c-n", Action::ScrollDown),
                ("c-p", Action::ScrollUp),
                ("c-g", Action::ScrollTop),
            ]
            .map(|(k, v)| (k.to_string().try_into().unwrap(), v))
            .into(),
            auto_pairs: true,
            threads: 0,
        }
    }
}

impl FromStr for Config {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut c: Config = toml::from_str(s)?;
        let base = Self::default();
        // merge in any keys that the user didn't override
        for (k, v) in base.keys {
            c.keys.entry(k).or_insert(v);
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;

    #[test]
    fn test_config_valid() {
        let t = toml::toml! {
            auto_pairs = false

            [theme]
            base.fg = "6"
            find.fg = "#00FF00"
            find.add_modifier = "BOLD"

            [keys]
            c-x = "exit"
        }
        .to_string();

        let c: Config = t.parse().unwrap();
        let mut keys = Config::default().keys;
        keys.insert(
            Key {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::CONTROL,
            },
            Action::Exit,
        );

        assert_eq!(
            c,
            Config {
                keys,
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
                },
                auto_pairs: false,
                threads: 0,
            }
        )
    }
}
