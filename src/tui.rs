use std::{ops::Range, path::PathBuf};

use super::input::LineInput;
use crate::{
    config::{Action, Config, Theme},
    search::{self, FileMatch, LineMatch, SearchParams},
};
use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, RecvError, bounded, never, select_biased};
use crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Position},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Row, Table, TableState},
};
use regex::{Regex, RegexBuilder};
use tracing::{debug, info, trace, warn};

// How many off-screen results to pre-populate
const SEARCH_BUFFER: usize = 3;

#[derive(Debug)]
struct Substitution {
    range: Range<usize>,
    replacement: String, // only set if we have a replacement string
}

#[derive(Debug)]
struct TextSubstitution {
    start_line: u64,
    line_count: u16,
    text: String,
    matches: Vec<Substitution>,
}

impl TextSubstitution {
    fn new(line: LineMatch, re: &Regex, replacement: &str) -> Self {
        Self {
            start_line: line.number,
            line_count: line.text.lines().count() as u16,
            matches: re
                .find_iter(&line.text)
                .map(|m| {
                    let range = Range {
                        start: m.start(),
                        end: m.end(),
                    };
                    let replacement = re
                        .replace_all(&line.text[range.clone()], replacement)
                        .to_string();
                    Substitution { range, replacement }
                })
                .collect(),
            text: line.text,
        }
    }

    fn update_replacement(&mut self, re: &Regex, replacement: &str) {
        for m in &mut self.matches {
            m.replacement = re
                .replace_all(&self.text[m.range.clone()], replacement)
                .to_string();
        }
    }
}

#[derive(Debug)]
struct FileSubstitution {
    path: PathBuf,
    subs: Vec<TextSubstitution>,
}

impl FileSubstitution {
    fn new(file: FileMatch, re: &Regex, replacement: &str) -> Self {
        Self {
            path: file.path,
            subs: file
                .lines
                .into_iter()
                .map(|line| TextSubstitution::new(line, re, replacement))
                .collect(),
        }
    }

    fn update_replacement(&mut self, re: &Regex, replacement: &str) {
        for s in &mut self.subs {
            s.update_replacement(re, replacement);
        }
    }

    fn line_count(&self) -> u16 {
        self.subs.iter().map(|s| s.line_count).sum()
    }
}

fn push_lines<'a>(s: &'a str, text: &mut Text<'a>, style: Style) {
    let mut lines = s.lines();
    if let Some(first_line) = lines.next() {
        text.push_span(Span::styled(first_line, style));
    }

    for line in lines {
        text.push_line(Line::default());
        text.push_span(Span::styled(line, style));
    }

    // Handle case where string ends with newline
    if s.ends_with('\n') {
        text.push_line(Line::default());
    }
}

#[test]
fn test_push_lines() {
    let mut text = Text::default();
    let style = Style::default();

    push_lines("foo bar", &mut text, style);
    assert_eq!(text, Text::raw("foo bar"));

    push_lines("biz baz\nbuz", &mut text, style);
    assert_eq!(
        text,
        vec![
            Line::from(vec![Span::raw("foo bar"), Span::raw("biz baz")]),
            Line::raw("buz"),
        ]
        .into()
    );

    push_lines("one two\nthree four\nfive six", &mut text, style);
    assert_eq!(
        text,
        vec![
            Line::from(vec![Span::raw("foo bar"), Span::raw("biz baz")]),
            Line::from(vec![Span::raw("buz"), Span::raw("one two")]),
            Line::from(vec![Span::raw("three four")]),
            Line::raw("five six")
        ]
        .into()
    );
}

