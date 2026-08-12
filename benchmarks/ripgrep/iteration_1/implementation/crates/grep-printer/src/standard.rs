/*!
Standard text printer for search results.

This module provides `StandardBuilder` and `Standard`, which format search
results in the traditional grep-like text format. Output includes optional
path labels, line numbers, column numbers, byte offsets, match highlighting,
context lines, and context break separators.

# Example

```rust,no_run
use grep_printer::StandardBuilder;
use termcolor::NoColor;

let mut printer = StandardBuilder::new()
    .line_number(true)
    .heading(false)
    .build(NoColor::new(Vec::<u8>::new()));
```
*/

use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::{Match, Matcher};
use grep_searcher::{Searcher, Sink, SinkContext, SinkFinish, SinkMatch};
use termcolor::WriteColor;

use crate::color::ColorSpecs;
use crate::hyperlink::HyperlinkFormat;

/// The default context separator.
const DEFAULT_CONTEXT_SEPARATOR: &[u8] = b"--";

/// Builder for configuring a [`Standard`] printer.
#[derive(Clone, Debug)]
pub struct StandardBuilder {
    color_specs: ColorSpecs,
    heading: bool,
    line_number: bool,
    column: bool,
    byte_offset: bool,
    only_matching: bool,
    replacement: Option<Vec<u8>>,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    trim_ascii: bool,
    null_path: bool,
    path: bool,
    hyperlink: Option<HyperlinkFormat>,
    context_separator: Vec<u8>,
    field_match_separator: Vec<u8>,
    field_context_separator: Vec<u8>,
}

impl Default for StandardBuilder {
    fn default() -> Self {
        StandardBuilder {
            color_specs: ColorSpecs::default(),
            heading: false,
            line_number: true,
            column: false,
            byte_offset: false,
            only_matching: false,
            replacement: None,
            max_columns: None,
            max_columns_preview: false,
            trim_ascii: false,
            null_path: false,
            path: true,
            hyperlink: None,
            context_separator: DEFAULT_CONTEXT_SEPARATOR.to_vec(),
            field_match_separator: b":".to_vec(),
            field_context_separator: b"-".to_vec(),
        }
    }
}

impl StandardBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the color specifications for output elements.
    pub fn color_specs(&mut self, specs: ColorSpecs) -> &mut Self {
        self.color_specs = specs;
        self
    }

    /// Enables or disables heading mode.
    ///
    /// In heading mode, the file path is printed once before its results.
    /// Otherwise the path is prefixed on every output line.
    pub fn heading(&mut self, yes: bool) -> &mut Self {
        self.heading = yes;
        self
    }

    /// Enables or disables line number output.
    pub fn line_number(&mut self, yes: bool) -> &mut Self {
        self.line_number = yes;
        self
    }

    /// Enables or disables column number output.
    pub fn column(&mut self, yes: bool) -> &mut Self {
        self.column = yes;
        self
    }

    /// Enables or disables byte offset output.
    pub fn byte_offset(&mut self, yes: bool) -> &mut Self {
        self.byte_offset = yes;
        self
    }

    /// Enables or disables only-matching mode (print only matched portions).
    pub fn only_matching(&mut self, yes: bool) -> &mut Self {
        self.only_matching = yes;
        self
    }

    /// Sets a replacement string for matched text.
    pub fn replacement(&mut self, rep: Vec<u8>) -> &mut Self {
        self.replacement = Some(rep);
        self
    }

    /// Sets the maximum number of columns to display per line.
    ///
    /// `None` means no limit.
    pub fn max_columns(&mut self, limit: Option<u64>) -> &mut Self {
        self.max_columns = limit;
        self
    }

    /// When `max_columns` is set and a line exceeds the limit, display a
    /// truncation preview instead of omitting the line entirely.
    pub fn max_columns_preview(&mut self, yes: bool) -> &mut Self {
        self.max_columns_preview = yes;
        self
    }

    /// Enables or disables trimming of leading ASCII whitespace from each line.
    pub fn trim_ascii(&mut self, yes: bool) -> &mut Self {
        self.trim_ascii = yes;
        self
    }

    /// Use NUL (`\0`) instead of newline as the path terminator.
    pub fn null_path(&mut self, yes: bool) -> &mut Self {
        self.null_path = yes;
        self
    }

    /// Enables or disables path display.
    pub fn path(&mut self, yes: bool) -> &mut Self {
        self.path = yes;
        self
    }

    /// Sets the hyperlink format for clickable paths in terminal output.
    pub fn hyperlink(&mut self, hl: Option<HyperlinkFormat>) -> &mut Self {
        self.hyperlink = hl;
        self
    }

    /// Sets the separator printed between non-contiguous context groups.
    pub fn context_separator(&mut self, sep: Vec<u8>) -> &mut Self {
        self.context_separator = sep;
        self
    }

    /// Sets the field separator for match lines (default: `:`).
    pub fn field_match_separator(&mut self, sep: Vec<u8>) -> &mut Self {
        self.field_match_separator = sep;
        self
    }

    /// Sets the field separator for context lines (default: `-`).
    pub fn field_context_separator(&mut self, sep: Vec<u8>) -> &mut Self {
        self.field_context_separator = sep;
        self
    }

    /// Builds a [`Standard`] printer that writes to `wtr`.
    pub fn build<W: WriteColor>(&self, wtr: W) -> Standard<W> {
        Standard {
            wtr,
            builder: self.clone(),
            has_written: false,
        }
    }
}

