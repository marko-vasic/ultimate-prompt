/*!
The [`Searcher`] and [`SearcherBuilder`] — the core search engine.

The searcher reads bytes from various sources (paths, files, readers, slices),
applies a [`Matcher`](grep_matcher::Matcher), and reports results via a
[`Sink`](crate::Sink).

# Binary detection

The searcher can detect binary content by scanning for NUL (`\x00`) bytes.
Three modes are supported:

- [`BinaryDetection::None`] — no detection.
- [`BinaryDetection::Quit`] — stop searching when a NUL byte is found.
- [`BinaryDetection::Convert`] — replace NUL bytes with the line terminator.

# Memory-mapped I/O

When searching files via [`Searcher::search_path`] or
[`Searcher::search_file`], the searcher can optionally memory-map the file
instead of reading it into a heap buffer. See [`MmapChoice`].
*/

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use grep_matcher::{LineTerminator, Matcher};

use crate::line_buffer::LineBuffer;
use crate::lines::LineIter;
use crate::sink::{Sink, SinkContext, SinkContextKind, SinkFinish, SinkMatch};

// ---------------------------------------------------------------------------
// Configuration enums
// ---------------------------------------------------------------------------

/// Controls binary-content detection during searching.
#[derive(Clone, Debug)]
pub enum BinaryDetection {
    /// No binary detection — search all content regardless.
    None,
    /// Quit searching as soon as a NUL byte (`\x00`) is found.
    Quit,
    /// Replace NUL bytes with the line terminator so they don't break
    /// line-oriented output.
    Convert,
}

impl Default for BinaryDetection {
    fn default() -> Self {
        BinaryDetection::Quit
    }
}

/// Controls whether memory-mapped I/O is used for file searches.
#[derive(Clone, Debug)]
pub enum MmapChoice {
    /// Automatically decide (currently equivalent to `Never`).
    Auto,
    /// Always use memory-mapped I/O.
    Always,
    /// Never use memory-mapped I/O.
    Never,
}

impl Default for MmapChoice {
    fn default() -> Self {
        MmapChoice::Never
    }
}

impl MmapChoice {
    /// Automatically decide whether to memory-map.
    pub fn auto() -> Self {
        MmapChoice::Auto
    }
    /// Always memory-map files.
    pub fn always() -> Self {
        MmapChoice::Always
    }
    /// Never memory-map files.
    pub fn never() -> Self {
        MmapChoice::Never
    }

    /// Returns `true` if memory-mapping should be attempted for the given
    /// file.
    fn should_mmap(&self, _file: &File) -> bool {
        match self {
            MmapChoice::Always => true,
            MmapChoice::Auto => false, // conservative default
            MmapChoice::Never => false,
        }
    }
}

/// Encoding for file content.
#[derive(Clone, Debug)]
pub enum Encoding {
    /// Automatically detect encoding (currently treats as UTF-8).
    Auto,
    /// A specific encoding from the `encoding_rs` crate.
    Named(&'static encoding_rs::Encoding),
}

impl Default for Encoding {
    fn default() -> Self {
        Encoding::Auto
    }
}

// ---------------------------------------------------------------------------
// SearcherBuilder
// ---------------------------------------------------------------------------

/// Internal configuration produced by the builder.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) line_terminator: LineTerminator,
    pub(crate) invert_match: bool,
    pub(crate) line_number: bool,
    pub(crate) multi_line: bool,
    pub(crate) binary_detection: BinaryDetection,
    pub(crate) encoding: Option<Encoding>,
    pub(crate) before_context: usize,
    pub(crate) after_context: usize,
    pub(crate) passthru: bool,
    pub(crate) memory_map: MmapChoice,
    pub(crate) stop_on_nonmatch: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            line_terminator: LineTerminator::default(),
            invert_match: false,
            line_number: true,
            multi_line: false,
            binary_detection: BinaryDetection::default(),
            encoding: None,
            before_context: 0,
            after_context: 0,
            passthru: false,
            memory_map: MmapChoice::default(),
            stop_on_nonmatch: false,
        }
    }
}

