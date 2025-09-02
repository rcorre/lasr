use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use grep::{
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, sinks},
};
use ignore::WalkState;
use std::path::PathBuf;
use tracing::{debug, warn};

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

fn walk(
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    path: Result<ignore::DirEntry, ignore::Error>,
    tx: &Sender<FileMatch>,
) -> Result<WalkState> {
    debug!("Searching path {path:?}");
    let path = path?;
    let meta = path.metadata()?;
    if !meta.is_file() {
        return Ok(WalkState::Continue);
    };
    let mut lines = vec![];
    searcher.search_path(
        matcher,
        path.path(),
        sinks::UTF8(|number, text| {
            lines.push(LineMatch {
                number,
                text: text.to_string(),
            });
            Ok(true)
        }),
    )?;
    if lines.is_empty() {
        return Ok(WalkState::Continue);
    }
    if tx
        .send(FileMatch {
            path: path.into_path(),
            lines,
        })
        .is_err()
    {
        debug!("TX closed, ending search thread");
        return Ok(WalkState::Quit);
    }
    Ok(WalkState::Continue)
}

pub fn search(
    pattern: String,
    paths: Vec<PathBuf>,
    ignore_case: bool,
    tx: Sender<FileMatch>,
    types: ignore::types::Types,
) -> Result<()> {
    debug!("Starting search with pattern: '{pattern}', ignore_case: {ignore_case}");

    let matcher = RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .case_smart(false)
        .case_insensitive(ignore_case)
        .build(&pattern)
        .with_context(|| format!("Failed to compile searcher with pattern: {pattern}"))?;

    let searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut builder = ignore::WalkBuilder::new(&paths[0]);
    builder
        .sort_by_file_name(|a, b| a.cmp(b))
        .threads(0)
        .types(types);
    for path in paths.iter().skip(1) {
        builder.add(path);
    }

    builder.build_parallel().run(move || {
        let tx = tx.clone();
        let mut searcher = searcher.clone();
        let matcher = matcher.clone();
        Box::new(move |path| -> WalkState {
            match walk(&matcher, &mut searcher, path, &tx) {
                Ok(state) => state,
                Err(e) => {
                    warn!("Search error: {e}");
                    WalkState::Continue
                }
            }
        })
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use crossbeam::channel::{RecvError, unbounded};
    use pretty_assertions::assert_eq;

    use super::*;

    fn types(t: &[&str]) -> ignore::types::Types {
        let mut types = ignore::types::TypesBuilder::new();
        types.add_defaults();
        for t in t {
            types.select(t);
        }
        types.build().unwrap()
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_search() {
        let (tx, rx) = unbounded();

        search(
            "line".into(),
            vec!["testdata".into()],
            false,
            tx,
            types(&[]),
        )
        .unwrap();

        let mut results: Vec<_> = rx.iter().collect();
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

    #[test]
    #[tracing_test::traced_test]
    fn test_search_ignore_case() {
        let (tx, rx) = unbounded();

        search("the".into(), vec!["testdata".into()], true, tx, types(&[])).unwrap();
        let mut results: Vec<_> = rx.iter().collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            results,
            [FileMatch {
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
            },]
        );

        assert_eq!(rx.recv(), Err(RecvError));
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_search_file_types() {
        let (tx, rx) = unbounded();

        search(
            "First".into(),
            vec!["testdata".into()],
            true,
            tx,
            types(&["md"]),
        )
        .unwrap();
        let mut results: Vec<_> = rx.iter().collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            results,
            [FileMatch {
                path: "testdata/example.md".into(),
                lines: vec![LineMatch {
                    number: 1,
                    text: "# First heading\n".into(),
                },],
            },]
        );

        assert_eq!(rx.recv(), Err(RecvError));
    }
}
