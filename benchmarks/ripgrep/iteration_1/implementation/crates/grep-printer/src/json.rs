/*!
JSON printer for machine-readable search output.

This module provides `JSONBuilder` and `JSON`, which format search results
as newline-delimited JSON (one JSON object per line). Each message has a
`"type"` field indicating its kind:

- `"begin"` — emitted at the start of a file search.
- `"match"` — emitted for each matching line with match submatches.
- `"context"` — emitted for each context line.
- `"end"` — emitted when a file search finishes, with summary stats.

The `"data"` field contains message-specific details such as path, line
number, line text, byte offset, and binary detection info.

Text data is encoded either as a `"text"` field (for valid UTF-8) or a
`"bytes"` field (base64-encoded).
*/

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use grep_matcher::Matcher;
use grep_searcher::{
    Searcher, Sink, SinkContext, SinkContextKind, SinkFinish, SinkMatch,
};
use termcolor::WriteColor;

/// Builder for configuring a [`JSON`] printer.
#[derive(Clone, Debug, Default)]
pub struct JSONBuilder;

impl JSONBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self
    }

    /// Builds a [`JSON`] printer that writes to `wtr`.
    pub fn build<W: WriteColor>(&self, wtr: W) -> JSON<W> {
        JSON {
            wtr,
            match_count: 0,
        }
    }
}

/// A JSON printer for search results.
///
/// Each result is output as a single line of JSON (JSONL format).
pub struct JSON<W> {
    wtr: W,
    match_count: u64,
}

impl<W: WriteColor> JSON<W> {
    /// Returns the total number of matches printed.
    pub fn match_count(&self) -> u64 {
        self.match_count
    }

    /// Returns `true` if any matches have been printed.
    pub fn has_matches(&self) -> bool {
        self.match_count > 0
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    /// Creates a [`JSONSink`] for searching without a file path.
    pub fn sink<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
    ) -> JSONSink<'s, M, W> {
        JSONSink {
            printer: self,
            _matcher: matcher,
            path: None,
        }
    }

    /// Creates a [`JSONSink`] for searching with a file path.
    pub fn sink_with_path<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
        path: &'s Path,
    ) -> JSONSink<'s, M, W> {
        JSONSink {
            printer: self,
            _matcher: matcher,
            path: Some(path.to_path_buf()),
        }
    }
}

/// A [`Sink`] implementation for the JSON printer.
pub struct JSONSink<'s, M, W> {
    printer: &'s mut JSON<W>,
    _matcher: &'s M,
    path: Option<PathBuf>,
}

impl<'s, M, W> JSONSink<'s, M, W> {
    /// Returns the path data as a JSON value.
    fn path_data(&self) -> serde_json::Value {
        match self.path {
            Some(ref p) => {
                let s = p.to_string_lossy();
                serde_json::json!({ "text": s })
            }
            None => serde_json::Value::Null,
        }
    }
}

/// Encodes bytes as a JSON value.
///
/// If the bytes are valid UTF-8, returns `{"text": "..."}`.
/// Otherwise, returns `{"bytes": "<base64>"}`.
fn data_from_bytes(bytes: &[u8]) -> serde_json::Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => serde_json::json!({ "text": s }),
        Err(_) => {
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(bytes);
            serde_json::json!({ "bytes": encoded })
        }
    }
}

impl<'s, M: Matcher, W: WriteColor> Sink for JSONSink<'s, M, W> {
    type Error = io::Error;

    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        let obj = serde_json::json!({
            "type": "begin",
            "data": {
                "path": self.path_data()
            }
        });
        writeln!(self.printer.wtr, "{}", obj)?;
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.printer.match_count += 1;
        let line_bytes = mat.bytes();
        let line_num = mat.line_number().unwrap_or(0);

        let obj = serde_json::json!({
            "type": "match",
            "data": {
                "path": self.path_data(),
                "line_number": line_num,
                "lines": data_from_bytes(line_bytes),
                "absolute_offset": mat.absolute_byte_offset()
            }
        });
        writeln!(self.printer.wtr, "{}", obj)?;
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        let line_bytes = ctx.bytes();
        let line_num = ctx.line_number().unwrap_or(0);
        let kind = match ctx.kind() {
            SinkContextKind::Before => "before",
            SinkContextKind::After => "after",
            SinkContextKind::Other => "other",
        };

