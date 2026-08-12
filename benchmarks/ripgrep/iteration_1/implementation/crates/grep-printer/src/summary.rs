/*!
Summary printer for aggregate search results.

This module provides `SummaryBuilder` and `Summary`, which produce compact
output for several modes:

- **Count** (`-c`): prints the number of matching lines per file.
- **CountMatches**: prints the number of individual matches per file.
- **PathWithMatch** (`-l`): prints file paths that contain matches.
- **PathWithoutMatch** (`--files-without-match`): prints file paths that
  do NOT contain matches.
- **Quiet** (`-q`): produces no output; stops at the first match.
*/

use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::Matcher;
use grep_searcher::{Searcher, Sink, SinkFinish, SinkMatch};
use termcolor::WriteColor;

/// The kind of summary to produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SummaryKind {
    /// Print the count of matching lines per file.
    Count,
    /// Print the count of individual matches per file.
    CountMatches,
    /// Print file paths that have at least one match.
    PathWithMatch,
    /// Print file paths that have no matches.
    PathWithoutMatch,
    /// Produce no output; stop searching at the first match.
    Quiet,
}

/// Builder for configuring a [`Summary`] printer.
#[derive(Clone, Debug)]
pub struct SummaryBuilder {
    kind: SummaryKind,
    null_path: bool,
}

impl Default for SummaryBuilder {
    fn default() -> Self {
        SummaryBuilder {
            kind: SummaryKind::Count,
            null_path: false,
        }
    }
}

impl SummaryBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the kind of summary to produce.
    pub fn kind(&mut self, kind: SummaryKind) -> &mut Self {
        self.kind = kind;
        self
    }

    /// If `true`, use NUL instead of newline as the path terminator.
    pub fn null_path(&mut self, yes: bool) -> &mut Self {
        self.null_path = yes;
        self
    }

    /// Builds a [`Summary`] printer that writes to `wtr`.
    pub fn build<W: WriteColor>(&self, wtr: W) -> Summary<W> {
        Summary {
            wtr,
            kind: self.kind.clone(),
            null_path: self.null_path,
            has_matches: false,
            match_count: 0,
        }
    }
}

/// A summary printer for aggregate search results.
pub struct Summary<W> {
    wtr: W,
    kind: SummaryKind,
    null_path: bool,
    has_matches: bool,
    match_count: u64,
}

impl<W: WriteColor> Summary<W> {
    /// Returns `true` if this printer observed any matches.
    pub fn has_matches(&self) -> bool {
        self.has_matches
    }

    /// Returns the total match count observed by this printer.
    pub fn match_count(&self) -> u64 {
        self.match_count
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    /// Creates a [`SummarySink`] for searching without a file path.
    pub fn sink<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
    ) -> SummarySink<'s, M, W> {
        SummarySink {
            printer: self,
            _matcher: matcher,
            path: None,
        }
    }

    /// Creates a [`SummarySink`] for searching with a file path.
    pub fn sink_with_path<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
        path: &'s Path,
    ) -> SummarySink<'s, M, W> {
        SummarySink {
            printer: self,
            _matcher: matcher,
            path: Some(path.to_path_buf()),
        }
    }

    /// Returns the path terminator byte based on configuration.
    fn path_terminator(&self) -> &str {
        if self.null_path { "\0" } else { "\n" }
    }
}

/// A [`Sink`] implementation for the summary printer.
pub struct SummarySink<'s, M, W> {
    printer: &'s mut Summary<W>,
    _matcher: &'s M,
    path: Option<PathBuf>,
}

impl<'s, M: Matcher, W: WriteColor> Sink for SummarySink<'s, M, W> {
    type Error = io::Error;

    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        self.printer.has_matches = false;
        self.printer.match_count = 0;
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        _mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.printer.has_matches = true;
        self.printer.match_count += 1;
        if self.printer.kind == SummaryKind::Quiet {
            return Ok(false);
        }
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        _finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        let term = self.printer.path_terminator().to_string();
        match self.printer.kind {
            SummaryKind::Count | SummaryKind::CountMatches => {
                if let Some(ref path) = self.path {
                    write!(
                        self.printer.wtr,
                        "{}:{}{}",
                        path.display(),
                        self.printer.match_count,
                        term,
                    )?;
                } else {
                    write!(
                        self.printer.wtr,
                        "{}{}",
                        self.printer.match_count,
                        term,
                    )?;
                }
            }
            SummaryKind::PathWithMatch => {
                if self.printer.has_matches {
                    if let Some(ref path) = self.path {
                        write!(
                            self.printer.wtr,
                            "{}{}",
                            path.display(),
                            term,
                        )?;
                    }
                }
            }
            SummaryKind::PathWithoutMatch => {
                if !self.printer.has_matches {
                    if let Some(ref path) = self.path {
                        write!(
                            self.printer.wtr,
                            "{}{}",
                            path.display(),
                            term,
                        )?;
                    }
                }
            }
            SummaryKind::Quiet => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Match, Matcher, NoCaptures, NoError};
    use grep_searcher::SearcherBuilder;
    use termcolor::NoColor;

