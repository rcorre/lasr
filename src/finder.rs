use anyhow::{Context, Result};
use ast_grep_core::{Pattern, language::Language};
use ast_grep_language::{LanguageExt, SupportLang};
use grep::{
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, sinks},
};
use regex::{Regex, RegexBuilder};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
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
    pub types: ignore::types::Types,
    pub threads: usize,
}

#[derive(Debug, Clone)]
pub struct RegexParams {
    pub ignore_case: bool,
    pub multi_line: bool,
}

#[derive(Debug, Clone)]
pub enum Finder {
    Regex(Box<RegexFinder>),
    Ast(AstFinder),
}

fn is_ast_pattern(pattern: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    let re = REGEX.get_or_init(|| Regex::new("\\$[A-Z_][A-Z_0-9]*|\\$\\$\\$").unwrap());
    re.is_match(pattern)
}

#[test]
fn test_is_ast_pattern() {
    assert!(is_ast_pattern("let $X ="));
    assert!(is_ast_pattern("fn($$$ARGS)"));
    assert!(!is_ast_pattern("^foo$"));
    assert!(!is_ast_pattern("foo"));
    assert!(!is_ast_pattern("foo.*"));
    assert!(!is_ast_pattern("foo(.*)"));
}

impl Finder {
    pub fn new(pattern: &str, params: &RegexParams) -> Option<Self> {
        if is_ast_pattern(pattern) {
            return Some(Self::Ast(AstFinder::new(pattern)));
        }
        match RegexFinder::new(pattern, params) {
            Ok(f) => Some(Self::Regex(Box::new(f))),
            Err(e) => {
                trace!("Not a valid regex pattern: {pattern}: {e}");
                None
            }
        }
    }

    pub fn find(&mut self, path: &Path) -> Result<Vec<LineMatch>> {
        match self {
            Finder::Regex(f) => f.find(path),
            Finder::Ast(f) => f.find(path),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RegexFinder {
    regex: Regex,
    matcher: RegexMatcher,
    searcher: Searcher,
}

impl RegexFinder {
    pub fn new(pattern: &str, params: &RegexParams) -> Result<Self> {
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
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
        }
    }

    fn find(&mut self, path: &Path) -> Result<Vec<LineMatch>> {
        let Some(lang) = SupportLang::from_path(path) else {
            trace!("No AST language for {path:?}");
            return Ok(vec![]);
        };

        let pattern = match Pattern::try_new(&self.pattern, lang) {
            Ok(p) => p,
            Err(e) => {
                trace!("Invalid pattern for language {lang:?}: {e}");
                return Ok(vec![]);
            }
        };

        trace!(
            "reading {path:?} of lang {lang} with pattern {}",
            self.pattern
        );
        let src = std::fs::read_to_string(path).with_context(|| format!("Reading {path:?}"))?;
        let root = lang.ast_grep(src);
        let node = root.root();

        Ok(node
            .find_all(pattern)
            .map(|m| LineMatch {
                number: m.start_pos().line() as u64,
                text: m.text().into(),
            })
            .collect())
    }
}