        let obj = serde_json::json!({
            "type": "context",
            "data": {
                "path": self.path_data(),
                "line_number": line_num,
                "lines": data_from_bytes(line_bytes),
                "absolute_offset": ctx.absolute_byte_offset(),
                "subtype": kind
            }
        });
        writeln!(self.printer.wtr, "{}", obj)?;
        Ok(true)
    }

    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        // Optionally emit a separator marker in JSON output.
        // We don't emit anything for context breaks in JSON output,
        // as the line numbers and offsets are sufficient.
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        let obj = serde_json::json!({
            "type": "end",
            "data": {
                "path": self.path_data(),
                "stats": {
                    "bytes_searched": finish.byte_count(),
                    "matches": self.printer.match_count,
                },
                "binary_offset": finish.binary_byte_offset()
            }
        });
        writeln!(self.printer.wtr, "{}", obj)?;
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

    fn json_output(
        matcher: &TestMatcher,
        haystack: &[u8],
        path: Option<&Path>,
    ) -> String {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = JSONBuilder::new().build(buf);
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
    fn test_json_begin_end() {
        let matcher = TestMatcher::new("nonexistent");
        let output = json_output(&matcher, b"hello\n", None);
        let lines: Vec<&str> = output.trim().lines().collect();
        assert!(lines.len() >= 2);

        let begin: serde_json::Value =
            serde_json::from_str(lines[0]).unwrap();
        assert_eq!(begin["type"], "begin");

        let end: serde_json::Value =
            serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(end["type"], "end");
    }

    #[test]
    fn test_json_match() {
        let matcher = TestMatcher::new("hello");
        let output =
            json_output(&matcher, b"hello world\n", Some(Path::new("test.rs")));
        let lines: Vec<&str> = output.trim().lines().collect();
        // Should have begin, match, end.
        assert_eq!(lines.len(), 3);

        let mat: serde_json::Value =
            serde_json::from_str(lines[1]).unwrap();
        assert_eq!(mat["type"], "match");
        assert_eq!(mat["data"]["line_number"], 1);
        assert_eq!(mat["data"]["path"]["text"], "test.rs");
        assert!(mat["data"]["lines"]["text"]
            .as_str()
            .unwrap()
            .contains("hello"));
    }

    #[test]
    fn test_json_multiple_matches() {
        let matcher = TestMatcher::new("x");
        let output = json_output(&matcher, b"x\na\nx\n", None);
        let lines: Vec<&str> = output.trim().lines().collect();
        // begin, match, match, end
        assert_eq!(lines.len(), 4);

        let m1: serde_json::Value =
            serde_json::from_str(lines[1]).unwrap();
        assert_eq!(m1["type"], "match");
        assert_eq!(m1["data"]["line_number"], 1);

        let m2: serde_json::Value =
            serde_json::from_str(lines[2]).unwrap();
        assert_eq!(m2["type"], "match");
        assert_eq!(m2["data"]["line_number"], 3);
    }

    #[test]
    fn test_json_context() {
        let matcher = TestMatcher::new("match");
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = JSONBuilder::new().build(buf);
        let mut searcher =
            SearcherBuilder::new().after_context(1).build();
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(
                    &matcher,
                    b"match line\ncontext line\n",
                    &mut sink,
                )
                .unwrap();
        }
        let output =
            String::from_utf8(printer.get_mut().get_ref().clone()).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        // begin, match, context, end
        assert_eq!(lines.len(), 4);

        let ctx: serde_json::Value =
            serde_json::from_str(lines[2]).unwrap();
        assert_eq!(ctx["type"], "context");
        assert_eq!(ctx["data"]["subtype"], "after");
    }

    #[test]
    fn test_json_binary_data() {
        // Test that non-UTF-8 data is base64 encoded.
        let data = data_from_bytes(&[0xFF, 0xFE, 0xFD]);
        assert!(data["bytes"].is_string());
    }

    #[test]
    fn test_json_utf8_data() {
        let data = data_from_bytes(b"hello");
        assert_eq!(data["text"], "hello");
    }

    #[test]
    fn test_json_end_stats() {
        let matcher = TestMatcher::new("x");
        let output = json_output(&matcher, b"x\nx\n", None);
        let lines: Vec<&str> = output.trim().lines().collect();
        let end: serde_json::Value =
            serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(end["type"], "end");
        assert_eq!(end["data"]["stats"]["matches"], 2);
    }

    #[test]
    fn test_json_no_path() {
        let matcher = TestMatcher::new("x");
        let output = json_output(&matcher, b"x\n", None);
        let lines: Vec<&str> = output.trim().lines().collect();
        let begin: serde_json::Value =
            serde_json::from_str(lines[0]).unwrap();
        assert!(begin["data"]["path"].is_null());
    }

    #[test]
    fn test_json_match_count() {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = JSONBuilder::new().build(buf);
        let matcher = TestMatcher::new("foo");
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(&matcher, b"foo\nbar\nfoo\n", &mut sink)
                .unwrap();
        }
        assert_eq!(printer.match_count(), 2);
        assert!(printer.has_matches());
    }
}