    struct TestMatcher {
        needle: Vec<u8>,
    }

    impl TestMatcher {
        fn new(needle: &str) -> Self {
            TestMatcher {
                needle: needle.as_bytes().to_vec(),
            }
        }
    }

    impl Matcher for TestMatcher {
        type Error = NoError;
        type Captures = NoCaptures;

        fn find_at(
            &self,
            haystack: &[u8],
            at: usize,
        ) -> Result<Option<Match>, NoError> {
            if at > haystack.len() {
                return Ok(None);
            }
            match memchr::memmem::find(&haystack[at..], &self.needle) {
                Some(pos) => {
                    let start = at + pos;
                    let end = start + self.needle.len();
                    Ok(Some(Match::new(start, end)))
                }
                None => Ok(None),
            }
        }

        fn new_captures(&self) -> Result<NoCaptures, NoError> {
            Ok(NoCaptures::new())
        }
    }

    fn summary_output(
        kind: SummaryKind,
        matcher: &TestMatcher,
        haystack: &[u8],
        path: Option<&Path>,
    ) -> String {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = SummaryBuilder::new().kind(kind).build(buf);
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = if let Some(p) = path {
                printer.sink_with_path(matcher, p)
            } else {
                printer.sink(matcher)
            };
            searcher
                .search_slice(matcher, haystack, &mut sink)
                .unwrap();
        }
        String::from_utf8(printer.get_mut().get_ref().clone()).unwrap()
    }

    #[test]
    fn test_count_with_matches() {
        let matcher = TestMatcher::new("foo");
        let output = summary_output(
            SummaryKind::Count,
            &matcher,
            b"foo\nbar\nfoo\n",
            None,
        );
        assert_eq!(output.trim(), "2");
    }

    #[test]
    fn test_count_with_path() {
        let matcher = TestMatcher::new("foo");
        let path = Path::new("test.txt");
        let output = summary_output(
            SummaryKind::Count,
            &matcher,
            b"foo\n",
            Some(path),
        );
        assert_eq!(output.trim(), "test.txt:1");
    }

    #[test]
    fn test_count_no_matches() {
        let matcher = TestMatcher::new("baz");
        let output = summary_output(
            SummaryKind::Count,
            &matcher,
            b"foo\nbar\n",
            None,
        );
        assert_eq!(output.trim(), "0");
    }

    #[test]
    fn test_path_with_match() {
        let matcher = TestMatcher::new("foo");
        let path = Path::new("found.txt");
        let output = summary_output(
            SummaryKind::PathWithMatch,
            &matcher,
            b"foo\n",
            Some(path),
        );
        assert_eq!(output.trim(), "found.txt");
    }

    #[test]
    fn test_path_with_match_no_match() {
        let matcher = TestMatcher::new("baz");
        let path = Path::new("notfound.txt");
        let output = summary_output(
            SummaryKind::PathWithMatch,
            &matcher,
            b"foo\n",
            Some(path),
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_path_without_match() {
        let matcher = TestMatcher::new("baz");
        let path = Path::new("empty.txt");
        let output = summary_output(
            SummaryKind::PathWithoutMatch,
            &matcher,
            b"foo\n",
            Some(path),
        );
        assert_eq!(output.trim(), "empty.txt");
    }

    #[test]
    fn test_path_without_match_has_match() {
        let matcher = TestMatcher::new("foo");
        let path = Path::new("matched.txt");
        let output = summary_output(
            SummaryKind::PathWithoutMatch,
            &matcher,
            b"foo\n",
            Some(path),
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_quiet() {
        let matcher = TestMatcher::new("foo");
        let output = summary_output(
            SummaryKind::Quiet,
            &matcher,
            b"foo\nfoo\nfoo\n",
            None,
        );
        assert!(output.is_empty());
    }

    #[test]
    fn test_quiet_stops_early() {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer =
            SummaryBuilder::new().kind(SummaryKind::Quiet).build(buf);
        let matcher = TestMatcher::new("foo");
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(
                    &matcher,
                    b"foo\nfoo\nfoo\n",
                    &mut sink,
                )
                .unwrap();
        }
        // In quiet mode, we should stop after the first match.
        assert!(printer.has_matches());
        assert_eq!(printer.match_count(), 1);
    }

    #[test]
    fn test_has_matches_false() {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer =
            SummaryBuilder::new().kind(SummaryKind::Count).build(buf);
        let matcher = TestMatcher::new("baz");
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(&matcher, b"foo\n", &mut sink)
                .unwrap();
        }
        assert!(!printer.has_matches());
    }
}
