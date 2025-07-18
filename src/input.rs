use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{
    Frame,
    widgets::{Block, Borders, Paragraph},
};

#[derive(Default)]
pub struct LineInput {
    pattern: String,
    cursor_pos: usize,
}

impl LineInput {
    // Returns true if the pattern changed
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> Option<&str> {
        match key_event.code {
            KeyCode::Left => {
                tracing::debug!("Moving cursor left");
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                tracing::debug!("Moving cursor right");
                self.cursor_pos = (self.cursor_pos + 1).min(self.pattern.len());
                None
            }
            KeyCode::Char('w') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
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
                Some(&self.pattern)
            }
            KeyCode::Backspace => {
                if self.cursor_pos == 0 {
                    return None;
                };
                self.cursor_pos -= 1;
                let c = self.pattern.remove(self.cursor_pos);
                tracing::debug!("Removed '{c}' from pattern, new pattern: {}", self.pattern);
                Some(&self.pattern)
            }
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

    pub fn draw(&self, frame: &mut Frame, area: Rect, title: &str) {
        let input = Paragraph::new(self.pattern.as_str())
            .block(Block::new().borders(Borders::all()).title(title));
        frame.render_widget(input, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(li: &mut LineInput, s: &str) {
        for c in s.chars() {
            let result = li
                .handle_key_event(KeyCode::Char(c).into())
                .unwrap()
                .to_string();
            assert_eq!(result, li.pattern());
        }
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_input() {
        let mut app = LineInput::default();

        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        input(&mut app, "abc");
        assert_eq!(app.pattern, "abc");
        assert_eq!(app.cursor_pos, 3);

        assert_eq!(app.handle_key_event(KeyCode::Backspace.into()), Some("ab"));
        assert_eq!(app.pattern, "ab");
        assert_eq!(app.cursor_pos, 2);

        assert_eq!(app.handle_key_event(KeyCode::Backspace.into()), Some("a"));
        assert_eq!(app.pattern, "a");
        assert_eq!(app.cursor_pos, 1);

        assert_eq!(app.handle_key_event(KeyCode::Backspace.into()), Some(""));
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(app.handle_key_event(KeyCode::Backspace.into()), None);
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_delete_word() {
        let mut app = LineInput::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some("abc def ")
        );
        assert_eq!(app.pattern, "abc def ");
        assert_eq!(app.cursor_pos, 8);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some("abc ")
        );
        assert_eq!(app.pattern, "abc ");
        assert_eq!(app.cursor_pos, 4);

        input(&mut app, "    ");
        assert_eq!(app.pattern, "abc     ");
        assert_eq!(app.cursor_pos, 8);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some("")
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_movement() {
        let mut app = LineInput::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        assert_eq!(app.handle_key_event(KeyCode::Left.into()), None);
        assert_eq!(app.cursor_pos, 10);

        for _ in 0..4 {
            assert_eq!(app.handle_key_event(KeyCode::Left.into()), None);
        }
        assert_eq!(app.cursor_pos, 6);

        for _ in 0..8 {
            assert_eq!(app.handle_key_event(KeyCode::Left.into()), None);
        }
        assert_eq!(app.cursor_pos, 0);

        for _ in 0..8 {
            assert_eq!(app.handle_key_event(KeyCode::Right.into()), None);
        }
        assert_eq!(app.cursor_pos, 8);

        for _ in 0..8 {
            assert_eq!(app.handle_key_event(KeyCode::Right.into()), None);
        }
        assert_eq!(app.cursor_pos, 11);
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_cursor_input() {
        let mut app = LineInput::default();

        input(&mut app, "abc def ghi");
        assert_eq!(app.pattern, "abc def ghi");
        assert_eq!(app.cursor_pos, 11);

        for _ in 0..4 {
            assert_eq!(app.handle_key_event(KeyCode::Left.into()), None);
        }
        assert_eq!(app.cursor_pos, 7);

        input(&mut app, "bar");
        assert_eq!(app.pattern, "abc defbar ghi");
        assert_eq!(app.cursor_pos, 10);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some("abc  ghi")
        );
        assert_eq!(app.pattern, "abc  ghi");
        assert_eq!(app.cursor_pos, 4);

        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some(" ghi")
        );
        assert_eq!(app.pattern, " ghi");
        assert_eq!(app.cursor_pos, 0);
    }
}
