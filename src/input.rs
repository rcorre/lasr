use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{
    layout::Position,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

#[derive(Default)]
pub struct LineInput {
    pattern: String,
    cursor_pos: usize,
}

impl LineInput {
    pub fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Left => {
                tracing::debug!("Moving cursor left");
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                tracing::debug!("Moving cursor right");
                self.cursor_pos = (self.cursor_pos + 1).min(self.pattern.len());
            }
            KeyCode::Char('w') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.cursor_pos == 0 {
                    return;
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
            }
            KeyCode::Backspace => {
                if self.cursor_pos == 0 {
                    return;
                };
                self.cursor_pos -= 1;
                let c = self.pattern.remove(self.cursor_pos);
                tracing::debug!("Removed '{c}' from pattern, new pattern: {}", self.pattern);
            }
            KeyCode::Char(c) if (key_event.modifiers & !KeyModifiers::SHIFT).is_empty() => {
                self.pattern.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                tracing::debug!("Updated filter pattern: {}", self.pattern);
            }
            _ => {}
        }
    }

    pub fn cursor_pos(&self) -> u16 {
        self.cursor_pos as u16
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let input =
            Paragraph::new(self.pattern.as_str()).block(Block::new().borders(Borders::BOTTOM));
        frame.render_widget(input, area);
        frame.set_cursor_position(Position::new(area.x + self.cursor_pos as u16, area.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(li: &mut LineInput, s: &str) {
        for c in s.chars() {
            li.handle_key_event(KeyCode::Char(c).into());
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

        app.handle_key_event(KeyCode::Backspace.into());
        assert_eq!(app.pattern, "ab");
        assert_eq!(app.cursor_pos, 2);

        app.handle_key_event(KeyCode::Backspace.into());
        assert_eq!(app.pattern, "a");
        assert_eq!(app.cursor_pos, 1);

        app.handle_key_event(KeyCode::Backspace.into());
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        app.handle_key_event(KeyCode::Backspace.into());
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
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

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.pattern, "abc def ");
        assert_eq!(app.cursor_pos, 8);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.pattern, "abc ");
        assert_eq!(app.cursor_pos, 4);

        input(&mut app, "    ");
        assert_eq!(app.pattern, "abc     ");
        assert_eq!(app.cursor_pos, 8);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.pattern, "");
        assert_eq!(app.cursor_pos, 0);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
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

        app.handle_key_event(KeyCode::Left.into());
        assert_eq!(app.cursor_pos, 10);

        for _ in 0..4 {
            app.handle_key_event(KeyCode::Left.into());
        }
        assert_eq!(app.cursor_pos, 6);

        for _ in 0..8 {
            app.handle_key_event(KeyCode::Left.into());
        }
        assert_eq!(app.cursor_pos, 0);

        for _ in 0..8 {
            app.handle_key_event(KeyCode::Right.into());
        }
        assert_eq!(app.cursor_pos, 8);

        for _ in 0..8 {
            app.handle_key_event(KeyCode::Right.into());
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
            app.handle_key_event(KeyCode::Left.into());
        }
        assert_eq!(app.cursor_pos, 7);

        input(&mut app, "bar");
        assert_eq!(app.pattern, "abc defbar ghi");
        assert_eq!(app.cursor_pos, 10);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.pattern, "abc  ghi");
        assert_eq!(app.cursor_pos, 4);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.pattern, " ghi");
        assert_eq!(app.cursor_pos, 0);
    }
}
