use std::{ops::Range, path::PathBuf};

use super::input::LineInput;
use crate::search::{self, FileMatch};
use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, RecvError, bounded, never, select_biased, unbounded};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Position},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Row, Table, TableState},
};
use regex::Regex;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug)]
struct LineSubstitution {
    line_number: u64,
    text: String,
    matches: Vec<Range<usize>>,
}

#[derive(Debug)]
struct FileSubstitution {
    path: PathBuf,
    subs: Vec<LineSubstitution>,
}

impl LineSubstitution {
    fn to_line<'a>(&'a self, re: &'a Regex, replacement: &'a str) -> Line<'a> {
        let mut line = Line::default();
        let mut last_end = 0;

        for range in &self.matches {
            // Add text before the match
            if last_end < range.start {
                line.push_span(Span::raw(&self.text[last_end..range.start]));
            }

            // Add the match with a red background
            line.push_span(Span::styled(
                &self.text[range.clone()],
                Style::default()
                    .bg(ratatui::style::Color::LightRed)
                    .crossed_out(),
            ));

            let replaced = &self.text[range.clone()];
            let replaced = re.replace_all(replaced, replacement);
            // Add the replacement with a green background
            line.push_span(Span::styled(
                replaced,
                Style::default().bg(ratatui::style::Color::LightGreen),
            ));

            last_end = range.end;
        }

        // Add remaining text after the last match
        if last_end < self.text.len() {
            line.push_span(Span::raw(&self.text[last_end..]));
        }

        line
    }
}

pub struct App {
    path: PathBuf,
    subs: Vec<FileSubstitution>,
    search_rx: Option<Receiver<FileMatch>>,
    event_rx: Receiver<Event>,
    pattern_input: LineInput,
    replacement_input: LineInput,
    editing_pattern: bool,
    // TODO: respect casing
    re: Option<Regex>,
    replacement: String,
}

enum State {
    Continue,
    Exit,
    Confirm,
}

impl App {
    fn start_search(&mut self) {
        // blocking channel to pause the search when we aren't ready for more results
        let (tx, rx) = bounded(0);
        let pattern = self.pattern_input.pattern().to_string();
        let path = self.path.clone();
        self.search_rx.replace(rx);
        std::thread::spawn(move || -> Result<()> {
            search::search(pattern, path, tx).context("Search thread error")
        });
    }

    pub fn new(path: impl Into<PathBuf>, event_rx: Receiver<Event>) -> Self {
        let path = path.into();

        Self {
            path,
            pattern_input: LineInput::default(),
            replacement_input: LineInput::default(),
            search_rx: None,
            event_rx,
            subs: vec![],
            editing_pattern: true,
            re: None,
            replacement: "".to_string(),
        }
    }

