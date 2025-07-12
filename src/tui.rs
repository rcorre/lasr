use super::input::LineInput;
use crate::search;
use anyhow::Result;
use crossbeam::channel::{Receiver, Sender, select_biased, unbounded};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    widgets::{Row, Table, TableState},
};
use tracing::{debug, info, trace};

pub struct Substitution {
    path: String,
    line: String,
    before: String,
    after: String,
}

pub struct App {
    exit: bool,
    subs: Vec<Substitution>,
    search_rx: Receiver<Substitution>,
    event_rx: Receiver<Event>,
    pattern_tx: Sender<String>,
    line_input: LineInput,
}

impl App {
    pub fn new() -> Result<Self> {
        let (search_tx, search_rx) = unbounded();
        let (pattern_tx, pattern_rx) = unbounded();
        std::thread::spawn(move || {
            search::search(pattern_rx, ".".into(), |finding| {
                search_tx
                    .send(Substitution {
                        path: finding.path.to_string_lossy().to_string(),
                        line: finding.line,
                        before: finding.line_number.to_string(),
                        after: "".to_string(), // TODO
                    })
                    .unwrap()
            })
            .unwrap();
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
            line_input: LineInput::default(),
            search_rx,
            event_rx,
            subs: vec![],
            pattern_tx,
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

        self.line_input.draw(frame, input_area);

        let mut table_state = TableState::default();
        let table = Table::new(
            self.subs.iter().map(|s| {
                Row::new(vec![
                    s.path.as_str(),
                    s.line.as_str(),
                    s.before.as_str(),
                    s.after.as_str(),
                ])
            }),
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
                self.subs.push(sub?);
                debug!(
                    "Pushing subtitution into list, total subs: {}",
                    self.subs.len()
                );
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
            KeyCode::Enter => {
                // self.editing_query = false;
                // TODO
            }
            _ => {}
        }

        if let Some(pattern) = self.line_input.handle_key_event(key_event) {
            info!("New pattern: {pattern}");
            self.pattern_tx.send(pattern.to_string())?;
            // Drain obsolete results
            while self.search_rx.try_recv().is_ok() {}
            self.subs.clear();
        }
        Ok(())
    }
}