impl TextSubstitution {
    fn to_text<'a>(&'a self, theme: &Theme) -> Text<'a> {
        let mut text = Text::default();
        let mut last_end = 0;

        for sub in &self.matches {
            let range = &sub.range;
            // Add text before the match
            if last_end < range.start {
                push_lines(&self.text[last_end..range.start], &mut text, theme.base);
            }

            if sub.replacement.is_empty() {
                // no replacement text, draw the existing text
                push_lines(&self.text[range.clone()], &mut text, theme.find);
            } else {
                push_lines(&sub.replacement, &mut text, theme.replace);
            }

            last_end = range.end;
        }

        // Add remaining text after the last match
        if last_end < self.text.len() {
            push_lines(&self.text[last_end..], &mut text, theme.base);
        }

        text
    }
}

#[test]
fn test_line_substitution_to_text_find() {
    let theme = Theme::default();
    assert_eq!(
        TextSubstitution {
            start_line: 1,
            line_count: 1,
            text: "foo bar baz".into(),
            matches: vec![Substitution {
                range: Range { start: 4, end: 7 },
                replacement: "".to_string(),
            }],
        }
        .to_text(&theme),
        Text::from(Line::from(vec![
            Span::styled("foo ", theme.base),
            Span::styled("bar", theme.find),
            Span::styled(" baz", theme.base),
        ]))
    );
}

#[test]
fn test_line_substitution_to_text_replace() {
    let theme = Theme::default();
    assert_eq!(
        TextSubstitution {
            start_line: 1,
            line_count: 1,
            text: "foo bar baz".into(),
            matches: vec![Substitution {
                range: Range { start: 4, end: 7 },
                replacement: "test".into()
            }],
        }
        .to_text(&theme),
        Text::from(Line::from(vec![
            Span::styled("foo ", theme.base),
            Span::styled("test", theme.replace),
            Span::styled(" baz", theme.base),
        ]))
    );
}

#[test]
fn test_line_substitution_to_text_multiline() {
    // to_text should return multiple lines, with the highlight spanning
    // lines where the multi-line regex matched
    let theme = Theme::default();
    assert_eq!(
        TextSubstitution {
            start_line: 1,
            line_count: 2,
            text: "foo bar baz\nbiz baz buz".into(),
            matches: vec![Substitution {
                range: Range { start: 8, end: 15 },
                replacement: "".to_string()
            }],
        }
        .to_text(&theme),
        Text::from(vec![
            Line::from(vec![
                Span::styled("foo bar ", theme.base),
                Span::styled("baz", theme.find),
            ]),
            Line::from(vec![
                Span::styled("biz", theme.find),
                Span::styled(" baz buz", theme.base),
            ])
        ])
    );
}

#[test]
fn test_line_substitution_to_text_multiline_split_on_newline() {
    // Test multi line splitting when a range ends on a newline
    let theme = Theme::default();
    assert_eq!(
        TextSubstitution {
            start_line: 1,
            line_count: 2,
            text: "foo\nbar".into(),
            matches: vec![
                Substitution {
                    range: Range { start: 0, end: 3 },
                    replacement: "".to_string()
                },
                Substitution {
                    range: Range { start: 4, end: 7 },
                    replacement: "".to_string()
                }
            ],
        }
        .to_text(&theme),
        Text::from(vec![
            Line::from(vec![
                Span::styled("foo", theme.find),
                Span::styled("", theme.base),
            ]),
            Line::from(vec![Span::styled("bar", theme.find),])
        ])
    );
}