/// A standard text printer for search results.
///
/// This implements the classic grep output format: optional file path, line
/// number, and matched text on each line, with colored highlights.
pub struct Standard<W> {
    wtr: W,
    builder: StandardBuilder,
    has_written: bool,
}

impl<W: WriteColor> Standard<W> {
    /// Returns `true` if this printer has written any output.
    pub fn has_written(&self) -> bool {
        self.has_written
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    /// Creates a [`StandardSink`] for searching without a file path.
    pub fn sink<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
    ) -> StandardSink<'s, M, W> {
        StandardSink {
            printer: self,
            matcher,
            path: None,
            header_written: false,
            match_count: 0,
            needs_separator: false,
        }
    }

    /// Creates a [`StandardSink`] for searching with a file path label.
    pub fn sink_with_path<'s, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
        path: &'s Path,
    ) -> StandardSink<'s, M, W> {
        StandardSink {
            printer: self,
            matcher,
            path: Some(path.to_path_buf()),
            header_written: false,
            match_count: 0,
            needs_separator: false,
        }
    }
}

/// A [`Sink`] implementation for the standard text printer.
///
/// This is created via [`Standard::sink`] or [`Standard::sink_with_path`].
pub struct StandardSink<'s, M, W> {
    printer: &'s mut Standard<W>,
    matcher: &'s M,
    path: Option<PathBuf>,
    header_written: bool,
    match_count: u64,
    needs_separator: bool,
}

impl<'s, M: Matcher, W: WriteColor> StandardSink<'s, M, W> {
    /// Writes the path prefix for a line.
    fn write_path(&mut self, separator: &[u8]) -> Result<(), io::Error> {
        if let Some(ref path) = self.path {
            self.printer
                .wtr
                .set_color(&self.printer.builder.color_specs.path)?;
            write!(self.printer.wtr, "{}", path.display())?;
            self.printer.wtr.reset()?;
            self.printer.wtr.write_all(separator)?;
        }
        Ok(())
    }

    /// Writes a line number with the given separator.
    fn write_line_number(
        &mut self,
        line_num: u64,
        separator: &[u8],
    ) -> Result<(), io::Error> {
        self.printer
            .wtr
            .set_color(&self.printer.builder.color_specs.line)?;
        write!(self.printer.wtr, "{}", line_num)?;
        self.printer.wtr.reset()?;
        self.printer.wtr.write_all(separator)?;
        Ok(())
    }

