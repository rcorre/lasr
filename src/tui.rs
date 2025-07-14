use std::ops::Range;

use super::input::LineInput;
use crate::search::{self, Finding};
use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, Sender, select_biased, unbounded};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Row, Table, TableState},
};
use regex::Regex;
use tracing::{debug, info, trace, warn};

#[derive(Debug)]
struct Substitution {
    path: String,
    line_number: String,
    before: String,
    matches: Vec<Range<usize>>,
}

impl Substitution {
    fn to_line<'a>(&'a self, replacement: &'a str) -> Line<'a> {
        let mut line = Line::default();
        let mut last_end = 0;

        for range in &self.matches {
            // Add text before the match
            if last_end < range.start {
                line.push_span(Span::raw(&self.before[last_end..range.start]));
            }

            // Add the match with a red background
            line.push_span(Span::styled(
                &self.before[range.clone()],
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
        if last_end < self.before.len() {
            line.push_span(Span::raw(&self.before[last_end..]));
        }

        line
    }
}

pub struct App {
    exit: bool,
    subs: Vec<Substitution>,
    search_rx: Receiver<Finding>,
    event_rx: Receiver<Event>,
    pattern_tx: Sender<String>,
    pattern_input: LineInput,
    replacement_input: LineInput,
    editing_pattern: bool,
    re: Option<Regex>,
    replacement: String,
}

impl App {
    pub fn new() -> Result<Self> {
        let (search_tx, search_rx) = unbounded();
        let (pattern_tx, pattern_rx) = unbounded();
        std::thread::spawn(move || -> Result<()> {
            search::search(pattern_rx, ".".into(), |finding| {
                search_tx.send(finding)?;
                Ok(())
            })
            .context("Search thread exited")?;
            Ok(())
        });

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
            terminal.draw(|frame| self.draw(frame).unwrap())?;
            self.handle_events()?;
        }
        Ok(())
    }

    // returns true if more results are needed
    fn draw(&mut self, frame: &mut Frame) -> Result<()> {
        trace!("Drawing");

        let [input_area, search_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2), Constraint::Fill(1)])
            .margin(1) // to account for the border we draw around everything
            .areas(frame.area());

        self.pattern_input.draw(frame, input_area);

        let table = Table::new(
            self.subs.iter().map(|s| {
                Row::new(vec![
                    Line::raw(&s.path),
                    Line::raw(s.line_number.as_str()),
                    s.to_line(&self.replacement),
                ])
            }),
            &[Constraint::Fill(1), Constraint::Max(8), Constraint::Fill(4)],
        );
        let mut table_state = TableState::default();
        frame.render_stateful_widget(table, search_area, &mut table_state);

        // if self.table_state.offset() + search_area.height as usize >= self.issues.len()
        // {
        //     tracing::debug!("Requesting more items");
        //     if self.tx.try_send(PAGE_SIZE).is_err() {
        //         // TODO: watch
        //         tracing::debug!("Queue full");
        //     }
        // }

        Ok(())
    }

    fn on_finding(&mut self, finding: Finding) -> Result<()> {
        let Some(ref re) = self.re else {
            warn!("Got substitution, but no regex set");
            return Ok(());
        };
        let matches = re
            .find_iter(&finding.line)
            .map(|m| Range {
                start: m.start(),
                end: m.end(),
            })
            .collect();
        let sub = Substitution {
            path: finding.path.to_string_lossy().to_string(),
            line_number: finding.line_number.to_string(),
            before: finding.line,
            matches,
        };
        debug!("Pushing item: {sub:?}");
        self.subs.push(sub);
        debug!("Total items: {}", self.subs.len());
        Ok(())
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> Result<()> {
        trace!("Awaiting event");

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
            recv(self.search_rx) -> sub => {
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
