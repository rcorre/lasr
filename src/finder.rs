use anyhow::{Context, Result};
use ast_grep_core::language::Language;
use ast_grep_language::{LanguageExt, SupportLang};
use crossbeam::channel::Sender;
use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, sinks},
};
use regex::{Regex, RegexBuilder};
use std::path::{Path, PathBuf};
use tracing::trace;

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

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub paths: Vec<PathBuf>,
    pub ignore_case: bool,
    pub multi_line: bool,
    pub tx: Sender<FileMatch>,
    pub types: ignore::types::Types,
    pub threads: usize,
}

pub trait Finder {
    fn find(&mut self, path: &Path) -> Result<Vec<LineMatch>>;
}

#[derive(Clone, Debug)]
pub struct RegexFinder {
    regex: Regex,
    matcher: RegexMatcher,
    searcher: Searcher,
}

impl RegexFinder {
    pub fn new(pattern: &str, params: &SearchParams) -> Result<Self> {
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(params.ignore_case)
            .build()
            .with_context(|| format!("Invalid regex: {pattern}"))?;

        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .case_insensitive(params.ignore_case)
            .multi_line(params.multi_line)
            .build(pattern)
            .with_context(|| format!("Failed to compile searcher with params: {params:?}"))?;

        let searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(0))
            .multi_line(params.multi_line)
            .build();

        Ok(Self {
            regex,
            matcher,
            searcher,
        })
    }
}

impl Finder for RegexFinder {
    fn find(&mut self, path: &Path) -> Result<Vec<LineMatch>> {
        let mut lines = vec![];
        self.searcher.search_path(
            &self.matcher,
            path,
            sinks::UTF8(|number, text| {
                lines.push(LineMatch {
                    number,
                    text: text.to_string(),
                });
                Ok(true)
            }),
        )?;

        Ok(lines)
    }
}

#[derive(Clone, Debug)]
pub struct AstFinder {
    pattern: String,
}

impl AstFinder {
    pub fn new(pattern: impl Into<String>) -> Result<Self> {
        Ok(Self {
            pattern: pattern.into(),
        })
    }
}

impl Finder for AstFinder {
    fn find(&mut self, path: &Path) -> Result<Vec<LineMatch>> {
        let Some(lang) = SupportLang::from_path(path) else {
            trace!("No AST language for {path:?}");
            return Ok(vec![]);
        };

        trace!(
            "reading {path:?} of lang {lang} with pattern {}",
            self.pattern
        );
        let src = std::fs::read_to_string(path).with_context(|| format!("Reading {path:?}"))?;
        let root = lang.ast_grep(src);

        Ok(root
            .root()
            .find_all(self.pattern.as_str())
            .inspect(|m| eprintln!("{}", m.text()))
            .map(|m| LineMatch {
                number: m.start_pos().line() as u64,
                text: m.text().into(),
            })
            .collect())
    }
}
