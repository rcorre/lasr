use std::path::PathBuf;

use crate::search;

use super::input::LineInput;
use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::{FutureExt as _, StreamExt as _};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    widgets::{Block, Paragraph, Row, Table, TableState},
};
use tokio::sync::mpsc::{self, Receiver};

pub struct Substitution {
    path: String,
    line: String,
    before: String,
    after: String,
}

pub struct App {
    event_stream: EventStream,
    exit: bool,
    subs: Vec<Substitution>,
    rx: Receiver<Substitution>,
    line_input: LineInput,
}

impl App {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(16);

        std::thread::spawn(move || {
            search::search("regex", ".".into(), |finding| {
                tx.blocking_send(Substitution {
                    path: finding.path.to_string_lossy().to_string(),
                    line: finding.line,
                    before: finding.line_number.to_string(),
                    after: "".to_string(), // TODO
                })
                .unwrap()
            })
            .unwrap();
        });

        Ok(Self {
            event_stream: EventStream::default(),
            exit: false,
            line_input: LineInput::default(),
            rx,
            subs: vec![],
        })
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame).unwrap())?;
            self.handle_events().await?;
        }
        Ok(())
    }

    // returns true if more results are needed
    fn draw(&mut self, frame: &mut Frame) -> Result<()> {
        tracing::debug!("Drawing");

        let [input_area, search_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2), Constraint::Fill(1)])
            .margin(1) // to account for the border we draw around everything
            .areas(frame.area());

        self.line_input.draw(frame, input_area);

        let mut table_state = TableState::default();
        let table = Table::new(
            self.subs
                .iter()
                .map(|s| {
                    Row::new(vec![
                        s.path.as_str(),
                        s.line.as_str(),
                        s.before.as_str(),
                        s.after.as_str(),
                    ])
                })
                .chain(std::iter::once(Row::new(vec![
                    "...".to_string(),
                    "loading".to_string(),
                ]))),
            &[Constraint::Max(8), Constraint::Fill(1)],
        )
        .row_highlight_style(Style::new().bold().reversed())
        .highlight_symbol(">");
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

    /// updates the application's state based on user input
    async fn handle_events(&mut self) -> Result<()> {
        tracing::trace!("Awaiting event");

        tokio::select! {
            event = self.event_stream.next().fuse() => {
                tracing::debug!("Handling terminal event");
                let event = event.context("Event stream closed")??;
                match event {
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        self.handle_key_event(key_event).await?
                    }
                    _ => {}
                };
            },
            Some(sub) = self.rx.recv() => {
                self.subs.push(sub);
                tracing::debug!("Pushing subtitution into list, total subs: {}", self.subs.len());
            }
        }
        Ok(())
    }

    async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        // these keys are handled regardless of whether we're editing the query
        match key_event.code {
            KeyCode::Esc => {
                tracing::debug!("Exit requested");
                self.exit = true;
            }
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                tracing::debug!("Exit requested");
                self.exit = true;
            }
            KeyCode::Enter => {
                // self.editing_query = false;
                // TODO
            }
            _ => {}
        }

        self.line_input.handle_key_event(key_event);
        Ok(())
    }
}