/// A builder for configuring and constructing a [`Searcher`].
#[derive(Clone, Debug)]
pub struct SearcherBuilder {
    config: Config,
}

impl SearcherBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        SearcherBuilder {
            config: Config::default(),
        }
    }

    /// Sets the line terminator.
    pub fn line_terminator(&mut self, lt: LineTerminator) -> &mut Self {
        self.config.line_terminator = lt;
        self
    }

    /// Enables or disables inverted matching (report non-matching lines).
    pub fn invert_match(&mut self, yes: bool) -> &mut Self {
        self.config.invert_match = yes;
        self
    }

    /// Enables or disables line number tracking.
    pub fn line_number(&mut self, yes: bool) -> &mut Self {
        self.config.line_number = yes;
        self
    }

    /// Enables or disables multi-line searching.
    pub fn multi_line(&mut self, yes: bool) -> &mut Self {
        self.config.multi_line = yes;
        self
    }

    /// Sets the binary detection strategy.
    pub fn binary_detection(&mut self, detection: BinaryDetection) -> &mut Self {
        self.config.binary_detection = detection;
        self
    }

    /// Sets the encoding. `None` means auto-detect (UTF-8).
    pub fn encoding(&mut self, enc: Option<Encoding>) -> &mut Self {
        self.config.encoding = enc;
        self
    }

    /// Sets the number of lines of before-context to show.
    pub fn before_context(&mut self, n: usize) -> &mut Self {
        self.config.before_context = n;
        self
    }

    /// Sets the number of lines of after-context to show.
    pub fn after_context(&mut self, n: usize) -> &mut Self {
        self.config.after_context = n;
        self
    }

    /// Enables or disables passthru mode (show all lines, marking matches).
    pub fn passthru(&mut self, yes: bool) -> &mut Self {
        self.config.passthru = yes;
        self
    }

    /// Sets the memory-map choice for file searches.
    pub fn memory_map(&mut self, choice: MmapChoice) -> &mut Self {
        self.config.memory_map = choice;
        self
    }

    /// If enabled, stop searching as soon as a non-matching line is found
    /// after the first match.
    pub fn stop_on_nonmatch(&mut self, yes: bool) -> &mut Self {
        self.config.stop_on_nonmatch = yes;
        self
    }

    /// Builds a [`Searcher`] with the current configuration.
    pub fn build(&self) -> Searcher {
        Searcher {
            config: self.config.clone(),
            line_buffer: LineBuffer::new(),
        }
    }
}

