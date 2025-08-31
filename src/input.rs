use crate::config::{Action, Key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::{
    Frame,
    widgets::{Block, Borders, Paragraph},
};
use std::collections::HashMap;

#[derive(Default)]
pub struct LineInput {
    pattern: String,
    cursor_pos: usize,
}

impl LineInput {
    // Returns true if the pattern changed
    pub fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        key_map: &HashMap<Key, Action>,
    ) -> Option<&str> {
        if let Some(action) = key_map.get(&key_event.into()) {
            match action {
                Action::CursorLeft => {
                    tracing::debug!("Moving cursor left");
                    self.cursor_pos = self.cursor_pos.saturating_sub(1);
                    return None;
                }
                Action::CursorRight => {
                    tracing::debug!("Moving cursor right");
                    self.cursor_pos = (self.cursor_pos + 1).min(self.pattern.len());
                    return None;
                }
                Action::CursorHome => {
                    tracing::debug!("Moving cursor to beginning of line");
                    self.cursor_pos = 0;
                    return None;
                }
                Action::CursorEnd => {
                    tracing::debug!("Moving cursor to end of line");
                    self.cursor_pos = self.pattern.len();
                    return None;
                }
                Action::DeleteChar => {
                    if self.cursor_pos >= self.pattern.len() {
                        return None;
                    }
                    tracing::debug!("Deleting character at cursor position {}", self.cursor_pos);
                    self.pattern.remove(self.cursor_pos);
                    return Some(&self.pattern);
                }
                Action::DeleteCharBackward => {
                    if self.cursor_pos == 0 {
                        return None;
                    };
                    self.cursor_pos -= 1;
                    // BUG: Doesn't handle unicode
                    let c = self.pattern.remove(self.cursor_pos);
                    tracing::debug!("Removed '{c}' from pattern, new pattern: {}", self.pattern);
                    return Some(&self.pattern);
                }
                Action::DeleteWord => {
                    if self.cursor_pos == 0 {
                        return None;
                    };
                    tracing::debug!(
                        "Deleting word from '{}' at {}",
                        self.pattern,
                        self.cursor_pos
                    );
                    let (s, rest) = self.pattern.split_at(self.cursor_pos);
                    if let Some(idx) = s.trim_end().rfind(char::is_whitespace) {
                        self.cursor_pos = idx + 1;
                        self.pattern = s[0..=idx].to_owned() + rest;
                        tracing::debug!("Truncated pattern to {}", self.pattern);
                    } else {
                        self.pattern = rest.into();
                        self.cursor_pos = 0;
                        tracing::debug!("Cleared pattern");
                    }
                    return Some(&self.pattern);
                }
                Action::DeleteToEndOfLine => {
                    if self.cursor_pos >= self.pattern.len() {
                        return None;
                    }
                    tracing::debug!("Deleting from cursor to end of line");
                    self.pattern.truncate(self.cursor_pos);
                    return Some(&self.pattern);
                }
                Action::DeleteLine => {
                    if self.pattern.is_empty() {
                        return None;
                    }
                    tracing::debug!("Deleting entire line");
                    self.pattern.clear();
                    self.cursor_pos = 0;
                    return Some(&self.pattern);
                }
                _ => {} // Ignore other actions
            }
        }

        // Fall back to character input if no action matched
        match key_event.code {
            KeyCode::Char(c) if (key_event.modifiers & !KeyModifiers::SHIFT).is_empty() => {
                self.pattern.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                tracing::debug!("Updated filter pattern: {}", self.pattern);
                Some(&self.pattern)
            }
            _ => None,
        }
    }

    pub fn cursor_pos(&self) -> u16 {
        self.cursor_pos as u16
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn size(&self) -> u16 {
        // +2 for borders
        self.pattern.len() as u16 + 2
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, title: &str, style: Style) {
        let input = Paragraph::new(self.pattern.as_str())
            .block(Block::new().borders(Borders::all()).title(title))
            .style(style);
        frame.render_widget(input, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn input(li: &mut LineInput, s: &str) {
        let config = Config::default();
        for c in s.chars() {
            let result = li
                .handle_key_event(KeyCode::Char(c).into(), &config.keys)
                .unwrap()
                .to_string();
            assert_eq!(result, li.pattern());
        }
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_input() {
        let mut app = LineInput::default();
        let config = Config::default();

        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        input(&mut app, "abc");
        assert_eq!(app.pattern, "abc");
        assert_eq!(app.cursor_pos, 3);

        assert_eq!(
            app.handle_key_event(KeyCode::Backspace.into(), &config.keys),
            Some("ab")
        );
        assert_eq!(app.pattern, "ab");
        assert_eq!(app.cursor_pos, 2);

        assert_eq!(
            app.handle_key_event(KeyCode::Backspace.into(), &config.keys),
            Some("a")
        );
        assert_eq!(app.pattern, "a");
        assert_eq!(app.cursor_pos, 1);

        assert_eq!(
            app.handle_key_event(KeyCode::Backspace.into(), &config.keys),
            Some("")
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(
            app.handle_key_event(KeyCode::Backspace.into(), &config.keys),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_word() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("abc def ")
        );
        assert_eq!(app.pattern, "abc def ");
        assert_eq!(app.cursor_pos, 8);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("abc ")
        );
        assert_eq!(app.pattern, "abc ");
        assert_eq!(app.cursor_pos, 4);

        input(&mut app, "    ");
        assert_eq!(app.pattern, "abc     ");
        assert_eq!(app.cursor_pos, 8);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("")
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_movement() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        assert_eq!(
            app.handle_key_event(KeyCode::Left.into(), &config.keys),
            None
        );
        assert_eq!(app.cursor_pos, 10);

        for _ in 0..4 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 6);

        for _ in 0..8 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 0);

        for _ in 0..8 {
            assert_eq!(
                app.handle_key_event(KeyCode::Right.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 8);

        for _ in 0..8 {
            assert_eq!(
                app.handle_key_event(KeyCode::Right.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 11);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_input() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        for _ in 0..4 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 7);

        input(&mut app, "bar");
        assert_eq!(app.pattern, "abc defbar ghi");
        assert_eq!(app.cursor_pos, 10);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("abc  ghi")
        );
        assert_eq!(app.pattern, "abc  ghi");
        assert_eq!(app.cursor_pos, 4);

        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some(" ghi")
        );
        assert_eq!(app.pattern, " ghi");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_home() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello world");
        assert_eq!(app.cursor_pos, 11);

        // Test Ctrl+A
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.cursor_pos, 0);

        // Move cursor away from home
        assert_eq!(
            app.handle_key_event(KeyCode::Right.into(), &config.keys),
            None
        );
        assert_eq!(app.cursor_pos, 1);

        // Test Home key
        assert_eq!(
            app.handle_key_event(KeyCode::Home.into(), &config.keys),
            None
        );
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_end() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello world");
        // Move cursor to beginning
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.cursor_pos, 0);

        // Test Ctrl+E
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.cursor_pos, 11);

        // Move cursor away from end
        assert_eq!(
            app.handle_key_event(KeyCode::Left.into(), &config.keys),
            None
        );
        assert_eq!(app.cursor_pos, 10);

        // Test End key
        assert_eq!(
            app.handle_key_event(KeyCode::End.into(), &config.keys),
            None
        );
        assert_eq!(app.cursor_pos, 11);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_char() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello");
        // Move cursor to position 2 (before 'l')
        for _ in 0..3 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 2);

        // Test Ctrl+D (delete char at cursor)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("helo")
        );
        assert_eq!(app.pattern, "helo");
        assert_eq!(app.cursor_pos, 2);

        // Test at end of string (should do nothing)
        assert_eq!(
            app.handle_key_event(KeyCode::End.into(), &config.keys),
            None
        );
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "helo");
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_to_end_of_line() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello world");
        // Move cursor to position 5 (before ' ')
        for _ in 0..6 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 5);

        // Test Ctrl+K (delete to end)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("hello")
        );
        assert_eq!(app.pattern, "hello");
        assert_eq!(app.cursor_pos, 5);

        // Test at end of string (should do nothing)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "hello");
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_line() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello world");
        // Move cursor to middle
        for _ in 0..5 {
            assert_eq!(
                app.handle_key_event(KeyCode::Left.into(), &config.keys),
                None
            );
        }
        assert_eq!(app.cursor_pos, 6);

        // Test Ctrl+U (delete entire line)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("")
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        // Test on empty line (should do nothing)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_char_backward() {
        let mut app = LineInput::default();
        let config = Config::default();

        input(&mut app, "hello");
        assert_eq!(app.cursor_pos, 5);

        // Test Ctrl+H (same as backspace)
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
                &config.keys
            ),
            Some("hell")
        );
        assert_eq!(app.pattern, "hell");
        assert_eq!(app.cursor_pos, 4);

        // Test at beginning (should do nothing)
        assert_eq!(
            app.handle_key_event(KeyCode::Home.into(), &config.keys),
            None
        );
        assert_eq!(
            app.handle_key_event(
                KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
                &config.keys
            ),
            None
        );
        assert_eq!(app.pattern, "hell");
        assert_eq!(app.cursor_pos, 0);
    }
}
