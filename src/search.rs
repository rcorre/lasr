use anyhow::Result;
use regex::Regex;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc::Sender;
use tracing::trace;
use walkdir::WalkDir;

pub struct Finding {
    pub path: PathBuf,
    pub line_number: u64,
    pub line: String,
}

pub fn search<F: Fn(Finding)>(pattern: &str, path: PathBuf, func: F) -> Result<()> {
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_smart(true)
        .line_terminator(Some(b'\n'))
        .build(pattern)?;
    let mut searcher = grep_searcher::SearcherBuilder::new()
        .binary_detection(grep_searcher::BinaryDetection::quit(0))
        .build();
    for path in WalkDir::new(path) {
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
    Ok(())
}
