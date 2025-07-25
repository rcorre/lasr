use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use grep::{
    regex::RegexMatcherBuilder,
    searcher::{BinaryDetection, SearcherBuilder, sinks},
};
use std::path::PathBuf;
use tracing::debug;

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

pub fn search(pattern: String, path: PathBuf, tx: Sender<FileMatch>) -> Result<()> {
    debug!("Starting search with pattern: '{pattern}'");

    let matcher = RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .build(&pattern)
        .with_context(|| format!("Failed to compile searcher with pattern: {pattern}"))?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .build();
    for path in ignore::Walk::new(path) {
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
            if !lines.is_empty()
                && tx
                    .send(FileMatch {
                        path: path.into_path(),
                        lines,
                    })
                    .is_err()
            {
                debug!("TX closed, ending search thread");
                return Ok(());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crossbeam::channel::{RecvError, unbounded};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    #[tracing_test::traced_test]
    fn test_search() {
        let (tx, rx) = unbounded();

        search("line".into(), "testdata".into(), tx).unwrap();

        let mut results = [rx.recv().unwrap(), rx.recv().unwrap()];
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

        assert_eq!(rx.recv(), Err(RecvError));
    }
}
