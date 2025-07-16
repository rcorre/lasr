use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use grep::{
    regex::RegexMatcherBuilder,
    searcher::{BinaryDetection, SearcherBuilder, sinks},
};
use std::path::PathBuf;
use tracing::{debug, info, trace};
use walkdir::WalkDir;

pub struct LineMatch {
    pub number: u64,
    pub text: String,
}

pub struct FileMatch {
    pub path: PathBuf,
    pub lines: Vec<LineMatch>,
}

// rx sends the new pattern
pub fn search<F: Fn(FileMatch) -> Result<()>>(
    mut rx: Receiver<String>,
    path: PathBuf,
    func: F,
) -> Result<()> {
    let mut pattern = Some(rx.recv()?);

    while let Some(p) = pattern {
        debug!("Starting search with pattern: '{p}'");
        pattern = do_search(p, &mut rx, &path, &func)?;
    }

    info!("Ending search");
    Ok(())
}

fn do_search<F: Fn(FileMatch) -> Result<()>>(
    pattern: String,
    rx: &mut Receiver<String>,
    path: &PathBuf,
    func: &F,
) -> Result<Option<String>> {
    if pattern.trim().is_empty() {
        debug!("Nothing to search, awaiting non-empty pattern");
        return Ok(Some(rx.recv()?));
    }

    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .line_terminator(Some(b'\n'))
        .build(&pattern)
        .with_context(|| format!("Failed to compile searcher with pattern: {pattern}"))?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
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
            let mut lines = vec![];
            if let Err(e) = searcher.search_path(
                &matcher,
                path.path(),
                sinks::UTF8(|number, text| {
                    lines.push(LineMatch {
                        number,
                        text: text.to_string(),
                    });
                    Ok(true)
                }),
            ) {
                // Probably invalid UTF-8
                debug!("Failed to search {path:?}: {e:?}");
                continue;
            };
            if !lines.is_empty() {
                func(FileMatch {
                    path: path.into_path(),
                    lines,
                })?;
            }
        }
    }

    // Done with this search, block until we get a new pattern
    Ok(Some(rx.recv()?))
}
