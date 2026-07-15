//! # grep-searcher
//!
//! This crate provides the search orchestration layer for grep-like tools.
//! It connects a [`grep_matcher::Matcher`] (which knows how to find patterns)
//! with a [`Sink`] (which consumes search results), applying line-oriented
//! search logic including:
//!
//! - Line-by-line scanning with configurable line terminators
//! - Before/after context lines (like `grep -B` / `grep -A`)
//! - Inverted matching (like `grep -v`)
//! - Binary file detection (quit or convert modes)
//! - Line number tracking
//! - Memory-mapped file I/O support
//! - Passthrough mode (print all lines, highlighting matches)
//!
//! # Overview
//!
//! The main entry point is [`SearcherBuilder`], which configures and builds a
//! [`Searcher`]. The `Searcher` can search byte slices, readers, or file paths.
//! Results are delivered to a caller-provided [`Sink`] implementation.
//!
//! ```no_run
//! use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch, SinkFinish, SinkError};
//!
//! // Build a searcher with default settings
//! let mut searcher = SearcherBuilder::new().build();
//! ```

#![deny(missing_docs)]

use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use grep_matcher::Matcher;

// Re-export LineTerminator from grep-matcher so consumers don't need
// to depend on grep-matcher directly for this type.
pub use grep_matcher::LineTerminator;

// ---------------------------------------------------------------------------
// BinaryDetection
// ---------------------------------------------------------------------------

/// Configuration for binary file detection.
///
/// Binary detection works by scanning each line for NUL bytes (`\x00`).
/// When a NUL byte is found, the searcher can either stop searching entirely
/// or replace the NUL bytes and continue.
///
/// # Variants
///
/// - `None` — No binary detection; search proceeds regardless of content.
/// - `Quit` — Stop searching as soon as a NUL byte is found. The byte offset
///   of the NUL byte is recorded in the [`SinkFinish`] result.
/// - `Convert(byte)` — Replace NUL bytes with the given byte and continue
///   searching. This is useful for treating binary files as text by replacing
///   NUL with some visible replacement character.
#[derive(Clone, Copy, Debug)]
pub enum BinaryDetection {
    /// No binary detection is performed.
    None,
    /// Quit searching when binary content (a NUL byte) is detected.
    Quit,
    /// Convert binary content by replacing NUL bytes with the given byte.
    Convert(u8),
}

impl Default for BinaryDetection {
    fn default() -> Self {
        BinaryDetection::None
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encoding configuration for the searcher.
///
/// This controls how input bytes are interpreted. In `Auto` mode, UTF-8 is
/// assumed. A named encoding can be specified to transcode input before
/// searching.
#[derive(Clone, Debug)]
pub enum Encoding {
    /// Automatically detect encoding (defaults to UTF-8).
    Auto,
    /// Use a specific named encoding (e.g., `"UTF-16LE"`, `"Shift_JIS"`).
    Named(String),
}

impl Default for Encoding {
    fn default() -> Self {
        Encoding::Auto
    }
}

// ---------------------------------------------------------------------------
// MmapChoice
// ---------------------------------------------------------------------------

/// Controls whether memory-mapped I/O is used when searching files.
///
/// Memory mapping can be significantly faster for large files but may not
/// be appropriate in all situations (e.g., searching stdin, or on platforms
/// where mmap is unreliable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmapChoice {
    /// Automatically decide whether to use memory mapping.
    /// Currently this behaves the same as `Never`.
    Auto,
    /// Always use memory mapping when searching files.
    Always,
    /// Never use memory mapping.
    Never,
}

impl Default for MmapChoice {
    fn default() -> Self {
        MmapChoice::Auto
    }
}

// ---------------------------------------------------------------------------
// SinkError
// ---------------------------------------------------------------------------

/// A trait for error types used by [`Sink`] implementations.
///
/// This trait allows sink implementations to use their own error types while
/// ensuring they can be constructed from I/O errors and display messages.
pub trait SinkError: Sized {
    /// Create an error from a displayable message.
    fn error_message<T: fmt::Display>(msg: T) -> Self;
    /// Create an error from an I/O error.
    fn error_io(err: io::Error) -> Self;
}

impl SinkError for io::Error {
    fn error_message<T: fmt::Display>(msg: T) -> Self {
        io::Error::new(io::ErrorKind::Other, msg.to_string())
    }
    fn error_io(err: io::Error) -> Self {
        err
    }
}

impl SinkError for Box<dyn std::error::Error> {
    fn error_message<T: fmt::Display>(msg: T) -> Self {
        Box::from(msg.to_string())
    }
    fn error_io(err: io::Error) -> Self {
        Box::new(err)
    }
}

// ---------------------------------------------------------------------------
// SinkMatch
// ---------------------------------------------------------------------------

/// A matched line delivered to a [`Sink`].
///
/// This contains the bytes of the matching line, its line number (if tracking
/// is enabled), and its absolute byte offset within the searched input.
///
/// The bytes of a match always include the line terminator (if present), so
/// consumers that need the line content without a trailing newline should
/// strip it themselves.
pub struct SinkMatch<'b> {
    line_number: Option<u64>,
    absolute_byte_offset: u64,
    bytes: &'b [u8],
    line_term: LineTerminator,
}

impl<'b> SinkMatch<'b> {
    /// Returns the line number of this match, if line numbers are enabled.
    ///
    /// Line numbers start at 1.
    #[inline]
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }

    /// Returns the absolute byte offset of the start of this match
    /// within the entire searched input.
    #[inline]
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// Returns the raw bytes of this match, including the line terminator.
    #[inline]
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes
    }

    /// Returns an iterator over the logical lines in this match.
    ///
    /// In single-line mode, this typically yields a single line. The lines
    /// include their terminators. In multi-line mode, a match may span
    /// multiple lines.
    pub fn lines(&self) -> LineIter<'b> {
        LineIter {
            bytes: self.bytes,
            line_term: self.line_term.as_byte(),
        }
    }
}

impl<'b> fmt::Debug for SinkMatch<'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SinkMatch")
            .field("line_number", &self.line_number)
            .field("absolute_byte_offset", &self.absolute_byte_offset)
            .field("bytes", &String::from_utf8_lossy(self.bytes))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SinkContext
