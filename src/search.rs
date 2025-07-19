use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use grep::{
    regex::RegexMatcherBuilder,
    searcher::{BinaryDetection, SearcherBuilder, sinks},
};
use ignore::Walk;
use std::path::PathBuf;
use tracing::{debug, info, trace};

#[derive(Debug, PartialEq)]
pub struct LineMatch {
    pub number: u64,
    pub text: String,
}

#[derive(Debug, PartialEq)]
pub struct FileMatch {
    pub path: PathBuf,
    pub lines: Vec<LineMatch>,
}

// rx sends the new pattern each time it changes
// rx sends None to mean "no more searches, complete current search and send all results"
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
    let mut ended = false;

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
    for path in Walk::new(path) {
        if !ended {
            match rx.try_recv() {
                Ok(pattern) => {
                    debug!("New pattern, restarting search");
                    return Ok(Some(pattern));
                }
                Err(TryRecvError::Empty) => {
                    trace!("No new pattern, continuing search");
                }
                Err(TryRecvError::Disconnected) => {
                    debug!("Completing search");
                    ended = true;
                }
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
    if ended {
        Ok(None)
    } else {
        Ok(Some(rx.recv()?))
    }
}

#[cfg(test)]
mod tests {
    use crossbeam::channel::bounded;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_search() {
        let (pattern_tx, pattern_rx) = bounded(0);
        let (result_tx, result_rx) = bounded(0);

        std::thread::spawn(move || {
            search(pattern_rx, "testdata".into(), |res| {
                result_tx.send(res).unwrap();
                Ok(())
            })
            .unwrap()
        });

        pattern_tx.send("line".into()).unwrap();

        let mut results = [result_rx.recv().unwrap(), result_rx.recv().unwrap()];
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            results,
            [
                FileMatch {
                    path: "testdata/dir1/file2.txt".into(),
                    lines: vec![
                        LineMatch {
                            number: 1,
                            text: "The first line.\n".into(),
                        },
                        LineMatch {
                            number: 2,
                            text: "The second line.\n".into(),
                        },
                        LineMatch {
                            number: 3,
                            text: "The third line.\n".into(),
                        },
                    ],
                },
                FileMatch {
                    path: "testdata/file1.txt".into(),
                    lines: vec![
                        LineMatch {
                            number: 1,
                            text: "This is line one.\n".into(),
                        },
                        LineMatch {
                            number: 2,
                            text: "This is line two.\n".into(),
                        },
                        LineMatch {
                            number: 3,
                            text: "This is line three.\n".into(),
                        },
                    ],
                }
            ]
        );
    }
}
