use anyhow::Result;
use crossbeam::channel::{Receiver, TryRecvError};
use std::path::PathBuf;
use tracing::{debug, info, trace};
use walkdir::WalkDir;

pub struct Finding {
    pub path: PathBuf,
    pub line_number: u64,
    pub line: String,
}

// rx sends the new pattern
pub fn search<F: Fn(Finding)>(mut rx: Receiver<String>, path: PathBuf, func: F) -> Result<()> {
    let mut pattern = Some(rx.recv()?);

    while let Some(p) = pattern {
        debug!("Starting search with pattern: '{p}'");
        pattern = do_search(p, &mut rx, &path, &func)?;
    }

    info!("Ending search");
    Ok(())
}

fn do_search<F: Fn(Finding)>(
    pattern: String,
    rx: &mut Receiver<String>,
    path: &PathBuf,
    func: &F,
) -> Result<Option<String>> {
    if pattern.trim().is_empty() {
        debug!("Nothing to search, awaiting non-empty pattern");
        return Ok(Some(rx.recv()?));
    }

    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_smart(true)
        .line_terminator(Some(b'\n'))
        .build(&pattern)?;
    let mut searcher = grep_searcher::SearcherBuilder::new()
        .binary_detection(grep_searcher::BinaryDetection::quit(0))
        .build();
    for path in WalkDir::new(path) {
        match rx.try_recv() {
            Ok(pattern) => {
                debug!("New pattern, restarting search");
                return Ok(Some(pattern));
            }
            Err(TryRecvError::Empty) => {
                trace!("No new pattern, continuing search");
            }
            Err(TryRecvError::Disconnected) => {
                debug!("No pattern, aborting search");
                return Ok(None);
            }
        }
        debug!("Searching  path {path:?}");
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

    // Done with this search, block until we get a new pattern
    Ok(Some(rx.recv()?))
}
