use anyhow::Result;
use regex::Regex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::{path::PathBuf, sync::Arc};
use tracing::trace;
use walkdir::WalkDir;

pub struct Finding {
    pub path: PathBuf,
    pub line_number: u64,
    pub line: String,
}

// rx sends the new pattern
pub fn search<F: Fn(Finding)>(mut rx: Receiver<String>, path: PathBuf, func: F) -> Result<()> {
    let mut pattern = Some(rx.recv()?);

    while pattern.is_some() {
        pattern = do_search(pattern.unwrap(), &mut rx, &path, &func)?;
    }

    Ok(())
}

fn do_search<F: Fn(Finding)>(
    pattern: String,
    rx: &mut Receiver<String>,
    path: &PathBuf,
    func: &F,
) -> Result<Option<String>> {
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_smart(true)
        .line_terminator(Some(b'\n'))
        .build(&pattern)?;
    let mut searcher = grep_searcher::SearcherBuilder::new()
        .binary_detection(grep_searcher::BinaryDetection::quit(0))
        .build();
    for path in WalkDir::new(path) {
        match rx.try_recv() {
            Ok(pattern) => return Ok(Some(pattern)),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(None);
            }
        }
        let path = path?;
        let meta = path.metadata()?;
        if meta.is_file() {
            let path = path.path();
            searcher.search_path(
                &matcher,
                path,
                grep_searcher::sinks::UTF8(|line_number, line| {
                    func(Finding {
                        path: path.to_owned(),
                        line_number,
                        line: line.to_string(),
                    });
                    Ok(true)
                }),
            )?;
        }
    }
    Ok(None)
}