    /// Writes a column number with the given separator.
    fn write_column(
        &mut self,
        col: u64,
        separator: &[u8],
    ) -> Result<(), io::Error> {
        self.printer
            .wtr
            .set_color(&self.printer.builder.color_specs.column)?;
        write!(self.printer.wtr, "{}", col)?;
        self.printer.wtr.reset()?;
        self.printer.wtr.write_all(separator)?;
        Ok(())
    }

    /// Writes a byte offset with the given separator.
    fn write_byte_offset(
        &mut self,
        offset: u64,
        separator: &[u8],
    ) -> Result<(), io::Error> {
        self.printer
            .wtr
            .set_color(&self.printer.builder.color_specs.line)?;
        write!(self.printer.wtr, "{}", offset)?;
        self.printer.wtr.reset()?;
        self.printer.wtr.write_all(separator)?;
        Ok(())
    }

    /// Writes a line of bytes, trimming leading whitespace if configured.
    fn write_line(&mut self, mut line: &[u8]) -> Result<(), io::Error> {
        if self.printer.builder.trim_ascii {
            line = trim_ascii_start(line);
        }
        self.printer.wtr.write_all(line)?;
        if !line.ends_with(b"\n") {
            writeln!(self.printer.wtr)?;
        }
        Ok(())
    }

    /// Writes a line with match highlighting.
    ///
    /// Finds all matches within the line and highlights them with the
    /// configured match color.
    fn write_highlighted_line(
        &mut self,
        line: &[u8],
    ) -> Result<(), io::Error> {
        let mut trimmed_line = line;
        if self.printer.builder.trim_ascii {
            trimmed_line = trim_ascii_start(line);
        }

        // Find all matches in this line.
        let mut matches: Vec<Match> = Vec::new();
        let _ = self.matcher.find_iter(trimmed_line, |m| {
            matches.push(m);
            true
        });

        if matches.is_empty() {
            // No matches to highlight, just write the line.
            self.printer.wtr.write_all(trimmed_line)?;
        } else {
            let mut last_end = 0;
            for m in &matches {
                // Write bytes before this match.
                if m.start() > last_end {
                    self.printer
                        .wtr
                        .write_all(&trimmed_line[last_end..m.start()])?;
                }
                // Write the matched bytes with color.
                self.printer
                    .wtr
                    .set_color(&self.printer.builder.color_specs.matched)?;
                self.printer
                    .wtr
                    .write_all(&trimmed_line[m.start()..m.end()])?;
                self.printer.wtr.reset()?;
                last_end = m.end();
            }
            // Write remaining bytes after the last match.
            if last_end < trimmed_line.len() {
                self.printer.wtr.write_all(&trimmed_line[last_end..])?;
            }
        }

        if !trimmed_line.ends_with(b"\n") {
            writeln!(self.printer.wtr)?;
        }
        Ok(())
    }

    /// Writes the context separator.
    fn write_context_separator(&mut self) -> Result<(), io::Error> {
        if !self.printer.builder.context_separator.is_empty() {
            let sep = self.printer.builder.context_separator.clone();
            self.printer.wtr.write_all(&sep)?;
            writeln!(self.printer.wtr)?;
        }
        Ok(())
    }
}

impl<'s, M: Matcher, W: WriteColor> Sink for StandardSink<'s, M, W> {
    type Error = io::Error;

    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        if self.printer.builder.heading && !self.header_written {
            if let Some(ref path) = self.path {
                self.printer
                    .wtr
                    .set_color(&self.printer.builder.color_specs.path)?;
                writeln!(self.printer.wtr, "{}", path.display())?;
                self.printer.wtr.reset()?;
                self.header_written = true;
                self.printer.has_written = true;
            }
        }
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.printer.has_written = true;
        self.match_count += 1;
        let line_bytes = mat.bytes();
        let sep = &self.printer.builder.field_match_separator.clone();

        // Print context separator if needed.
        if self.needs_separator {
            self.write_context_separator()?;
            self.needs_separator = false;
        }

        // Print path prefix (non-heading mode).
        if !self.printer.builder.heading && self.printer.builder.path {
            self.write_path(sep)?;
        }