impl Default for SearcherBuilder {
    fn default() -> Self {
        SearcherBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Searcher
// ---------------------------------------------------------------------------

/// The main search engine.
///
/// A `Searcher` is configured via [`SearcherBuilder`] and executes searches
/// on various byte sources, reporting results through a [`Sink`].
///
/// The searcher is `Send` but not `Sync`, as searches require `&mut self`.
#[derive(Clone, Debug)]
pub struct Searcher {
    pub(crate) config: Config,
    line_buffer: LineBuffer,
}

impl Searcher {
    /// Creates a new `Searcher` with default settings.
    ///
    /// Equivalent to `SearcherBuilder::new().build()`.
    pub fn new() -> Self {
        SearcherBuilder::new().build()
    }

    /// Returns the configured line terminator.
    pub fn line_terminator(&self) -> LineTerminator {
        self.config.line_terminator
    }

    /// Returns `true` if inverted matching is enabled.
    pub fn invert_match(&self) -> bool {
        self.config.invert_match
    }

    /// Returns `true` if line number tracking is enabled.
    pub fn line_number(&self) -> bool {
        self.config.line_number
    }

    /// Returns `true` if multi-line mode is enabled.
    pub fn multi_line(&self) -> bool {
        self.config.multi_line
    }

    /// Returns the before-context line count.
    pub fn before_context(&self) -> usize {
        self.config.before_context
    }

    /// Returns the after-context line count.
    pub fn after_context(&self) -> usize {
        self.config.after_context
    }

    /// Returns `true` if passthru mode is enabled.
    pub fn passthru(&self) -> bool {
        self.config.passthru
    }

    /// Returns `true` if stop-on-nonmatch is enabled.
    pub fn stop_on_nonmatch(&self) -> bool {
        self.config.stop_on_nonmatch
    }

    /// Search the file at the given path.
    ///
    /// This opens the file and optionally memory-maps it. If memory mapping
    /// is not used (or fails), the file is read into an internal buffer and
    /// searched as a slice.
    pub fn search_path<M, S>(
        &mut self,
        matcher: &M,
        path: &Path,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        M::Error: std::fmt::Display + std::fmt::Debug + Send + 'static,
        S: Sink,
    {
        let file = File::open(path).map_err(S::Error::from)?;
        self.search_file(matcher, &file, sink)
    }

    /// Search an already-opened file.
    ///
    /// If memory mapping is configured (and succeeds), the file is searched
    /// as a memory-mapped slice. Otherwise it is read into an internal buffer.
    pub fn search_file<M, S>(
        &mut self,
        matcher: &M,
        file: &File,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        M::Error: std::fmt::Display + std::fmt::Debug + Send + 'static,
        S: Sink,
    {
        if self.config.memory_map.should_mmap(file) {
            // Safety: memory mapping a file is inherently unsafe because
            // the file can be modified by another process while we're
            // reading it. The caller assumes this risk by choosing
            // MmapChoice::Always.
            let mmap = unsafe { memmap2::Mmap::map(file) };
            match mmap {
                Ok(mmap) => return self.search_slice(matcher, &mmap, sink),
                Err(err) => {
                    log::debug!("mmap failed, falling back to read: {}", err);
                }
            }
        }
        // Fall back to reading the file into a buffer.
        self.search_reader(matcher, file, sink)
    }

    /// Search from a reader.
    ///
    /// The reader's entire contents are read into memory, then searched as
    /// a slice.
    pub fn search_reader<M, R, S>(
        &mut self,
        matcher: &M,
        rdr: R,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        M::Error: std::fmt::Display + std::fmt::Debug + Send + 'static,
        R: Read,
        S: Sink,
    {
        self.line_buffer
            .read_all(rdr)
            .map_err(S::Error::from)?;
        // Copy the buffer out so we can pass an immutable slice while
        // still allowing &mut self for the search.
        let buf = self.line_buffer.buffer().to_vec();
        self.search_slice(matcher, &buf, sink)
    }

    /// Search a byte slice.
    ///
    /// This is the core search implementation. All other search methods
    /// ultimately delegate here.
    pub fn search_slice<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        M::Error: std::fmt::Display + std::fmt::Debug + Send + 'static,
        S: Sink,
    {
        // Apply encoding transcoding if configured.
        let data = self.maybe_transcode(slice);
        let buf: &[u8] = data.as_deref().unwrap_or(slice);

        // Apply binary detection: Convert mode replaces NUL bytes.
        let owned_buf;
        let buf = match &self.config.binary_detection {
            BinaryDetection::Convert => {
                let lt = self.config.line_terminator.as_byte();
                let mut v = buf.to_vec();
                for b in v.iter_mut() {
                    if *b == 0 {
                        *b = lt;
                    }
                }
                owned_buf = v;
                &owned_buf
            }
            _ => buf,
        };

        if !sink.begin(self)? {
            sink.finish(
                self,
                &SinkFinish {
                    byte_count: buf.len() as u64,
                    binary_byte_offset: None,
                },
            )?;
            return Ok(());
        }

        let lt_byte = self.config.line_terminator.as_byte();
        let line_number_enabled = self.config.line_number;
        let invert = self.config.invert_match;
        let before_ctx = self.config.before_context;
        let after_ctx = self.config.after_context;
        let passthru = self.config.passthru;
        let stop_on_nonmatch = self.config.stop_on_nonmatch;
        let binary_quit = matches!(self.config.binary_detection, BinaryDetection::Quit);

        let mut line_number: u64 = 0; // 0 = not yet incremented
        let mut binary_byte_offset: Option<u64> = None;

        // Context tracking
        let mut before_buf: VecDeque<(usize, usize, u64)> = VecDeque::new(); // (start, end, line_no)
        let mut after_remaining: usize = 0;
        let mut has_matched = false; // whether we've emitted any match at all
        let mut last_was_match = false;
        // Track the line number of the last line in the last contiguous
        // output group, so we can emit context_break when there's a gap.
        let mut last_output_line: u64 = 0;

        for (line_start, line_end) in LineIter::new(buf, lt_byte) {
            line_number += 1;
            let line = &buf[line_start..line_end];

            // Binary detection: Quit mode
            if binary_quit {
                if let Some(rel) = memchr::memchr(0, line) {
                    binary_byte_offset = Some((line_start + rel) as u64);
                    break;
                }
            }

            // Check if this line matches.
            let is_match = match matcher.find(line) {
                Ok(Some(_)) => !invert,
                Ok(None) => invert,
                Err(err) => {
                    let io_err =
                        io::Error::new(io::ErrorKind::Other, err.to_string());
                    return Err(S::Error::from(io_err));
                }
            };

            let current_line_number = if line_number_enabled {
                Some(line_number)
            } else {
                None
            };

            if is_match {
                // Before emitting this match, check if we need a
                // context_break. A break is needed when:
                //  - We have previously output something
                //  - There is a gap between the last output line and the
                //    current before-context / match line.
                let first_ctx_line = if !before_buf.is_empty() {
                    before_buf.front().unwrap().2
                } else {
                    line_number
                };

                if has_matched && last_output_line + 1 < first_ctx_line {
                    if !sink.context_break(self)? {
                        break;
                    }
                }

                // Emit before-context lines.
                while let Some((ctx_start, ctx_end, ctx_ln)) =
                    before_buf.pop_front()
                {
                    let ctx = SinkContext {
                        kind: SinkContextKind::Before,
                        bytes: &buf[ctx_start..ctx_end],
                        absolute_byte_offset: ctx_start as u64,
                        line_number: if line_number_enabled {
                            Some(ctx_ln)
                        } else {
                            None
                        },
                    };
                    if !sink.context(self, &ctx)? {
                        // Drain remaining and stop.
                        before_buf.clear();
                        sink.finish(
                            self,
                            &SinkFinish {
                                byte_count: buf.len() as u64,
                                binary_byte_offset,
                            },
                        )?;
                        return Ok(());
                    }
                    last_output_line = ctx_ln;
                }

                // Emit the match.
                let sink_match = SinkMatch {
                    bytes: line,
                    absolute_byte_offset: line_start as u64,
                    line_number: current_line_number,
                    buffer: buf,
                    bytes_range_in_buffer: line_start..line_end,
                };
                if !sink.matched(self, &sink_match)? {
                    break;
                }

                has_matched = true;
                last_was_match = true;
                last_output_line = line_number;
                after_remaining = after_ctx;
            } else {
                // Non-matching line.
                if stop_on_nonmatch && has_matched {
                    break;
                }

                if after_remaining > 0 {
                    // This line is after-context for the previous match.
                    let ctx = SinkContext {
                        kind: SinkContextKind::After,
                        bytes: line,
                        absolute_byte_offset: line_start as u64,
                        line_number: current_line_number,
                    };
                    if !sink.context(self, &ctx)? {
                        break;
                    }
                    last_output_line = line_number;
                    after_remaining -= 1;
                    last_was_match = false;
                } else if passthru {
                    // In passthru mode, emit non-matching, non-context
                    // lines as "Other" context.
                    if has_matched && last_output_line + 1 < line_number {
                        if !sink.context_break(self)? {
                            break;
                        }
                    }
                    let ctx = SinkContext {
                        kind: SinkContextKind::Other,
                        bytes: line,
                        absolute_byte_offset: line_start as u64,
                        line_number: current_line_number,
                    };
                    if !sink.context(self, &ctx)? {
                        break;
                    }
                    last_output_line = line_number;
                    last_was_match = false;
                } else {
                    // Store as potential before-context for the next match.
                    if before_ctx > 0 {
                        if before_buf.len() >= before_ctx {
                            before_buf.pop_front();
                        }
                        before_buf.push_back((
                            line_start,
                            line_end,
                            line_number,
                        ));
                    }
                    last_was_match = false;
                }
            }
        }

        let _ = last_was_match; // suppress unused warning

        sink.finish(
            self,
            &SinkFinish {
                byte_count: buf.len() as u64,
                binary_byte_offset,
            },
        )?;

        Ok(())
    }

    /// Optionally transcode `slice` using the configured encoding.
    ///
    /// Returns `None` if no transcoding is needed (auto / UTF-8),
    /// or `Some(transcoded)` with the UTF-8 bytes.
    fn maybe_transcode(&self, slice: &[u8]) -> Option<Vec<u8>> {
        match &self.config.encoding {
            Some(Encoding::Named(encoding)) => {
                let (cow, _, _) = encoding.decode(slice);
                Some(cow.into_owned().into_bytes())
            }
            _ => None,
        }
    }
}

impl Default for Searcher {
    fn default() -> Self {
        Searcher::new()
    }
}

// Ensure Searcher is Send.
fn _assert_send<T: Send>() {}
fn _assert_searcher_send() {
    _assert_send::<Searcher>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::{SinkFinish, SinkMatch};
    use grep_matcher::{Match as GrepMatch, NoCaptures, NoError};

    /// A simple literal matcher for tests.
    struct LitMatcher(Vec<u8>);

    impl Matcher for LitMatcher {
        type Error = NoError;
        type Captures = NoCaptures;

        fn find_at(
            &self,
            haystack: &[u8],
            at: usize,
        ) -> Result<Option<GrepMatch>, NoError> {
            if at > haystack.len() {
                return Ok(None);
            }
            Ok(
                memchr::memmem::find(&haystack[at..], &self.0).map(|pos| {
                    let start = at + pos;
                    GrepMatch::new(start, start + self.0.len())
                }),
            )
        }

        fn new_captures(&self) -> Result<NoCaptures, NoError> {
            Ok(NoCaptures::new())
        }
    }

    #[test]
    fn test_search_slice_basic() {
        let matcher = LitMatcher(b"world".to_vec());
        let data = b"hello\nworld\nfoo\n";
        let mut matches: Vec<(u64, String)> = vec![];

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    matches.push((
                        m.line_number().unwrap(),
                        String::from_utf8_lossy(m.bytes()).into_owned(),
                    ));
                    Ok(true)
                },
            )
            .unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 2);
        assert_eq!(matches[0].1, "world\n");
    }

    #[test]
    fn test_search_slice_multiple_matches() {
        let matcher = LitMatcher(b"o".to_vec());
        let data = b"foo\nbar\nboo\n";
        let mut line_numbers: Vec<u64> = vec![];

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    line_numbers.push(m.line_number().unwrap());
                    Ok(true)
                },
            )
            .unwrap();

        assert_eq!(line_numbers, vec![1, 3]);
    }

