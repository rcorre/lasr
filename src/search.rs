use crate::finder::{FileMatch, Finder, SearchParams};
use anyhow::Result;
use crossbeam::channel::Sender;
use ignore::WalkState;
use tracing::{debug, warn};

fn walk(
    finder: &mut Finder,
    path: Result<ignore::DirEntry, ignore::Error>,
    tx: &Sender<FileMatch>,
) -> Result<WalkState> {
    debug!("Searching path {path:?}");
    let path = path?;
    let meta = path.metadata()?;
    if !meta.is_file() {
        return Ok(WalkState::Continue);
    };
    let lines = finder.find(path.path())?;
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

pub fn search(mut finder: Finder, params: SearchParams, tx: Sender<FileMatch>) -> Result<()> {
    debug!("Starting search with params: {params:?}");

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
            match walk(&mut finder, path, &tx) {
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
        let tx = tx.clone();
        let mut finder = finder.clone();
        Box::new(move |path| -> WalkState {
            match walk(&mut finder, path, &tx) {
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

    use crate::finder::LineMatch;

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

        let params = SearchParams {
            paths: vec!["testdata".into()],
            ignore_case: false,
            multi_line: false,
            types: types(&[]),
            threads: 1,
        };
        let finder = Finder::new("line", &params).unwrap();
        search(finder, params, tx).unwrap();

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

        let params = SearchParams {
            paths: vec!["testdata".into()],
            ignore_case: true,
            multi_line: false,
            types: types(&[]),
            threads: 1,
        };
        let finder = Finder::new("the", &params).unwrap();
        search(finder, params, tx).unwrap();
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

        let params = SearchParams {
            paths: vec!["testdata".into()],
            ignore_case: true,
            multi_line: false,
            types: types(&["md"]),
            threads: 1,
        };
        let finder = Finder::new("First", &params).unwrap();
        search(finder, params, tx).unwrap();
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

    #[test]
    #[tracing_test::traced_test]
    fn test_search_ast() {
        let (tx, rx) = unbounded();

        let params = SearchParams {
            paths: vec!["testdata".into()],
            ignore_case: false,
            multi_line: false,
            types: types(&[]),
            threads: 1,
        };
        let finder = Finder::new("$FN($$$ARGS)", &params).unwrap();
        search(finder, params, tx).unwrap();

        let mut results: Vec<_> = rx.iter().collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            results,
            [
                FileMatch {
                    path: "testdata/main.py".into(),
                    lines: vec![
                        LineMatch {
                            number: 1,
                            text: "print(x + y)".into(),
                        },
                        LineMatch {
                            number: 4,
                            text: "thing(3, 5)".into(),
                        },
                    ],
                },
                FileMatch {
                    path: "testdata/main.rs".into(),
                    lines: vec![LineMatch {
                        number: 5,
                        text: "thing(3, 5)".into(),
                    },],
                },
            ]
        );

        assert_eq!(rx.recv(), Err(RecvError));
    }

    #[test]
    #[tracing_test::traced_test]
    fn test_search_ast_invalid_pattern() {
        // This is a valid pattern for rust but not python
        let (tx, rx) = unbounded();

        let params = SearchParams {
            paths: vec!["testdata".into()],
            ignore_case: false,
            multi_line: false,
            types: types(&[]),
            threads: 1,
        };
        let finder = Finder::new("fn $FN", &params).unwrap();
        search(finder, params, tx).unwrap();

        let mut results: Vec<_> = rx.iter().collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            results,
            [FileMatch {
                path: "testdata/main.rs".into(),
                lines: vec![
                    LineMatch {
                        number: 0,
                        text: "fn thing(x: u64, y: u64) {\n    println!(\"{x} {y}\");\n}".into(),
                    },
                    LineMatch {
                        number: 4,
                        text: "fn main() {\n    thing(3, 5);\n}".into(),
                    },
                ],
            },]
        );

        assert_eq!(rx.recv(), Err(RecvError));
    }
}