        // Print line number.
        if let Some(line_num) = mat.line_number() {
            if self.printer.builder.line_number {
                self.write_line_number(line_num, sep)?;
            }
        }

        // Print column number (first match column).
        if self.printer.builder.column {
            if let Ok(Some(m)) = self.matcher.find(line_bytes) {
                self.write_column((m.start() + 1) as u64, sep)?;
            }
        }

        // Print byte offset.
        if self.printer.builder.byte_offset {
            self.write_byte_offset(mat.absolute_byte_offset(), sep)?;
        }

        // Handle max_columns truncation.
        if let Some(max) = self.printer.builder.max_columns {
            let line_len = line_bytes.len() as u64;
            if line_len > max {
                if self.printer.builder.max_columns_preview {
                    let preview = &line_bytes[..max as usize];
                    self.printer.wtr.write_all(preview)?;
                    writeln!(self.printer.wtr, " [... {} more bytes]", line_len - max)?;
                } else {
                    writeln!(
                        self.printer.wtr,
                        "[Omitted long line with {} bytes]",
                        line_len
                    )?;
                }
                return Ok(true);
            }
        }

        // Write the line with match highlighting.
        self.write_highlighted_line(line_bytes)?;

        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        self.printer.has_written = true;
        let line_bytes = ctx.bytes();
        let sep = &self.printer.builder.field_context_separator.clone();

        // Print context separator if needed.
        if self.needs_separator {
            self.write_context_separator()?;
            self.needs_separator = false;
        }

        // Print path prefix (non-heading mode).
        if !self.printer.builder.heading && self.printer.builder.path {
            self.write_path(sep)?;
        }

        // Print line number.
        if let Some(line_num) = ctx.line_number() {
            if self.printer.builder.line_number {
                self.write_line_number(line_num, sep)?;
            }
        }

        // Print byte offset.
        if self.printer.builder.byte_offset {
            self.write_byte_offset(ctx.absolute_byte_offset(), sep)?;
        }

        self.write_line(line_bytes)?;
        Ok(true)
    }

    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        // Mark that we need a separator before the next output.
        self.needs_separator = true;
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        _finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        Ok(())
    }
}