    fn replace_all(&self) -> Result<()> {
        let Some(ref re) = self.re else {
            debug!("No replacement");
            return Ok(());
        };

        debug!("Replacing in cached results");
        for sub in &self.subs {
            let path = &sub.path;
            debug!("Replacing in {path:?}");
            let text = std::fs::read_to_string(path)?;
            let text = re.replace_all(&text, &self.replacement);
            std::fs::write(path, text.as_ref())?;
        }

        let Some(ref rx) = self.search_rx else {
            debug!("No pending search results, replacement complete");
            return Ok(());
        };

        debug!("Draining remaining results");
        for finding in rx {
            let path = &finding.path;
            debug!("Replacing in {path:?}");
            let text = std::fs::read_to_string(path)?;
            let text = re.replace_all(&text, &self.replacement);
            std::fs::write(path, text.as_ref())?;
        }

        debug!("Replacement complete");
        Ok(())
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            let mut need_more = false;
            terminal.draw(|frame| need_more = self.draw(frame).unwrap())?;
            match self.handle_events(need_more)? {
                State::Continue => {}
                State::Exit => return Ok(()),
                State::Confirm => return self.replace_all(),
            }
        }
    }

    // returns true if more results are needed
    fn draw(&mut self, frame: &mut Frame) -> Result<bool> {
        trace!("Drawing");

        let [input_area, search_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Fill(1)])
            .margin(1) // to account for the border we draw around everything
            .areas(frame.area());

        let [pattern_area, tab_area, replace_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Length(self.pattern_input.size().max(16)),
                Constraint::Length(9),
                Constraint::Length(self.replacement_input.size().max(16)),
            ])
            .areas(input_area);

        self.pattern_input.draw(frame, pattern_area, "Search");
        self.replacement_input.draw(frame, replace_area, "Replace");

        frame.render_widget(Paragraph::new("\n< TAB >").centered(), tab_area);

        // All the +1s account for borders
        frame.set_cursor_position(if self.editing_pattern {
            Position::new(
                pattern_area.x + self.pattern_input.cursor_pos() + 1,
                pattern_area.y + 1,
            )
        } else {
            Position::new(
                replace_area.x + self.replacement_input.cursor_pos() + 1,
                replace_area.y + 1,
            )
        });

        let mut size_left = search_area.height;
        let constraints: Vec<_> = self
            .subs
            .iter()
            .map(|s| (s.subs.len() + 2) as u16) // +2 for top/bottom border
            .take_while(|s| {
                let ret = size_left > 0;
                size_left = size_left.saturating_sub(*s);
                ret
            })
            .map(Constraint::Length)
            .collect();

        let Some(ref re) = self.re else {
            return Ok(false);
        };
        let search_areas = Layout::vertical(constraints.as_slice()).split(search_area);
        for (area, sub) in search_areas.iter().zip(self.subs.iter()) {
            let table = Table::new(
                sub.subs.iter().map(|s| {
                    Row::new(vec![
                        Line::raw(s.line_number.to_string()),
                        s.to_line(re, &self.replacement),
                    ])
                }),
                &[Constraint::Max(6), Constraint::Fill(1)],
            )
            .block(
                Block::bordered().title_top(
                    sub.path
                        .strip_prefix(&self.path)
                        .unwrap_or(&sub.path)
                        .to_string_lossy(),
                ),
            );
            let mut table_state = TableState::default();
            frame.render_stateful_widget(table, *area, &mut table_state);
        }

        trace!("Draw complete");
        Ok(size_left > 0)
    }

    fn on_finding(&mut self, finding: FileMatch) -> Result<()> {
        let Some(ref re) = self.re else {
            warn!("Got substitution, but no regex set");
            return Ok(());
        };
        let sub = FileSubstitution {
            path: finding.path,
            subs: finding
                .lines
                .into_iter()
                .map(|line| LineSubstitution {
                    line_number: line.number,
                    matches: re
                        .find_iter(&line.text)
                        .map(|m| Range {
                            start: m.start(),
                            end: m.end(),
                        })
                        .collect(),
                    text: line.text,
                })
                .collect(),
        };
        debug!("Pushing item: {sub:?}");
        self.subs.push(sub);
        debug!("Total items: {}", self.subs.len());
        Ok(())
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self, need_more: bool) -> Result<State> {
        trace!("Awaiting event");

        let search_rx = match self.search_rx {
            Some(ref rx) if need_more => rx,
            _ => &never(),
        };

        // Bias for events, as they may invalidate search results
        select_biased! {
            recv(self.event_rx) -> ev => {
                debug!("Handling terminal event: {ev:?}");
                match ev? {
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        return self.handle_key_event(key_event);
                    }
                    _ => {}
                };
            }
            recv(search_rx) -> sub => {
                match sub {
                    Ok(sub) => self.on_finding(sub)?,
                    Err(RecvError) => {
                        debug!("Search complete");
                        self.search_rx = None;
                    }
                }
            }
        }
        Ok(State::Continue)
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<State> {
        // these keys are handled regardless of whether we're editing the query
        match key_event.code {
            KeyCode::Esc => {
                debug!("Exit requested");
                return Ok(State::Exit);
            }
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                debug!("Exit requested");
                return Ok(State::Exit);
            }
            KeyCode::Tab => {
                self.editing_pattern = !self.editing_pattern;
                info!(
                    "Toggled editing mode. editing_pattern={}",
                    self.editing_pattern
                );
            }
            KeyCode::Enter => {
                return Ok(State::Confirm);
            }
            _ => {}
        }

        if self.editing_pattern {
            let Some(pattern) = self.pattern_input.handle_key_event(key_event) else {
                debug!("Pattern unchanged");
                return Ok(State::Continue);
            };
            self.re = match Regex::new(pattern) {
                Ok(re) => Some(re),
                Err(err) => {
                    // Expected to happen as the user is typing, not an error
                    info!("Not a valid regex: '{pattern}': {err}");
                    return Ok(State::Continue);
                }
            };
            info!("New pattern: {pattern}");
            self.start_search();
            self.subs.clear();
        } else {
            let Some(replacement) = self.replacement_input.handle_key_event(key_event) else {
                debug!("Replacement unchanged");
                return Ok(State::Continue);
            };
            self.replacement = replacement.to_string();
            info!("New pattern: {replacement}");
        }

        Ok(State::Continue)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::App;
    use crossbeam::channel::{Sender, bounded};
    use crossterm::event::{Event, KeyCode};
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::{Terminal, backend::TestBackend};

    struct Test {
        app: App,
        event_tx: Sender<Event>,
    }

    impl Test {
        fn new() -> Self {
            Self::with_dir(Path::new("testdata"))
        }

        fn with_dir(path: &Path) -> Self {
            let (event_tx, event_rx) = bounded(1);
            Test {
                app: App::new(path, event_rx),
                event_tx,
            }
        }

        fn input(&mut self, s: &str) {
            for c in s.chars() {
                self.event_tx
                    .send(Event::Key(KeyCode::Char(c).into()))
                    .unwrap();
                self.app.handle_events(true).unwrap();
            }
        }
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_empty() {
        let mut test = Test::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                test.app.draw(frame).unwrap();
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_search() {
        let mut test = Test::new();
        test.input("line");

        // await results from 2 files
        test.app.handle_events(true).unwrap();
        test.app.handle_events(true).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|frame| {
                assert!(test.app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    #[tracing_test::traced_test]
    // BUG: weird how these collapse, would expect full results until last one
    // TODO: Show when results are truncated
    fn test_search_results_full() {
        let mut test = Test::new();
        test.input("line");
        // Use a smaller y size, so the results fill the page
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();

        test.app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(test.app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();

        test.app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(
                    !test.app.draw(frame).unwrap(),
                    "Should not need more results"
                );
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_replace() {
        let tmp = tempfile::tempdir().unwrap();
        for entry in ignore::Walk::new("testdata") {
            let entry = entry.unwrap();
            let src = entry.path();
            let dst = tmp.path().join(src.strip_prefix("testdata").unwrap());
            tracing::debug!("Test copying {src:?} to {dst:?}");

            let meta = entry.metadata().unwrap();
            if meta.is_file() {
                std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
                std::fs::copy(src, dst).unwrap();
            }
        }

        let mut test = Test::with_dir(tmp.path());
        test.input("line");
        test.app.handle_key_event(KeyCode::Tab.into()).unwrap();
        test.input("replacement");

        // await results from 2 files
        test.app.handle_events(true).unwrap();
        test.app.handle_events(true).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|frame| {
                assert!(test.app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());

        test.app.replace_all().unwrap();

        let content = std::fs::read_to_string(tmp.path().join("file1.txt")).unwrap();
        assert_eq!(
            content,
            "\
This is replacement one.
This is replacement two.
This is replacement three.
Line four.
"
        );

        let content = std::fs::read_to_string(tmp.path().join("dir1").join("file2.txt")).unwrap();
        assert_eq!(
            content,
            "\
The first replacement.
The second replacement.
The third replacement.
"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_replace_capture() {
        let tmp = tempfile::tempdir().unwrap();
        for entry in ignore::Walk::new("testdata") {
            let entry = entry.unwrap();
            let src = entry.path();
            let dst = tmp.path().join(src.strip_prefix("testdata").unwrap());
            tracing::debug!("Test copying {src:?} to {dst:?}");

            let meta = entry.metadata().unwrap();
            if meta.is_file() {
                std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
                std::fs::copy(src, dst).unwrap();
            }
        }

        let mut test = Test::with_dir(tmp.path());
        test.input("This is");
        test.app.handle_key_event(KeyCode::Tab.into()).unwrap();
        test.input("${0}n't");

        // await results from 2 files
        test.app.handle_events(true).unwrap();
        test.app.handle_events(true).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|frame| {
                assert!(test.app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());

        test.app.replace_all().unwrap();

        let content = std::fs::read_to_string(tmp.path().join("file1.txt")).unwrap();
        assert_eq!(
            content,
            "\
This isn't line one.
This isn't line two.
This isn't line three.
Line four.
"
        );

        let content = std::fs::read_to_string(tmp.path().join("dir1").join("file2.txt")).unwrap();
        assert_eq!(
            content,
            "\
The first line.
The second line.
The third line.
"
        );
    }
}
