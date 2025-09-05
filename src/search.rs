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

#[derive(Debug)]
pub struct SearchParams {
    pub pattern: String,
    pub paths: Vec<PathBuf>,
    pub ignore_case: bool,
    pub multi_line: bool,
    pub tx: Sender<FileMatch>,
    pub types: ignore::types::Types,
    pub threads: usize,
}

pub fn search(params: SearchParams) -> Result<()> {
    debug!("Starting search with params: {params:?}");

    let matcher = RegexMatcherBuilder::new()
        .case_smart(false)
        .case_insensitive(params.ignore_case)
        .multi_line(params.multi_line)
        .build(&params.pattern)
        .with_context(|| format!("Failed to compile searcher with params: {params:?}"))?;

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .multi_line(params.multi_line)
        .build();

    let mut builder = ignore::WalkBuilder::new(&params.paths[0]);
    builder
        .sort_by_file_name(|a, b| a.cmp(b))
        .threads(params.threads)
        .types(params.types);
    for path in params.paths.iter().skip(1) {
        builder.add(path);
    }

    if params.threads == 1 {
        for path in builder.build() {
            match walk(&matcher, &mut searcher, path, &params.tx) {
                Ok(WalkState::Quit) => {
                    return Ok(());
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("Search error: {e}");
                }
            }
        }
        return Ok(());
    }

    builder.build_parallel().run(move || {
        let tx = params.tx.clone();
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

        search(SearchParams {
            pattern: "line".into(),
            paths: vec!["testdata".into()],
            ignore_case: false,
            multi_line: false,
            tx,
            types: types(&[]),
            threads: 1,
        })
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

        search(SearchParams {
            pattern: "the".into(),
            paths: vec!["testdata".into()],
            ignore_case: true,
            multi_line: false,
            tx,
            types: types(&[]),
            threads: 1,
        })
        .unwrap();
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

        search(SearchParams {
            pattern: "First".into(),
            paths: vec!["testdata".into()],
            ignore_case: true,
            multi_line: false,
            tx,
            types: types(&["md"]),
            threads: 1,
        })
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