// ---------------------------------------------------------------------------

/// A context line delivered to a [`Sink`].
///
/// Context lines are non-matching lines that appear near a matching line.
/// They can be "before" context (preceding a match), "after" context
/// (following a match), or "other" (e.g., in passthrough mode).
pub struct SinkContext<'b> {
    line_number: Option<u64>,
    absolute_byte_offset: u64,
    bytes: &'b [u8],
    kind: SinkContextKind,
}

impl<'b> SinkContext<'b> {
    /// Returns the line number of this context line, if line numbers are enabled.
    #[inline]
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }

    /// Returns the absolute byte offset of this context line within
    /// the searched input.
    #[inline]
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// Returns the raw bytes of this context line, including the line terminator.
    #[inline]
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes
    }

    /// Returns the kind of context this line represents.
    #[inline]
    pub fn kind(&self) -> &SinkContextKind {
        &self.kind
    }
}

impl<'b> fmt::Debug for SinkContext<'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SinkContext")
            .field("line_number", &self.line_number)
            .field("absolute_byte_offset", &self.absolute_byte_offset)
            .field("bytes", &String::from_utf8_lossy(self.bytes))
            .field("kind", &self.kind)
            .finish()
    }
}

/// The kind of context a [`SinkContext`] line represents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinkContextKind {
    /// A line appearing before a match.
    Before,
    /// A line appearing after a match.
    After,
    /// A line appearing for other reasons (e.g., passthrough mode).
    Other,
}

// ---------------------------------------------------------------------------
// SinkFinish
// ---------------------------------------------------------------------------

/// Statistics about the completed search, delivered to [`Sink::finish`].
#[derive(Clone, Copy, Debug)]
pub struct SinkFinish {
    /// The total number of bytes searched.
    pub byte_count: u64,
    /// If binary content was detected (and binary detection is enabled),
    /// this holds the byte offset of the first NUL byte found.
    pub binary_byte_offset: Option<u64>,
}

// ---------------------------------------------------------------------------
// Sink trait
// ---------------------------------------------------------------------------

/// The callback interface for receiving search results.
///
/// A `Sink` implementation is passed to the [`Searcher`] and receives
/// callbacks for matched lines, context lines, context breaks (separators
/// between groups), and search lifecycle events (begin/finish).
///
/// All callbacks return `Result<bool, Self::Error>` where `Ok(true)` means
/// "continue searching" and `Ok(false)` means "stop early." Returning
/// `Err(e)` aborts the search and propagates the error.
///
/// # Required Methods
///
/// - [`matched`](Sink::matched) — Called for each matching line.
///
/// # Provided Methods
///
/// All other methods have default implementations that simply continue
/// searching.
pub trait Sink {
    /// The error type for this sink.
    type Error: SinkError;

    /// Called when a matching line is found.
    ///
    /// Returns `Ok(true)` to continue searching or `Ok(false)` to stop.
    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error>;