    #[test]
    fn test_search_slice_invert() {
        let matcher = LitMatcher(b"foo".to_vec());
        let data = b"foo\nbar\nbaz\n";
        let mut lines: Vec<String> = vec![];

        let mut searcher = SearcherBuilder::new()
            .invert_match(true)
            .build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    lines.push(
                        String::from_utf8_lossy(m.bytes()).into_owned(),
                    );
                    Ok(true)
                },
            )
            .unwrap();

        assert_eq!(lines, vec!["bar\n", "baz\n"]);
    }

    #[test]
    fn test_search_slice_no_line_numbers() {
        let matcher = LitMatcher(b"x".to_vec());
        let data = b"x\ny\nx\n";
        let mut has_none = true;

        let mut searcher = SearcherBuilder::new()
            .line_number(false)
            .build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    if m.line_number().is_some() {
                        has_none = false;
                    }
                    Ok(true)
                },
            )
            .unwrap();

        assert!(has_none);
    }

    #[test]
    fn test_search_slice_early_stop() {
        let matcher = LitMatcher(b"x".to_vec());
        let data = b"x\nx\nx\n";
        let mut count = 0;

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, _m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    count += 1;
                    Ok(count < 2) // stop after 2nd match
                },
            )
            .unwrap();

        assert_eq!(count, 2);
    }

    #[test]
    fn test_search_slice_binary_quit() {
        let matcher = LitMatcher(b"hello".to_vec());
        let data = b"hello\n\x00binary\nworld\n";
        let mut count = 0;
        let mut finish_binary_offset: Option<u64> = None;

        struct TestSink<'a> {
            count: &'a mut usize,
            binary_offset: &'a mut Option<u64>,
        }
        impl Sink for TestSink<'_> {
            type Error = io::Error;
            fn matched(
                &mut self,
                _: &Searcher,
                _: &SinkMatch<'_>,
            ) -> Result<bool, io::Error> {
                *self.count += 1;
                Ok(true)
            }
            fn finish(
                &mut self,
                _: &Searcher,
                f: &SinkFinish,
            ) -> Result<(), io::Error> {
                *self.binary_offset = f.binary_byte_offset();
                Ok(())
            }
        }

        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::Quit)
            .build();
        let mut test_sink = TestSink {
            count: &mut count,
            binary_offset: &mut finish_binary_offset,
        };
        searcher
            .search_slice(
                &matcher,
                data,
                &mut test_sink,
            )
            .unwrap();

        assert_eq!(count, 1); // only "hello" matched before binary
        assert_eq!(finish_binary_offset, Some(6)); // NUL at offset 6
    }

    #[test]
    fn test_search_slice_binary_convert() {
        let matcher = LitMatcher(b"hello".to_vec());
        // NUL byte on the "hello" line — it should be converted to \n
        let data = b"he\x00llo\nworld\n";
        let mut count = 0;

        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::Convert)
            .build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, _m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    count += 1;
                    Ok(true)
                },
            )
            .unwrap();

        // After conversion, "he\x00llo" becomes "he\nllo" which no longer
        // matches "hello". Only "world" wouldn't match either. So 0 matches.
        assert_eq!(count, 0);
    }

    #[test]
    fn test_search_slice_buffer_field() {
        let matcher = LitMatcher(b"bar".to_vec());
        let data = b"foo\nbar\nbaz\n";

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    // buffer should be the full data slice
                    assert_eq!(m.buffer(), data);
                    // bytes_range_in_buffer should point to "bar\n"
                    let range = m.bytes_range_in_buffer();
                    assert_eq!(&m.buffer()[range], b"bar\n");
                    Ok(true)
                },
            )
            .unwrap();
    }

    #[test]
    fn test_search_reader() {
        let matcher = LitMatcher(b"needle".to_vec());
        let data = b"haystack\nneedle\nmore\n";
        let mut lines: Vec<String> = vec![];

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_reader(
                &matcher,
                &data[..],
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    lines.push(
                        String::from_utf8_lossy(m.bytes()).into_owned(),
                    );
                    Ok(true)
                },
            )
            .unwrap();

        assert_eq!(lines, vec!["needle\n"]);
    }

    #[test]
    fn test_search_slice_no_trailing_newline() {
        let matcher = LitMatcher(b"end".to_vec());
        let data = b"start\nend";
        let mut found = false;

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    assert_eq!(m.bytes(), b"end");
                    assert_eq!(m.line_number(), Some(2));
                    found = true;
                    Ok(true)
                },
            )
            .unwrap();

        assert!(found);
    }

    #[test]
    fn test_search_empty() {
        let matcher = LitMatcher(b"x".to_vec());
        let data = b"";
        let mut count = 0;

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut |_s: &Searcher, _m: &SinkMatch<'_>| -> Result<bool, io::Error> {
                    count += 1;
                    Ok(true)
                },
            )
            .unwrap();

        assert_eq!(count, 0);
    }

    #[test]
    fn test_context_after() {
        let matcher = LitMatcher(b"match".to_vec());
        let data = b"before\nmatch\nafter1\nafter2\nfar\n";
        let mut context_lines: Vec<(String, SinkContextKind)> = vec![];

        struct CtxSink<'a> {
            context_lines: &'a mut Vec<(String, SinkContextKind)>,
        }
        impl Sink for CtxSink<'_> {
            type Error = io::Error;
            fn matched(
                &mut self,
                _: &Searcher,
                _: &SinkMatch<'_>,
            ) -> Result<bool, io::Error> {
                Ok(true)
            }
            fn context(
                &mut self,
                _: &Searcher,
                ctx: &SinkContext<'_>,
            ) -> Result<bool, io::Error> {
                self.context_lines.push((
                    String::from_utf8_lossy(ctx.bytes()).into_owned(),
                    ctx.kind().clone(),
                ));
                Ok(true)
            }
        }

        let mut searcher = SearcherBuilder::new()
            .after_context(2)
            .build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut CtxSink {
                    context_lines: &mut context_lines,
                },
            )
            .unwrap();

        assert_eq!(context_lines.len(), 2);
        assert_eq!(context_lines[0].0, "after1\n");
        assert_eq!(context_lines[0].1, SinkContextKind::After);
        assert_eq!(context_lines[1].0, "after2\n");
        assert_eq!(context_lines[1].1, SinkContextKind::After);
    }

    #[test]
    fn test_context_before() {
        let matcher = LitMatcher(b"match".to_vec());
        let data = b"far\nbefore1\nbefore2\nmatch\nafter\n";
        let mut context_lines: Vec<(String, SinkContextKind)> = vec![];

        struct CtxSink<'a> {
            context_lines: &'a mut Vec<(String, SinkContextKind)>,
        }
        impl Sink for CtxSink<'_> {
            type Error = io::Error;
            fn matched(
                &mut self,
                _: &Searcher,
                _: &SinkMatch<'_>,
            ) -> Result<bool, io::Error> {
                Ok(true)
            }
            fn context(
                &mut self,
                _: &Searcher,
                ctx: &SinkContext<'_>,
            ) -> Result<bool, io::Error> {
                self.context_lines.push((
                    String::from_utf8_lossy(ctx.bytes()).into_owned(),
                    ctx.kind().clone(),
                ));
                Ok(true)
            }
        }

        let mut searcher = SearcherBuilder::new()
            .before_context(2)
            .build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut CtxSink {
                    context_lines: &mut context_lines,
                },
            )
            .unwrap();

        assert_eq!(context_lines.len(), 2);
        assert_eq!(context_lines[0].0, "before1\n");
        assert_eq!(context_lines[0].1, SinkContextKind::Before);
        assert_eq!(context_lines[1].0, "before2\n");
        assert_eq!(context_lines[1].1, SinkContextKind::Before);
    }

    #[test]
    fn test_context_break() {
        let matcher = LitMatcher(b"match".to_vec());
        let data = b"match\na\nb\nc\nmatch\n";
        let mut break_count = 0;

        struct BreakSink<'a> {
            breaks: &'a mut usize,
        }
        impl Sink for BreakSink<'_> {
            type Error = io::Error;
            fn matched(
                &mut self,
                _: &Searcher,
                _: &SinkMatch<'_>,
            ) -> Result<bool, io::Error> {
                Ok(true)
            }
            fn context_break(
                &mut self,
                _: &Searcher,
            ) -> Result<bool, io::Error> {
                *self.breaks += 1;
                Ok(true)
            }
        }

        let mut searcher = SearcherBuilder::new().build();
        searcher
            .search_slice(
                &matcher,
                data,
                &mut BreakSink {
                    breaks: &mut break_count,
                },
            )
            .unwrap();

        assert_eq!(break_count, 1); // one break between the two matches
    }

    #[test]
    fn test_searcher_accessors() {
        let searcher = SearcherBuilder::new()
            .line_terminator(LineTerminator::CRLF)
            .invert_match(true)
            .line_number(false)
            .multi_line(true)
            .before_context(3)
            .after_context(5)
            .passthru(true)
            .stop_on_nonmatch(true)
            .build();

        assert_eq!(searcher.line_terminator(), LineTerminator::CRLF);
        assert!(searcher.invert_match());
        assert!(!searcher.line_number());
        assert!(searcher.multi_line());
        assert_eq!(searcher.before_context(), 3);
        assert_eq!(searcher.after_context(), 5);
        assert!(searcher.passthru());
        assert!(searcher.stop_on_nonmatch());
    }
}
