use std::{ops::Range, path::PathBuf};

use super::input::LineInput;
use crate::search::{self, FileMatch};
use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, Sender, bounded, never, select_biased, unbounded};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Position},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Row, Table, TableState},
};
use regex::Regex;
use tracing::{debug, info, trace, warn};

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
    fn to_line<'a>(&'a self, replacement: &'a str) -> Line<'a> {
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
                Style::default().bg(ratatui::style::Color::Red),
            ));

            // Add the replacement with a green background
            line.push_span(Span::styled(
                replacement,
                Style::default().bg(ratatui::style::Color::Green),
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
    exit: bool,
    subs: Vec<FileSubstitution>,
    search_rx: Receiver<FileMatch>,
    event_rx: Receiver<Event>,
    pattern_tx: Sender<String>,
    pattern_input: LineInput,
    replacement_input: LineInput,
    editing_pattern: bool,
    re: Option<Regex>,
    replacement: String,
}

impl App {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        // Search is bounded, so the search thread will block once we have enough results
        let (search_tx, search_rx) = bounded(1);
        // Pattern is unbounded -- we can keep sending new patterns to the search thread
        let (pattern_tx, pattern_rx) = unbounded();
        let path = path.into();
        std::thread::spawn(move || -> Result<()> {
            search::search(pattern_rx, path, |finding| {
                search_tx.send(finding)?;
                Ok(())
            })
            .context("Search thread exited")?;
            Ok(())
        });

        // Events are bounded, don't need to read more than one at once
        let (event_tx, event_rx) = unbounded();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let ev = crossterm::event::read()?;
                trace!("Sending terminal event {ev:?}");
                event_tx.send(ev)?;
            }
        });

        Ok(Self {
            exit: false,
            pattern_input: LineInput::default(),
            replacement_input: LineInput::default(),
            search_rx,
            event_rx,
            subs: vec![],
            pattern_tx,
            editing_pattern: true,
            re: None,
            replacement: "".to_string(),
        })
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.exit {
            let mut need_more = false;
            terminal.draw(|frame| need_more = self.draw(frame).unwrap())?;
            self.handle_events(need_more)?;
        }
        Ok(())
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

        let search_areas = Layout::vertical(constraints.as_slice()).split(search_area);

        for (area, sub) in search_areas.iter().zip(self.subs.iter()) {
            let table = Table::new(
                sub.subs.iter().map(|s| {
                    Row::new(vec![
                        Line::raw(s.line_number.to_string()),
                        s.to_line(&self.replacement),
                    ])
                }),
                &[Constraint::Max(6), Constraint::Fill(1)],
            )
            .block(Block::bordered().title_top(sub.path.to_string_lossy()));
            let mut table_state = TableState::default();
            frame.render_stateful_widget(table, *area, &mut table_state);
        }

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
    fn handle_events(&mut self, need_more: bool) -> Result<()> {
        trace!("Awaiting event");

        let search_rx = if need_more { &self.search_rx } else { &never() };

        // Bias for events, as they may invalidate search results
        select_biased! {
            recv(self.event_rx) -> ev => {
                debug!("Handling terminal event: {ev:?}");
                match ev? {
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        self.handle_key_event(key_event)?;
                    }
                    _ => {}
                };
            }
            recv(search_rx) -> sub => {
                self.on_finding(sub?)?;
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        // these keys are handled regardless of whether we're editing the query
        match key_event.code {
            KeyCode::Esc => {
                debug!("Exit requested");
                self.exit = true;
            }
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                debug!("Exit requested");
                self.exit = true;
            }
            KeyCode::Tab => {
                self.editing_pattern = !self.editing_pattern;
                info!(
                    "Toggled editing mode. editing_pattern={}",
                    self.editing_pattern
                );
            }
            KeyCode::Enter => {
                // self.editing_query = false;
                // TODO
            }
            _ => {}
        }

        if self.editing_pattern {
            let Some(pattern) = self.pattern_input.handle_key_event(key_event) else {
                debug!("Pattern unchanged");
                return Ok(());
            };
            self.re = match Regex::new(pattern) {
                Ok(re) => Some(re),
                Err(err) => {
                    // Expected to happen as the user is typing, not an error
                    info!("Not a valid regex: '{pattern}': {err}");
                    return Ok(());
                }
            };
            info!("New pattern: {pattern}");
            self.pattern_tx.send(pattern.to_string())?;
            // Drain obsolete results
            while self.search_rx.try_recv().is_ok() {}
            self.subs.clear();
        } else {
            let Some(replacement) = self.replacement_input.handle_key_event(key_event) else {
                debug!("Replacement unchanged");
                return Ok(());
            };
            self.replacement = replacement.to_string();
            info!("New pattern: {replacement}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use crossterm::event::KeyCode;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn input(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle_key_event(KeyCode::Char(c).into()).unwrap();
        }
    }

    #[test]
    fn test_empty() {
        let mut app = App::new("testdata").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                app.draw(frame).unwrap();
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_search() {
        let mut app = App::new("testdata").unwrap();
        input(&mut app, "line");
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();

        // await results from 2 files
        app.handle_events(true).unwrap();
        app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    // BUG: weird how these collapse, would expect full results until last one
    // TODO: Show when results are truncated
    fn test_search_results_full() {
        let mut app = App::new("testdata").unwrap();
        input(&mut app, "line");
        // Use a smaller y size, so the results fill the page
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();

        // await results from 2 files
        app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();

        app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(!app.draw(frame).unwrap(), "Should not need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_replace() {
        let mut app = App::new("testdata").unwrap();
        input(&mut app, "line");
        app.handle_key_event(KeyCode::Tab.into()).unwrap();
        input(&mut app, "replacement");
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();

        // await results from 2 files
        app.handle_events(true).unwrap();
        app.handle_events(true).unwrap();

        terminal
            .draw(|frame| {
                assert!(app.draw(frame).unwrap(), "Should need more results");
            })
            .unwrap();
        assert_snapshot!(terminal.backend());
    }
}