/// Trims leading ASCII whitespace from a byte slice.
fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() && bytes[i] != b'\n' {
        i += 1;
    }
    &bytes[i..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Matcher, NoCaptures, NoError};
    use grep_searcher::SearcherBuilder;
    use termcolor::NoColor;

    /// Simple literal matcher for testing.
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

    fn search_and_print(
        builder: &StandardBuilder,
        matcher: &TestMatcher,
        haystack: &[u8],
    ) -> String {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = builder.build(buf);
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink(matcher);
            searcher.search_slice(matcher, haystack, &mut sink).unwrap();
        }
        String::from_utf8(printer.get_mut().get_ref().clone()).unwrap()
    }

    fn search_and_print_with_path(
        builder: &StandardBuilder,
        matcher: &TestMatcher,
        haystack: &[u8],
        path: &Path,
    ) -> String {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = builder.build(buf);
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink_with_path(matcher, path);
            searcher.search_slice(matcher, haystack, &mut sink).unwrap();
        }
        String::from_utf8(printer.get_mut().get_ref().clone()).unwrap()
    }

    #[test]
    fn test_basic_match() {
        let builder = StandardBuilder::new();
        let matcher = TestMatcher::new("hello");
        let output = search_and_print(&builder, &matcher, b"hello world\n");
        assert!(output.contains("hello world"));
    }

    #[test]
    fn test_line_numbers() {
        let mut builder = StandardBuilder::new();
        builder.line_number(true).path(false);
        let matcher = TestMatcher::new("b");
        let output =
            search_and_print(&builder, &matcher, b"a\nb\nc\n");
        assert!(output.contains("2:"));
        assert!(output.contains("b\n"));
    }

    #[test]
    fn test_no_line_numbers() {
        let mut builder = StandardBuilder::new();
        builder.line_number(false).path(false);
        let matcher = TestMatcher::new("b");
        let output =
            search_and_print(&builder, &matcher, b"a\nb\nc\n");
        // Should not contain ":"
        assert!(!output.contains("2:"));
        assert!(output.contains("b\n"));
    }

    #[test]
    fn test_with_path() {
        let mut builder = StandardBuilder::new();
        builder.heading(false).path(true);
        let matcher = TestMatcher::new("test");
        let path = Path::new("foo.txt");
        let output = search_and_print_with_path(
            &builder,
            &matcher,
            b"test line\n",
            path,
        );
        assert!(output.contains("foo.txt:"));
    }

    #[test]
    fn test_heading_mode() {
        let mut builder = StandardBuilder::new();
        builder.heading(true).path(true);
        let matcher = TestMatcher::new("test");
        let path = Path::new("bar.rs");
        let output = search_and_print_with_path(
            &builder,
            &matcher,
            b"test line\n",
            path,
        );
        // In heading mode, path should be on its own line
        assert!(output.starts_with("bar.rs\n"));
    }

    #[test]
    fn test_no_match() {
        let builder = StandardBuilder::new();
        let matcher = TestMatcher::new("nonexistent");
        let output =
            search_and_print(&builder, &matcher, b"hello world\n");
        assert!(output.is_empty());
    }

    #[test]
    fn test_has_written() {
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = StandardBuilder::new().build(buf);
        assert!(!printer.has_written());

        let matcher = TestMatcher::new("hello");
        let mut searcher = SearcherBuilder::new().build();
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(&matcher, b"hello\n", &mut sink)
                .unwrap();
        }
        assert!(printer.has_written());
    }

    #[test]
    fn test_context_separator() {
        let mut builder = StandardBuilder::new();
        builder.path(false).line_number(true);
        let matcher = TestMatcher::new("match");
        let mut searcher = SearcherBuilder::new()
            .after_context(1)
            .build();
        let buf = NoColor::new(Vec::<u8>::new());
        let mut printer = builder.build(buf);
        {
            let mut sink = printer.sink(&matcher);
            searcher
                .search_slice(
                    &matcher,
                    b"match 1\nafter 1\nskip\nskip2\nmatch 2\nafter 2\n",
                    &mut sink,
                )
                .unwrap();
        }
        let output = String::from_utf8(printer.get_mut().get_ref().clone())
            .unwrap();
        assert!(output.contains("--\n"));
    }

    #[test]
    fn test_byte_offset() {
        let mut builder = StandardBuilder::new();
        builder.path(false).line_number(false).byte_offset(true);
        let matcher = TestMatcher::new("world");
        let output = search_and_print(
            &builder,
            &matcher,
            b"hello\nworld\n",
        );
        // "hello\n" is 6 bytes, so "world" starts at offset 6
        assert!(output.contains("6:"));
    }

    #[test]
    fn test_trim_ascii() {
        let mut builder = StandardBuilder::new();
        builder.path(false).line_number(false).trim_ascii(true);
        let matcher = TestMatcher::new("indented");
        let output = search_and_print(
            &builder,
            &matcher,
            b"    indented line\n",
        );
        assert!(output.starts_with("indented"));
    }

    #[test]
    fn test_max_columns() {
        let mut builder = StandardBuilder::new();
        builder.path(false).line_number(false).max_columns(Some(10));
        let matcher = TestMatcher::new("x");
        let output = search_and_print(
            &builder,
            &matcher,
            b"x this is a very long line that exceeds the limit\n",
        );
        assert!(output.contains("Omitted"));
    }

    #[test]
    fn test_max_columns_preview() {
        let mut builder = StandardBuilder::new();
        builder
            .path(false)
            .line_number(false)
            .max_columns(Some(10))
            .max_columns_preview(true);
        let matcher = TestMatcher::new("x");
        let output = search_and_print(
            &builder,
            &matcher,
            b"x this is a very long line that exceeds the limit\n",
        );
        assert!(output.contains("more bytes"));
    }
}