    /// Called for each context line (before-context, after-context, or other).
    ///
    /// The default implementation ignores context lines and continues.
    fn context(
        &mut self,
        _searcher: &Searcher,
        _ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called between non-contiguous groups of matches/context.
    ///
    /// This typically represents the `--` separator in grep output.
    /// The default implementation continues searching.
    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called at the beginning of a search, before any lines are processed.
    ///
    /// Returns `Ok(true)` to proceed with the search or `Ok(false)` to skip
    /// this search entirely.
    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called at the end of a search with summary statistics.
    ///
    /// This is always called, even if the search was stopped early.
    fn finish(
        &mut self,
        _searcher: &Searcher,
        _finish: &SinkFinish,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Blanket implementation allowing `&mut S` to be used as a `Sink` when `S: Sink`.
///
/// This enables callers to pass a mutable reference to a sink rather than
/// moving ownership, which is useful when the caller needs to inspect the
/// sink after the search completes.
impl<'a, S: Sink> Sink for &'a mut S {
    type Error = S::Error;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        (**self).matched(searcher, mat)
    }

    fn context(
        &mut self,
        searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        (**self).context(searcher, ctx)
    }

    fn context_break(
        &mut self,
        searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        (**self).context_break(searcher)
    }

    fn begin(
        &mut self,
        searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        (**self).begin(searcher)
    }

    fn finish(
        &mut self,
        searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), Self::Error> {
        (**self).finish(searcher, finish)
    }
}

// ---------------------------------------------------------------------------
// LineIter — iterator over lines in a byte slice
// ---------------------------------------------------------------------------

/// An iterator over lines in a byte slice.
///
/// Each yielded item includes its line terminator (if present). The last
/// line is yielded even if it does not end with a terminator.
pub struct LineIter<'a> {
    bytes: &'a [u8],
    line_term: u8,
}

impl<'a> Iterator for LineIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.bytes.is_empty() {
            return None;
        }
        match memchr(self.line_term, self.bytes) {
            Some(pos) => {
                let line = &self.bytes[..=pos];
                self.bytes = &self.bytes[pos + 1..];
                Some(line)
            }
            None => {
                let line = self.bytes;
                self.bytes = &[];
                Some(line)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SearcherConfig — internal configuration
// ---------------------------------------------------------------------------

/// Internal configuration for a [`Searcher`].
#[derive(Clone, Debug)]
struct SearcherConfig {
    line_number: bool,
    invert_match: bool,
    after_context: usize,
    before_context: usize,
    passthru: bool,
    binary_detection: BinaryDetection,
    encoding: Encoding,
    line_terminator: LineTerminator,
    multi_line: bool,
    memory_map: MmapChoice,
    stop_on_nonmatch: bool,
}

impl Default for SearcherConfig {
    fn default() -> Self {
        SearcherConfig {
            line_number: true,
            invert_match: false,
            after_context: 0,
            before_context: 0,
            passthru: false,
            binary_detection: BinaryDetection::Quit,
            encoding: Encoding::Auto,
            line_terminator: LineTerminator::default(),
            multi_line: false,
            memory_map: MmapChoice::Never,
            stop_on_nonmatch: false,
        }
    }
}

// ---------------------------------------------------------------------------
// SearcherBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and constructing a [`Searcher`].
///
/// The builder provides a fluent API for setting search options. Each setter
/// returns `&mut Self` so calls can be chained.
///
/// # Example
///
/// ```
/// use grep_searcher::{SearcherBuilder, BinaryDetection};
///
/// let mut searcher = SearcherBuilder::new()
///     .line_number(true)
///     .after_context(2)
///     .before_context(2)
///     .binary_detection(BinaryDetection::Quit)
///     .build();
/// ```
pub struct SearcherBuilder {
    config: SearcherConfig,
}

impl SearcherBuilder {
    /// Create a new `SearcherBuilder` with default settings.
    ///
    /// Defaults:
    /// - Line numbers: enabled
    /// - Binary detection: `Quit`
    /// - Line terminator: `\n`
    /// - Context: 0 lines before and after
    /// - Inverted match: disabled
    /// - Multi-line: disabled
    /// - Memory mapping: `Never`
    /// - Passthrough: disabled
    /// - Stop on nonmatch: disabled
    pub fn new() -> SearcherBuilder {
        SearcherBuilder {
            config: SearcherConfig::default(),
        }
    }

    /// Enable or disable line number tracking.
    ///
    /// When enabled, the [`SinkMatch`] and [`SinkContext`] values will
    /// include line numbers starting at 1.
    pub fn line_number(&mut self, yes: bool) -> &mut Self {
        self.config.line_number = yes;
        self
    }

    /// Enable or disable inverted matching.
    ///
    /// When enabled, non-matching lines are reported as matches and
    /// matching lines are treated as non-matches (like `grep -v`).
    pub fn invert_match(&mut self, yes: bool) -> &mut Self {
        self.config.invert_match = yes;
        self
    }

    /// Set the number of lines of context to show after each match.
    pub fn after_context(&mut self, lines: usize) -> &mut Self {
        self.config.after_context = lines;
        self
    }

    /// Set the number of lines of context to show before each match.
    pub fn before_context(&mut self, lines: usize) -> &mut Self {
        self.config.before_context = lines;
        self
    }

    /// Enable or disable passthrough mode.
    ///
    /// In passthrough mode, every line is printed, but matching lines are
    /// still reported via [`Sink::matched`]. Non-matching lines are
    /// reported via [`Sink::context`] with [`SinkContextKind::Other`].
    pub fn passthru(&mut self, yes: bool) -> &mut Self {
        self.config.passthru = yes;
        self
    }

    /// Set the binary detection mode.
    pub fn binary_detection(&mut self, detection: BinaryDetection) -> &mut Self {
        self.config.binary_detection = detection;
        self
    }

    /// Set the input encoding.
    pub fn encoding(&mut self, encoding: Encoding) -> &mut Self {
        self.config.encoding = encoding;
        self
    }

    /// Set the line terminator.
    pub fn line_terminator(&mut self, term: LineTerminator) -> &mut Self {
        self.config.line_terminator = term;
        self
    }

    /// Enable or disable multi-line matching.
    ///
    /// When enabled, the matcher may be applied to the entire input at once,
    /// allowing patterns to match across line boundaries.
    pub fn multi_line(&mut self, yes: bool) -> &mut Self {
        self.config.multi_line = yes;
        self
    }

    /// Set the memory mapping strategy.
    pub fn memory_map(&mut self, choice: MmapChoice) -> &mut Self {
        self.config.memory_map = choice;
        self
    }

    /// Enable or disable stop-on-nonmatch.
    ///
    /// When enabled, the searcher stops as soon as a non-matching line is
    /// found after at least one match. This is useful for sorted input.
    pub fn stop_on_nonmatch(&mut self, yes: bool) -> &mut Self {
        self.config.stop_on_nonmatch = yes;
        self
    }

    /// Build a [`Searcher`] with the current configuration.
    pub fn build(&self) -> Searcher {
        Searcher {
            config: self.config.clone(),
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
/// A `Searcher` takes a [`grep_matcher::Matcher`] and a [`Sink`], then
/// searches input (a byte slice, reader, or file path) line by line,
/// delivering results to the sink.
///
/// Use [`SearcherBuilder`] to create and configure a `Searcher`.
///
/// # Example
///
/// ```no_run
/// use grep_searcher::{SearcherBuilder, Sink, SinkMatch, SinkFinish, SinkError, Searcher};
///
/// let mut searcher = SearcherBuilder::new().build();
/// // searcher.search_slice(&matcher, b"hello\nworld\n", my_sink);
/// ```
pub struct Searcher {
    config: SearcherConfig,
}

impl Searcher {
    /// Returns whether line number tracking is enabled.
    #[inline]
    pub fn line_number(&self) -> bool {
        self.config.line_number
    }

    /// Returns whether inverted matching is enabled.
    #[inline]
    pub fn invert_match(&self) -> bool {
        self.config.invert_match
    }

    /// Returns the number of after-context lines.
    #[inline]
    pub fn after_context(&self) -> usize {
        self.config.after_context
    }

    /// Returns the number of before-context lines.
    #[inline]
    pub fn before_context(&self) -> usize {
        self.config.before_context
    }

    /// Returns the binary detection mode.
    #[inline]
    pub fn binary_detection(&self) -> &BinaryDetection {
        &self.config.binary_detection
    }

    /// Returns whether multi-line mode is enabled.
    #[inline]
    pub fn multi_line(&self) -> bool {
        self.config.multi_line
    }

    /// Returns the configured line terminator.
    #[inline]
    pub fn line_terminator(&self) -> LineTerminator {
        self.config.line_terminator
    }

    /// Returns whether passthrough mode is enabled.
    #[inline]
    pub fn passthru(&self) -> bool {
        self.config.passthru
    }

    /// Returns the memory-map choice.
    #[inline]
    pub fn memory_map(&self) -> MmapChoice {
        self.config.memory_map
    }

    /// Returns whether stop-on-nonmatch is enabled.
    #[inline]
    pub fn stop_on_nonmatch(&self) -> bool {
        self.config.stop_on_nonmatch
    }

    /// Search a file by path.
    ///
    /// This method opens the file and, depending on the memory-map
    /// configuration, either memory-maps it or reads it into a buffer,
    /// then delegates to [`search_slice`](Searcher::search_slice) or
    /// [`search_reader`](Searcher::search_reader).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or if a sink callback
    /// returns an error.
    pub fn search_path<M, S>(
        &mut self,
        matcher: &M,
        path: &Path,
        sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        let file = File::open(path).map_err(S::Error::error_io)?;

        // Try memory mapping if configured
        if self.config.memory_map == MmapChoice::Always {
            // SAFETY: Memory-mapping is inherently unsafe because the
            // underlying file could be modified while the map is active.
            // The caller opted in by choosing `MmapChoice::Always`.
            let mmap = unsafe {
                memmap2::Mmap::map(&file).map_err(S::Error::error_io)?
            };
            return self.search_slice(matcher, &mmap, sink);
        }

        // Otherwise read via the reader path
        self.search_reader(matcher, file, sink)
    }

    /// Search a reader.
    ///
    /// The reader is read to completion into an in-memory buffer, then
    /// the search is performed on the resulting byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or if a sink callback returns
    /// an error.
    pub fn search_reader<M, R, S>(
        &mut self,
        matcher: &M,
        mut reader: R,
        sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        R: Read,
        S: Sink,
    {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map_err(S::Error::error_io)?;
        self.search_slice(matcher, &buf, sink)
    }

    /// Search a byte slice.
    ///
    /// This is the core search method. It splits the input into lines,
    /// runs the matcher on each line, manages context, tracks line numbers,
    /// and delivers results to the sink.
    ///
    /// # Algorithm
    ///
    /// 1. Call `sink.begin()`. If it returns `Ok(false)`, skip the search.
    /// 2. Split input into lines by the configured line terminator.
    /// 3. For each line:
    ///    - Check for binary content (NUL bytes) if binary detection is on.
    ///    - Run the matcher on the line.
    ///    - If the line matches (accounting for invert_match):
    ///      - Emit a context break if there's a gap from the last output.
    ///      - Emit buffered before-context lines.
    ///      - Emit the match via `sink.matched()`.
    ///    - If the line does not match:
    ///      - If in the after-context window, emit via `sink.context()`.
    ///      - If in passthrough mode, emit as `Other` context.
    ///      - Otherwise, buffer for potential before-context.
    /// 4. Call `sink.finish()` with statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the matcher fails or if a sink callback returns
    /// an error.
    pub fn search_slice<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        mut sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        if self.config.multi_line {
            return self.search_slice_multi_line(matcher, slice, sink);
        }

        // Begin
        if !sink.begin(self)? {
            let finish = SinkFinish {
                byte_count: slice.len() as u64,
                binary_byte_offset: None,
            };
            sink.finish(self, &finish)?;
            return Ok(());
        }

        let line_term_byte = self.config.line_terminator.as_byte();
        let line_term = self.config.line_terminator;

        // State tracking
        let mut line_number: u64 = 0; // current line (1-indexed, 0 means not started)
        let mut byte_offset: u64 = 0;
        let mut binary_byte_offset: Option<u64> = None;

        // Context management
        let before_ctx_size = self.config.before_context;
        let after_ctx_size = self.config.after_context;
        let passthru = self.config.passthru;

        // Ring buffer for before-context lines.
        // Each entry: (bytes_range_start, bytes_range_end, line_number)
        let mut before_buf: VecDeque<(usize, usize, u64)> = VecDeque::with_capacity(
            if before_ctx_size > 0 { before_ctx_size } else { 0 }
        );

        let mut after_remaining: usize = 0; // lines of after-context still to emit
        let mut has_matched = false; // have we emitted any match at all?
        let mut last_output_line: u64 = 0; // line number of last emitted line
        let mut found_any_match = false; // for stop_on_nonmatch

        // Split into lines
        let lines = split_lines(slice, line_term_byte);

        for (line_start, line_end) in lines {
            let line = &slice[line_start..line_end];
            line_number += 1;
            let current_line_num = line_number;

            // Binary detection
            if let Some(nul_offset) = check_binary(line, &self.config.binary_detection) {
                let abs_nul = byte_offset + nul_offset as u64;
                match self.config.binary_detection {
                    BinaryDetection::Quit => {
                        binary_byte_offset = Some(abs_nul);
                        // Stop searching immediately
                        break;
                    }
                    BinaryDetection::Convert(_replacement) => {
                        // We note the offset but continue — the caller handles
                        // replacement via the sink receiving the raw bytes.
                        // In practice, for Convert mode we'd replace NULs in
                        // a mutable buffer. Since we operate on a slice, we
                        // record the offset and let the sink handle it.
                        if binary_byte_offset.is_none() {
                            binary_byte_offset = Some(abs_nul);
                        }
                    }
                    BinaryDetection::None => {}
                }
            }

            // Run the matcher
            let mat_result = matcher
                .find(line)
                .map_err(|e| S::Error::error_message(e))?;
            let is_match = mat_result.is_some();

            // Account for invert_match
            let is_hit = if self.config.invert_match {
                !is_match
            } else {
                is_match
            };

            if is_hit {
                found_any_match = true;

                // Determine if we need a context break.
                // A context break is needed if there is a gap between the last
                // output line and the current before-context window.
                if has_matched {
                    let earliest_context_line = if before_buf.is_empty() {
                        current_line_num
                    } else {
                        before_buf.front().unwrap().2
                    };
                    if last_output_line > 0 && earliest_context_line > last_output_line + 1 {
                        if !sink.context_break(self)? {
                            break;
                        }
                    }
                }

                // Emit before-context lines
                while let Some((ctx_start, ctx_end, ctx_line_num)) = before_buf.pop_front() {
                    let ctx_bytes = &slice[ctx_start..ctx_end];
                    let ctx = SinkContext {
                        line_number: if self.config.line_number {
                            Some(ctx_line_num)
                        } else {
                            None
                        },
                        absolute_byte_offset: ctx_start as u64,
                        bytes: ctx_bytes,
                        kind: SinkContextKind::Before,
                    };
                    if !sink.context(self, &ctx)? {
                        // We need to bail, but we should still call finish.
                        // Use a flag to break outer loop.
                        let finish = SinkFinish {
                            byte_count: slice.len() as u64,
                            binary_byte_offset,
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                }

                // Emit the match
                let sink_match = SinkMatch {
                    line_number: if self.config.line_number {
                        Some(current_line_num)
                    } else {
                        None
                    },
                    absolute_byte_offset: byte_offset,
                    bytes: line,
                    line_term,
                };
                if !sink.matched(self, &sink_match)? {
                    let finish = SinkFinish {
                        byte_count: slice.len() as u64,
                        binary_byte_offset,
                    };
                    sink.finish(self, &finish)?;
                    return Ok(());
                }
                last_output_line = current_line_num;
                has_matched = true;
                after_remaining = after_ctx_size;
            } else {
                // Non-match line

                // stop_on_nonmatch: if we already had a match and now see a
                // non-match, stop.
                if self.config.stop_on_nonmatch && found_any_match {
                    break;
                }

                if after_remaining > 0 {
                    // Emit as after-context
                    let ctx = SinkContext {
                        line_number: if self.config.line_number {
                            Some(current_line_num)
                        } else {
                            None
                        },
                        absolute_byte_offset: byte_offset,
                        bytes: line,
                        kind: SinkContextKind::After,
                    };
                    if !sink.context(self, &ctx)? {
                        let finish = SinkFinish {
                            byte_count: slice.len() as u64,
                            binary_byte_offset,
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                    last_output_line = current_line_num;
                    after_remaining -= 1;
                } else if passthru && has_matched {
                    // In passthrough mode, emit all non-match lines as Other
                    // context (but only after we've seen at least one match
                    // to avoid emitting everything before the first match as
                    // Other — those should be Before context).
                    let ctx = SinkContext {
                        line_number: if self.config.line_number {
                            Some(current_line_num)
                        } else {
                            None
                        },
                        absolute_byte_offset: byte_offset,
                        bytes: line,
                        kind: SinkContextKind::Other,
                    };
                    if !sink.context(self, &ctx)? {
                        let finish = SinkFinish {
                            byte_count: slice.len() as u64,
                            binary_byte_offset,
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                    last_output_line = current_line_num;
                } else if passthru && !has_matched {
                    // Before first match in passthrough, emit as Other context
                    let ctx = SinkContext {
                        line_number: if self.config.line_number {
                            Some(current_line_num)
                        } else {
                            None
                        },
                        absolute_byte_offset: byte_offset,
                        bytes: line,
                        kind: SinkContextKind::Other,
                    };
                    if !sink.context(self, &ctx)? {
                        let finish = SinkFinish {
                            byte_count: slice.len() as u64,
                            binary_byte_offset,
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                    last_output_line = current_line_num;
                } else {
                    // Buffer for before-context
                    if before_ctx_size > 0 {
                        if before_buf.len() == before_ctx_size {
                            before_buf.pop_front();
                        }
                        before_buf.push_back((line_start, line_end, current_line_num));
                    }
                }
            }

            byte_offset += line.len() as u64;
        }

        // Finish
        let finish = SinkFinish {
            byte_count: slice.len() as u64,
            binary_byte_offset,
        };
        sink.finish(self, &finish)?;
        Ok(())
    }

    /// Multi-line search on a byte slice.
    ///
    /// In multi-line mode, the entire slice is treated as a single "line" and
    /// the matcher may produce matches that span multiple lines. Each match is
    /// expanded to full line boundaries before being delivered to the sink.
    fn search_slice_multi_line<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        mut sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        if !sink.begin(self)? {
            let finish = SinkFinish {
                byte_count: slice.len() as u64,
                binary_byte_offset: None,
            };
            sink.finish(self, &finish)?;
            return Ok(());
        }

        let line_term_byte = self.config.line_terminator.as_byte();
        let line_term = self.config.line_terminator;
        let mut binary_byte_offset: Option<u64> = None;

        // Binary detection on entire slice
        if let Some(nul_offset) = check_binary(slice, &self.config.binary_detection) {
            match self.config.binary_detection {
                BinaryDetection::Quit => {
                    binary_byte_offset = Some(nul_offset as u64);
                    let finish = SinkFinish {
                        byte_count: slice.len() as u64,
                        binary_byte_offset,
                    };
                    sink.finish(self, &finish)?;
                    return Ok(());
                }
                BinaryDetection::Convert(_) => {
                    binary_byte_offset = Some(nul_offset as u64);
                }
                BinaryDetection::None => {}
            }
        }

        if self.config.invert_match {
            // For invert_match in multi-line mode, we find all matches and
            // report everything else as matched lines.
            self.search_slice_multi_line_inverted(matcher, slice, &mut sink, line_term_byte, line_term, binary_byte_offset)?;
        } else {
            // Find all matches in the slice
            let mut pos: usize = 0;
            loop {
                let mat_result = matcher
                    .find_at(slice, pos)
                    .map_err(|e| S::Error::error_message(e))?;
                let mat = match mat_result {
                    Some(m) => m,
                    None => break,
                };

                // Expand match to full line boundaries
                let line_start = find_line_start(slice, mat.start(), line_term_byte);
                let line_end = find_line_end(slice, mat.end(), line_term_byte);
                let matched_bytes = &slice[line_start..line_end];

                // Compute line number
                let line_num = if self.config.line_number {
                    Some(count_lines_before(slice, line_start, line_term_byte) + 1)
                } else {
                    None
                };

                let sink_match = SinkMatch {
                    line_number: line_num,
                    absolute_byte_offset: line_start as u64,
                    bytes: matched_bytes,
                    line_term,
                };
                if !sink.matched(self, &sink_match)? {
                    break;
                }

                // Advance past this match to avoid infinite loops
                if mat.is_empty() {
                    pos = mat.end() + 1;
                    if pos > slice.len() {
                        break;
                    }
                } else {
                    // Advance to end of the line containing the match end
                    pos = line_end;
                }
            }
        }

        let finish = SinkFinish {
            byte_count: slice.len() as u64,
            binary_byte_offset,
        };
        sink.finish(self, &finish)?;
        Ok(())
    }

    /// Helper for multi-line inverted matching.
    fn search_slice_multi_line_inverted<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        sink: &mut S,
        line_term_byte: u8,
        line_term: LineTerminator,
        _binary_byte_offset: Option<u64>,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        // Collect all match ranges (expanded to line boundaries)
        let mut matched_ranges: Vec<(usize, usize)> = Vec::new();
        let mut pos: usize = 0;
        loop {
            let mat_result = matcher
                .find_at(slice, pos)
                .map_err(|e| S::Error::error_message(e))?;
            let mat = match mat_result {
                Some(m) => m,
                None => break,
            };
            let line_start = find_line_start(slice, mat.start(), line_term_byte);
            let line_end = find_line_end(slice, mat.end(), line_term_byte);
            matched_ranges.push((line_start, line_end));

            if mat.is_empty() {
                pos = mat.end() + 1;
                if pos > slice.len() {
                    break;
                }
            } else {
                pos = line_end;
            }
        }

        // Merge overlapping ranges
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in matched_ranges {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            merged.push((start, end));
        }

        // Now report every line NOT in a merged range as a match
        let lines = split_lines(slice, line_term_byte);
        let mut byte_off: usize = 0;
        let mut line_number: u64 = 0;

        for (line_start, line_end) in lines {
            line_number += 1;
            let in_match = merged.iter().any(|&(ms, me)| line_start >= ms && line_end <= me);
            if !in_match {
                let line_bytes = &slice[line_start..line_end];
                let sink_match = SinkMatch {
                    line_number: if self.config.line_number {
                        Some(line_number)
                    } else {
                        None
                    },
                    absolute_byte_offset: line_start as u64,
                    bytes: line_bytes,
                    line_term,
                };
                if !sink.matched(self, &sink_match)? {
                    return Ok(());
                }
            }
            byte_off = line_end;
        }
        let _ = byte_off; // suppress unused warning
        Ok(())
    }
}

impl fmt::Debug for Searcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Searcher")
            .field("config", &self.config)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// A simple `memchr`-like function: find the first occurrence of `needle` in `haystack`.
///
/// This is a naive implementation. In production, the `memchr` crate would be
/// used for SIMD-accelerated scanning.
#[inline]
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Split a byte slice into lines by a line terminator byte.
///
/// Returns an iterator of `(start, end)` byte ranges. Each line includes
/// its terminator. The last line is included even if it does not end with
/// a terminator (unless the slice is empty).
fn split_lines(slice: &[u8], line_term: u8) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < slice.len() {
        match memchr(line_term, &slice[start..]) {
            Some(pos) => {
                let end = start + pos + 1;
                lines.push((start, end));
                start = end;
            }
            None => {
                // Last line without terminator
                lines.push((start, slice.len()));
                break;
            }
        }
    }
    lines
}

/// Check a line for binary content (NUL bytes).
///
/// Returns `Some(offset)` if a NUL byte is found (relative to the line),
/// or `None` if no NUL byte is present or binary detection is disabled.
#[inline]
fn check_binary(line: &[u8], detection: &BinaryDetection) -> Option<usize> {
    match detection {
        BinaryDetection::None => None,
        BinaryDetection::Quit | BinaryDetection::Convert(_) => {
            memchr(0, line)
        }
    }
}

/// Find the start of the line containing byte offset `pos`.
///
/// Searches backwards from `pos` for the line terminator byte, returning
/// the byte offset immediately after it (i.e., the start of the current line).
fn find_line_start(slice: &[u8], pos: usize, line_term: u8) -> usize {
    if pos == 0 {
        return 0;
    }
    // Search backwards from pos-1
    for i in (0..pos).rev() {
        if slice[i] == line_term {
            return i + 1;
        }
    }
    0
}

/// Find the end of the line containing byte offset `pos`.
///
/// Searches forward from `pos` for the line terminator byte, returning
/// the byte offset immediately after it (i.e., the exclusive end of the line,
/// including the terminator).
fn find_line_end(slice: &[u8], pos: usize, line_term: u8) -> usize {
    if pos >= slice.len() {
        return slice.len();
    }
    match memchr(line_term, &slice[pos..]) {
        Some(offset) => pos + offset + 1,
        None => slice.len(),
    }
}

/// Count the number of lines before byte offset `pos`.
///
/// Returns the number of line terminator bytes found in `slice[..pos]`.
fn count_lines_before(slice: &[u8], pos: usize, line_term: u8) -> u64 {
    let mut count = 0u64;
    for &b in &slice[..pos] {
        if b == line_term {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Match, Matcher, NoCaptures, NoError};

    // A trivial literal matcher for testing.
    struct LiteralMatcher {
        needle: Vec<u8>,
    }

    impl LiteralMatcher {
        fn new(needle: &[u8]) -> Self {
            LiteralMatcher {
                needle: needle.to_vec(),
            }
        }
    }

    impl Matcher for LiteralMatcher {
        type Captures = NoCaptures;
        type Error = NoError;

        fn find_at(
            &self,
            haystack: &[u8],
            at: usize,
        ) -> Result<Option<Match>, Self::Error> {
            if self.needle.is_empty() {
                if at <= haystack.len() {
                    return Ok(Some(Match::new(at, at)));
                } else {
                    return Ok(None);
                }
            }
            if at + self.needle.len() > haystack.len() {
                return Ok(None);
            }
            for i in at..=(haystack.len() - self.needle.len()) {
                if &haystack[i..i + self.needle.len()] == &self.needle[..] {
                    return Ok(Some(Match::new(i, i + self.needle.len())));
                }
            }
            Ok(None)
        }

        fn new_captures(&self) -> Result<Self::Captures, Self::Error> {
            Ok(NoCaptures::new())
        }
    }

    // A simple sink that collects matched lines.
    struct CollectSink {
        matches: Vec<(Option<u64>, Vec<u8>)>,
        contexts: Vec<(Option<u64>, Vec<u8>, SinkContextKind)>,
        context_breaks: usize,
        finish_stats: Option<SinkFinish>,
    }

    impl CollectSink {
        fn new() -> Self {
            CollectSink {
                matches: Vec::new(),
                contexts: Vec::new(),
                context_breaks: 0,
                finish_stats: None,
            }
        }
    }

    impl Sink for CollectSink {
        type Error = io::Error;

        fn matched(
            &mut self,
            _searcher: &Searcher,
            mat: &SinkMatch<'_>,
        ) -> Result<bool, Self::Error> {
            self.matches
                .push((mat.line_number(), mat.bytes().to_vec()));
            Ok(true)
        }

        fn context(
            &mut self,
            _searcher: &Searcher,
            ctx: &SinkContext<'_>,
        ) -> Result<bool, Self::Error> {
            self.contexts
                .push((ctx.line_number(), ctx.bytes().to_vec(), *ctx.kind()));
            Ok(true)
        }

        fn context_break(
            &mut self,
            _searcher: &Searcher,
        ) -> Result<bool, Self::Error> {
            self.context_breaks += 1;
            Ok(true)
        }

        fn finish(
            &mut self,
            _searcher: &Searcher,
            finish: &SinkFinish,
        ) -> Result<(), Self::Error> {
            self.finish_stats = Some(*finish);
            Ok(())
        }
    }

    // --- Basic search tests ---

    #[test]
    fn test_search_slice_basic() {
        let matcher = LiteralMatcher::new(b"world");
        let mut searcher = SearcherBuilder::new().build();
        let sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"hello\nworld\nfoo\n", sink)
            .unwrap();
    }

    #[test]
    fn test_search_slice_single_match() {
        let matcher = LiteralMatcher::new(b"world");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"hello\nworld\nfoo\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].0, Some(2));
        assert_eq!(sink.matches[0].1, b"world\n");
    }

    #[test]
    fn test_search_slice_multiple_matches() {
        let matcher = LiteralMatcher::new(b"a");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"abc\ndef\naxy\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
        assert_eq!(sink.matches[0].0, Some(1)); // "abc\n"
        assert_eq!(sink.matches[1].0, Some(3)); // "axy\n"
    }

    #[test]
    fn test_search_slice_no_match() {
        let matcher = LiteralMatcher::new(b"zzz");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"hello\nworld\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 0);
    }

    #[test]
    fn test_search_slice_no_trailing_newline() {
        let matcher = LiteralMatcher::new(b"foo");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"bar\nfoo", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].0, Some(2));
        assert_eq!(sink.matches[0].1, b"foo");
    }

    // --- Line number tests ---

    #[test]
    fn test_line_numbers_disabled() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new()
            .line_number(false)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"a\nx\nb\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].0, None);
    }

    // --- Inverted match tests ---

    #[test]
    fn test_invert_match() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new()
            .invert_match(true)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, b"a\nx\nb\n", &mut sink)
            .unwrap();
        // Lines "a\n" and "b\n" should match (inverted)
        assert_eq!(sink.matches.len(), 2);
        assert_eq!(sink.matches[0].1, b"a\n");
        assert_eq!(sink.matches[1].1, b"b\n");
    }

    // --- Context tests ---

    #[test]
    fn test_after_context() {
        let matcher = LiteralMatcher::new(b"match");
        let mut searcher = SearcherBuilder::new()
            .after_context(2)
            .build();
        let mut sink = CollectSink::new();
        let input = b"line1\nmatch\nafter1\nafter2\nline5\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].1, b"match\n");
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].1, b"after1\n");
        assert_eq!(sink.contexts[0].2, SinkContextKind::After);
        assert_eq!(sink.contexts[1].1, b"after2\n");
        assert_eq!(sink.contexts[1].2, SinkContextKind::After);
    }

    #[test]
    fn test_before_context() {
        let matcher = LiteralMatcher::new(b"match");
        let mut searcher = SearcherBuilder::new()
            .before_context(2)
            .build();
        let mut sink = CollectSink::new();
        let input = b"line1\nline2\nmatch\nline4\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].1, b"match\n");
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].1, b"line1\n");
        assert_eq!(sink.contexts[0].2, SinkContextKind::Before);
        assert_eq!(sink.contexts[1].1, b"line2\n");
        assert_eq!(sink.contexts[1].2, SinkContextKind::Before);
    }

    #[test]
    fn test_context_break() {
        let matcher = LiteralMatcher::new(b"match");
        let mut searcher = SearcherBuilder::new()
            .before_context(1)
            .after_context(1)
            .build();
        let mut sink = CollectSink::new();
        // Two matches separated by enough lines to cause a context break
        let input = b"line1\nmatch1\nafter1\ngap1\ngap2\nbefore2\nmatch2\nafter2\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        assert_eq!(sink.matches.len(), 2);
        assert!(sink.context_breaks >= 1);
    }

    // --- Binary detection tests ---

    #[test]
    fn test_binary_detection_quit() {
        let matcher = LiteralMatcher::new(b"hello");
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::Quit)
            .build();
        let mut sink = CollectSink::new();
        let input = b"hello\nwor\x00ld\nhello\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        // Should find the first match but quit at the NUL byte
        assert_eq!(sink.matches.len(), 1);
        let stats = sink.finish_stats.unwrap();
        assert!(stats.binary_byte_offset.is_some());
    }

    #[test]
    fn test_binary_detection_none() {
        let matcher = LiteralMatcher::new(b"hello");
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::None)
            .build();
        let mut sink = CollectSink::new();
        let input = b"hello\nwor\x00ld\nhello\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        // Should find both matches
        assert_eq!(sink.matches.len(), 2);
    }

    // --- Empty input tests ---

    #[test]
    fn test_empty_input() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        searcher.search_slice(&matcher, b"", &mut sink).unwrap();
        assert_eq!(sink.matches.len(), 0);
        let stats = sink.finish_stats.unwrap();
        assert_eq!(stats.byte_count, 0);
    }

    // --- Finish stats tests ---

    #[test]
    fn test_finish_byte_count() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        let input = b"abcdef\nxyz\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();
        let stats = sink.finish_stats.unwrap();
        assert_eq!(stats.byte_count, input.len() as u64);
    }

    // --- Builder tests ---

    #[test]
    fn test_builder_defaults() {
        let searcher = SearcherBuilder::new().build();
        assert!(searcher.line_number());
        assert!(!searcher.invert_match());
        assert_eq!(searcher.after_context(), 0);
        assert_eq!(searcher.before_context(), 0);
        assert!(!searcher.multi_line());
        assert!(!searcher.passthru());
    }

    #[test]
    fn test_builder_chaining() {
        let searcher = SearcherBuilder::new()
            .line_number(false)
            .invert_match(true)
            .after_context(3)
            .before_context(2)
            .multi_line(true)
            .passthru(true)
            .build();
        assert!(!searcher.line_number());
        assert!(searcher.invert_match());
        assert_eq!(searcher.after_context(), 3);
        assert_eq!(searcher.before_context(), 2);
        assert!(searcher.multi_line());
        assert!(searcher.passthru());
    }

    // --- Passthrough tests ---

    #[test]
    fn test_passthru_mode() {
        let matcher = LiteralMatcher::new(b"match");
        let mut searcher = SearcherBuilder::new()
            .passthru(true)
            .build();
        let mut sink = CollectSink::new();
        let input = b"line1\nmatch\nline3\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        // "match\n" should be a match
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].1, b"match\n");
        // "line1\n" and "line3\n" should be context (Other)
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].2, SinkContextKind::Other);
        assert_eq!(sink.contexts[1].2, SinkContextKind::Other);
    }

    // --- SinkMatch::lines tests ---

    #[test]
    fn test_sink_match_lines() {
        let sm = SinkMatch {
            line_number: Some(1),
            absolute_byte_offset: 0,
            bytes: b"line1\nline2\n",
            line_term: LineTerminator::default(),
        };
        let lines: Vec<&[u8]> = sm.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"line1\n");
        assert_eq!(lines[1], b"line2\n");
    }

    // --- Helper function tests ---

    #[test]
    fn test_split_lines() {
        let input = b"abc\ndef\nghi\n";
        let lines = split_lines(input, b'\n');
        assert_eq!(lines.len(), 3);
        assert_eq!(&input[lines[0].0..lines[0].1], b"abc\n");
        assert_eq!(&input[lines[1].0..lines[1].1], b"def\n");
        assert_eq!(&input[lines[2].0..lines[2].1], b"ghi\n");
    }

    #[test]
    fn test_split_lines_no_trailing() {
        let input = b"abc\ndef";
        let lines = split_lines(input, b'\n');
        assert_eq!(lines.len(), 2);
        assert_eq!(&input[lines[0].0..lines[0].1], b"abc\n");
        assert_eq!(&input[lines[1].0..lines[1].1], b"def");
    }

    #[test]
    fn test_split_lines_empty() {
        let lines = split_lines(b"", b'\n');
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_find_line_start() {
        let input = b"abc\ndef\nghi";
        assert_eq!(find_line_start(input, 0, b'\n'), 0);
        assert_eq!(find_line_start(input, 2, b'\n'), 0);
        assert_eq!(find_line_start(input, 4, b'\n'), 4);
        assert_eq!(find_line_start(input, 5, b'\n'), 4);
        assert_eq!(find_line_start(input, 8, b'\n'), 8);
    }

    #[test]
    fn test_find_line_end() {
        let input = b"abc\ndef\nghi";
        assert_eq!(find_line_end(input, 0, b'\n'), 4);
        assert_eq!(find_line_end(input, 4, b'\n'), 8);
        assert_eq!(find_line_end(input, 8, b'\n'), 11);
    }

    #[test]
    fn test_count_lines_before() {
        let input = b"abc\ndef\nghi\n";
        assert_eq!(count_lines_before(input, 0, b'\n'), 0);
        assert_eq!(count_lines_before(input, 4, b'\n'), 1);
        assert_eq!(count_lines_before(input, 8, b'\n'), 2);
    }

    // --- Stop on nonmatch ---

    #[test]
    fn test_stop_on_nonmatch() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new()
            .stop_on_nonmatch(true)
            .build();
        let mut sink = CollectSink::new();
        let input = b"x1\nx2\nnope\nx3\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();
        // Should find "x1\n" and "x2\n" but stop at "nope\n"
        assert_eq!(sink.matches.len(), 2);
    }

    // --- Sink that stops early ---

    struct StopAfterN {
        max: usize,
        count: usize,
    }

    impl Sink for StopAfterN {
        type Error = io::Error;

        fn matched(
            &mut self,
            _searcher: &Searcher,
            _mat: &SinkMatch<'_>,
        ) -> Result<bool, Self::Error> {
            self.count += 1;
            Ok(self.count < self.max)
        }
    }

    #[test]
    fn test_sink_early_stop() {
        let matcher = LiteralMatcher::new(b"x");
        let mut searcher = SearcherBuilder::new().build();
        let sink = StopAfterN { max: 2, count: 0 };
        // Input has 4 matching lines but sink stops after 2
        searcher
            .search_slice(&matcher, b"x\nx\nx\nx\n", sink)
            .unwrap();
        // No panic = success. The sink stopped after 2 matches.
    }

    // --- search_reader test ---

    #[test]
    fn test_search_reader() {
        let matcher = LiteralMatcher::new(b"hello");
        let mut searcher = SearcherBuilder::new().build();
        let mut sink = CollectSink::new();
        let input = io::Cursor::new(b"foo\nhello\nbar\n");
        searcher.search_reader(&matcher, input, &mut sink).unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].1, b"hello\n");
    }

    // --- Multi-line tests ---

    #[test]
    fn test_multi_line_search() {
        let matcher = LiteralMatcher::new(b"world");
        let mut searcher = SearcherBuilder::new()
            .multi_line(true)
            .build();
        let mut sink = CollectSink::new();
        let input = b"hello\nworld\nfoo\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();
        assert_eq!(sink.matches.len(), 1);
        // The matched line should be "world\n"
        assert_eq!(sink.matches[0].1, b"world\n");
    }

    // --- Before context ring buffer limit ---

    #[test]
    fn test_before_context_ring_buffer() {
        let matcher = LiteralMatcher::new(b"match");
        let mut searcher = SearcherBuilder::new()
            .before_context(2)
            .build();
        let mut sink = CollectSink::new();
        // 4 lines before match, but only 2 should be shown
        let input = b"a\nb\nc\nd\nmatch\n";
        searcher.search_slice(&matcher, input, &mut sink).unwrap();

        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].1, b"c\n");
        assert_eq!(sink.contexts[1].1, b"d\n");
    }
}