pub struct App {
    paths: Vec<PathBuf>,
    types: ignore::types::Types,
    config: Config,
    subs: Vec<FileSubstitution>,
    search_rx: Option<Receiver<FileMatch>>,
    event_rx: Receiver<Event>,
    pattern_input: LineInput,
    replacement_input: LineInput,
    editing_pattern: bool,
    re: Option<Regex>,
    ignore_case: bool,
    multi_line: bool,
    scroll: usize,
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
        let paths = self.paths.clone();
        self.search_rx.replace(rx);
        let ignore_case = self.ignore_case;
        let multi_line = self.multi_line;
        let types = self.types.clone();
        let threads = self.config.threads;
        std::thread::spawn(move || -> Result<()> {
            search::search(SearchParams {
                pattern,
                paths,
                ignore_case,
                multi_line,
                tx,
                types,
                threads,
            })
            .context("Search thread error")
        });
    }

    pub fn new(
        paths: Vec<PathBuf>,
        types: ignore::types::Types,
        config: Config,
        event_rx: Receiver<Event>,
        ignore_case: bool,
        multi_line: bool,
    ) -> Self {
        let paths = if paths.is_empty() {
            vec![".".into()]
        } else {
            paths
        };
        Self {
            paths,
            types,
            pattern_input: LineInput::new(config.auto_pairs),
            replacement_input: LineInput::new(config.auto_pairs),
            config,
            search_rx: None,
            event_rx,
            subs: vec![],
            editing_pattern: true,
            re: None,
            ignore_case,
            multi_line,
            scroll: 0,
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
            let text = re.replace_all(&text, self.replacement_input.pattern());
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
            let text = re.replace_all(&text, self.replacement_input.pattern());
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
        let theme = &self.config.theme;

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

        let mut flags = String::new();
        if self.ignore_case {
            flags += "i";
        }
        if self.multi_line {
            flags += "m";
        }
        let mut search_header = "Search".to_string();
        if !flags.is_empty() {
            search_header = format!("{search_header} ({flags})");
        }
        self.pattern_input
            .draw(frame, pattern_area, &search_header, theme.base);
        self.replacement_input
            .draw(frame, replace_area, "Replace", theme.base);

        if let Some(swap_key) = self
            .config
            .keys
            .iter()
            .find(|(_, v)| **v == Action::ToggleSearchReplace)
            .map(|(k, _)| k)
        {
            frame.render_widget(
                Paragraph::new(format!("\n< {swap_key} >"))
                    .centered()
                    .style(theme.base),
                tab_area,
            );
        };

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
            .skip(self.scroll)
            .map(|s| (s.line_count() + 2)) // +2 for top/bottom border
            .take_while(|s| {
                let ret = size_left > 0;
                size_left = size_left.saturating_sub(*s);
                ret
            })
            .map(Constraint::Length)
            .collect();

        let search_areas = Layout::vertical(constraints.as_slice()).split(search_area);
        let subs = self.subs.iter().skip(self.scroll);
        for (area, sub) in search_areas.iter().zip(subs) {
            let table = Table::new(
                sub.subs.iter().map(|s| {
                    Row::new(vec![Text::raw(s.start_line.to_string()), s.to_text(theme)])
                        .height(s.line_count)
                }),
                &[Constraint::Max(6), Constraint::Fill(1)],
            )
            .style(theme.base)
            .block(Block::bordered().title_top(sub.path.to_string_lossy()));
            let mut table_state = TableState::default();
            frame.render_stateful_widget(table, *area, &mut table_state);
        }

        trace!("Draw complete");
        // Pause searching once we're showing all the results we can on the screen,
        // Plus a few buffered results (so scrolling is instant)
        Ok(self.subs.len() < search_areas.len() + SEARCH_BUFFER + self.scroll)
    }

    fn on_finding(&mut self, finding: FileMatch) -> Result<()> {
        let Some(ref re) = self.re else {
            warn!("Got substitution, but no regex set");
            return Ok(());
        };
        let sub = FileSubstitution::new(finding, re, self.replacement_input.pattern());
        debug!("Pushing item: {sub:?}");
        self.subs.push(sub);
        debug!("Total items: {}", self.subs.len());
        Ok(())
    }

    fn update_pattern(&mut self) {
        let pattern = self.pattern_input.pattern();
        self.re = match RegexBuilder::new(pattern)
            .case_insensitive(self.ignore_case)
            .build()
        {
            Ok(re) => Some(re),
            Err(err) => {
                // Expected to happen as the user is typing, not an error
                info!("Not a valid regex: '{pattern}': {err}");
                return;
            }
        };
        info!("New pattern: {pattern}");
        self.start_search();
        self.subs.clear();
    }

    fn update_replacement(&mut self) {
        let replacement = self.replacement_input.pattern();
        let Some(re) = &self.re else { return };
        for sub in &mut self.subs {
            sub.update_replacement(re, replacement)
        }
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
        if let Some(action) = self.config.keys.get(&key_event.into()) {
            match action {
                Action::Exit => {
                    debug!("Exit requested");
                    return Ok(State::Exit);
                }
                Action::ToggleSearchReplace => {
                    self.editing_pattern = !self.editing_pattern;
                    info!(
                        "Toggled editing mode. editing_pattern={}",
                        self.editing_pattern
                    );
                    return Ok(State::Continue);
                }
                Action::Confirm => {
                    return Ok(State::Confirm);
                }
                Action::ToggleIgnoreCase => {
                    self.ignore_case = !self.ignore_case;
                    self.update_pattern();
                    return Ok(State::Continue);
                }
                Action::ToggleMultiLine => {
                    self.multi_line = !self.multi_line;
                    self.update_pattern();
                    return Ok(State::Continue);
                }
                Action::ScrollDown => {
                    if self.scroll < self.subs.len() - 1 {
                        self.scroll += 1;
                        info!("Scrolled to: {}", self.scroll);
                    }
                    return Ok(State::Continue);
                }
                Action::ScrollUp => {
                    self.scroll = self.scroll.saturating_sub(1);
                    info!("Scrolled to: {}", self.scroll);
                    return Ok(State::Continue);
                }
                Action::ScrollTop => {
                    self.scroll = 0;
                    info!("Scrolled to: {}", self.scroll);
                    return Ok(State::Continue);
                }
                _ => {}
            }
        }

        if self.editing_pattern {
            let Some(_) = self
                .pattern_input
                .handle_key_event(key_event, &self.config.keys)
            else {
                debug!("Pattern unchanged");
                return Ok(State::Continue);
            };
            self.update_pattern();
        } else {
            let Some(_) = self
                .replacement_input
                .handle_key_event(key_event, &self.config.keys)
            else {
                debug!("Replacement unchanged");
                return Ok(State::Continue);
            };
            self.update_replacement();
            info!("New replacement: {}", self.replacement_input.pattern());
        }

        Ok(State::Continue)
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Display, path::Path};

    use crate::config::Config;

    use super::App;
    use crossbeam::channel::{Sender, bounded};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
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
                app: App::new(
                    vec![path.into()],
                    ignore::types::TypesBuilder::new()
                        .add_defaults()
                        .build()
                        .unwrap(),
                    Config {
                        threads: 1,
                        ..Default::default()
                    },
                    event_rx,
                    false,
                    false,
                ),
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

    fn scrub_tmp(tmp: &impl AsRef<Path>, s: impl Display) -> String {
        let s = format!("{s}");
        let tmp = tmp.as_ref().to_str().unwrap();
        s.replace(tmp, "<TMP>")
    }

    fn stage_files() -> tempfile::TempDir {
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
        tmp
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
    fn test_search_ignore_case() {
        let mut test = Test::new();
        test.input("the");

        // Send ctrl-s to toggle case-insensitive
        test.app
            .handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
            .unwrap();

        test.app.handle_key_event(KeyCode::Tab.into()).unwrap();
        test.input("One");

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
    fn test_search_multiline() {
        let mut test = Test::new();
        test.input("\\w+\\n\\w+");

        // Send ctrl-l to toggle multiline
        test.app
            .handle_key_event(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
            .unwrap();

        test.app.handle_key_event(KeyCode::Tab.into()).unwrap();
        test.input("One");

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
        test.input("aaa");
        // Use a smaller y size, so the results fill the page
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();

        test.app.handle_events(true).unwrap();
        test.app.handle_events(true).unwrap();
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
        let tmp = stage_files();

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
        assert_snapshot!(scrub_tmp(&tmp, terminal.backend()));

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
        let tmp = stage_files();

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
        assert_snapshot!(scrub_tmp(&tmp, terminal.backend()));

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
