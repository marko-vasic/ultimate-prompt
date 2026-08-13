/*!
The `grep-searcher` crate provides the search orchestration engine for ripgrep.

It drives the actual search by reading input (files, readers, slices), splitting
it into lines, testing each line against a [`grep_matcher::Matcher`], and
reporting results through the [`Sink`] trait callback interface.

# Key types

- [`Searcher`] — the main search driver, created via [`SearcherBuilder`].
- [`Sink`] — a trait that consumers implement to receive match/context events.
- [`SinkMatch`], [`SinkContext`], [`SinkFinish`] — data passed to `Sink` callbacks.
- [`BinaryDetection`] — controls how binary files are handled.
- [`MemoryMap`] — controls memory-mapped file access.
- [`SinkError`] — trait for constructing errors inside `Sink` implementations.
*/

use std::cmp;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::Path;

use bstr::ByteSlice;
use encoding_rs_io::DecodeReaderBytesBuilder;
use grep_matcher::{LineTerminator, Matcher};

// ---------------------------------------------------------------------------
// SinkError trait
// ---------------------------------------------------------------------------

/// A trait for error types used by [`Sink`] implementations.
///
/// This trait enables the searcher to construct errors when I/O failures or
/// matcher errors occur during a search.
pub trait SinkError: Sized {
    /// Construct an error from a human-readable message.
    fn error_message<T: fmt::Display>(message: T) -> Self;

    /// Construct an error from an I/O error.
    fn error_io(err: io::Error) -> Self;
}

impl SinkError for io::Error {
    fn error_message<T: fmt::Display>(message: T) -> io::Error {
        io::Error::new(io::ErrorKind::Other, message.to_string())
    }

    fn error_io(err: io::Error) -> io::Error {
        err
    }
}

// ---------------------------------------------------------------------------
// BinaryDetection
// ---------------------------------------------------------------------------

/// Configuration for how binary data (NUL bytes) should be handled during
/// a search.
#[derive(Clone, Debug)]
pub enum BinaryDetection {
    /// No binary detection. NUL bytes are treated as ordinary data.
    None,
    /// Quit searching as soon as a NUL byte is found. The byte offset of
    /// the NUL byte is reported via [`Sink::binary_data`].
    Quit,
    /// Convert NUL bytes to the given replacement byte (typically the line
    /// terminator) before searching.
    Convert(u8),
}

impl BinaryDetection {
    /// Create a `BinaryDetection` that does nothing.
    pub fn none() -> BinaryDetection {
        BinaryDetection::None
    }

    /// Create a `BinaryDetection` that quits on the first NUL byte.
    pub fn quit() -> BinaryDetection {
        BinaryDetection::Quit
    }

    /// Create a `BinaryDetection` that converts NUL bytes to `byte`.
    pub fn convert(byte: u8) -> BinaryDetection {
        BinaryDetection::Convert(byte)
    }

    /// Returns true if binary detection is disabled.
    fn is_none(&self) -> bool {
        matches!(self, BinaryDetection::None)
    }
}

impl Default for BinaryDetection {
    fn default() -> BinaryDetection {
        BinaryDetection::None
    }
}

// ---------------------------------------------------------------------------
// MemoryMap
// ---------------------------------------------------------------------------

/// Memory mapping strategy for file searching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryMap {
    /// Automatically decide whether to use memory mapping based on heuristics
    /// (e.g., file size).
    Auto,
    /// Always use memory mapping when searching files.
    Always,
    /// Never use memory mapping.
    Never,
}

impl Default for MemoryMap {
    fn default() -> MemoryMap {
        MemoryMap::Never
    }
}

// ---------------------------------------------------------------------------
// SinkMatch
// ---------------------------------------------------------------------------

/// Data associated with a single matching line (or lines) reported to a
/// [`Sink`].
pub struct SinkMatch<'b> {
    bytes: &'b [u8],
    absolute_byte_offset: u64,
    line_number: Option<u64>,
    buffer: &'b [u8],
    bytes_range_in_buffer: Range<usize>,
}

impl<'b> SinkMatch<'b> {
    /// The matched line bytes, including the line terminator (if present).
    #[inline]
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes
    }

    /// The absolute byte offset of this match from the beginning of the
    /// input.
    #[inline]
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// The 1-based line number of this match, if line numbers are being
    /// tracked.
    #[inline]
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }

    /// The entire buffer contents (may contain more than just the matched
    /// line).
    #[inline]
    pub fn buffer(&self) -> &'b [u8] {
        self.buffer
    }

    /// The range within [`buffer()`](SinkMatch::buffer) that corresponds
    /// to [`bytes()`](SinkMatch::bytes).
    #[inline]
    pub fn bytes_range_in_buffer(&self) -> Range<usize> {
        self.bytes_range_in_buffer.clone()
    }
}

impl<'b> fmt::Debug for SinkMatch<'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SinkMatch")
            .field("bytes", &self.bytes.as_bstr())
            .field("absolute_byte_offset", &self.absolute_byte_offset)
            .field("line_number", &self.line_number)
            .field("bytes_range_in_buffer", &self.bytes_range_in_buffer)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SinkContext
// ---------------------------------------------------------------------------

/// Data associated with a context line reported to a [`Sink`].
pub struct SinkContext<'b> {
    bytes: &'b [u8],
    absolute_byte_offset: u64,
    line_number: Option<u64>,
    kind: SinkContextKind,
}

impl<'b> SinkContext<'b> {
    /// The context line bytes.
    #[inline]
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes
    }

    /// The absolute byte offset from the beginning of the input.
    #[inline]
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// The 1-based line number, if line numbers are being tracked.
    #[inline]
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }

    /// The kind of context (before, after, or other/passthru).
    #[inline]
    pub fn kind(&self) -> &SinkContextKind {
        &self.kind
    }
}

impl<'b> fmt::Debug for SinkContext<'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SinkContext")
            .field("bytes", &self.bytes.as_bstr())
            .field("absolute_byte_offset", &self.absolute_byte_offset)
            .field("line_number", &self.line_number)
            .field("kind", &self.kind)
            .finish()
    }
}

/// The kind of context line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SinkContextKind {
    /// A line that appeared before the match.
    Before,
    /// A line that appeared after the match.
    After,
    /// A line that is neither strictly before nor after (e.g., passthru).
    Other,
}

// ---------------------------------------------------------------------------
// SinkFinish
// ---------------------------------------------------------------------------

/// Summary information reported to [`Sink::finish`] after a search completes.
#[derive(Clone, Debug)]
pub struct SinkFinish {
    /// Total number of bytes searched.
    pub byte_count: u64,
    /// If binary data was detected, the byte offset where it was found.
    pub binary_byte_offset: Option<u64>,
}

// ---------------------------------------------------------------------------
// Sink trait
// ---------------------------------------------------------------------------

/// A trait that consumers implement to receive search results.
///
/// The searcher calls methods on this trait as it discovers matches and
/// context lines. Implementations can collect results, print them, or
/// do whatever they like.
pub trait Sink {
    /// The error type. Must be constructible from I/O and matcher errors.
    type Error: SinkError;

    /// Called for each matching line. Return `Ok(true)` to continue
    /// searching, `Ok(false)` to stop.
    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error>;

    /// Called for each context line. Return `Ok(true)` to continue,
    /// `Ok(false)` to stop. The default does nothing and continues.
    fn context(
        &mut self,
        _searcher: &Searcher,
        _ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called between groups of matches when context is enabled, to
    /// indicate a break. Return `Ok(true)` to continue, `Ok(false)` to
    /// stop. The default does nothing and continues.
    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called before searching begins. Return `Ok(true)` to proceed with
    /// the search, `Ok(false)` to skip it entirely.
    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called after searching completes, regardless of whether any matches
    /// were found.
    fn finish(
        &mut self,
        _searcher: &Searcher,
        _summary: &SinkFinish,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called when binary data is detected. The `binary_byte_offset` is the
    /// absolute position in the input where the binary byte was found.
    ///
    /// Return `Ok(true)` to continue searching, `Ok(false)` to stop.
    /// The default returns `Ok(false)` (stop searching).
    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

impl<'a, S: Sink + ?Sized> Sink for &'a mut S {
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
        summary: &SinkFinish,
    ) -> Result<(), Self::Error> {
        (**self).finish(searcher, summary)
    }

    fn binary_data(
        &mut self,
        searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        (**self).binary_data(searcher, binary_byte_offset)
    }
}

// ---------------------------------------------------------------------------
// SearcherBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and constructing a [`Searcher`].
#[derive(Clone, Debug)]
pub struct SearcherBuilder {
    line_terminator: LineTerminator,
    invert_match: bool,
    line_number: bool,
    multi_line: bool,
    before_context: usize,
    after_context: usize,
    passthru: bool,
    binary_detection: BinaryDetection,
    encoding: Option<&'static encoding_rs::Encoding>,
    memory_map: MemoryMap,
    max_count: Option<u64>,
    stop_on_nonmatch: bool,
}

impl SearcherBuilder {
    /// Create a new `SearcherBuilder` with default settings.
    pub fn new() -> SearcherBuilder {
        SearcherBuilder {
            line_terminator: LineTerminator::default(),
            invert_match: false,
            line_number: true,
            multi_line: false,
            before_context: 0,
            after_context: 0,
            passthru: false,
            binary_detection: BinaryDetection::None,
            encoding: None,
            memory_map: MemoryMap::default(),
            max_count: None,
            stop_on_nonmatch: false,
        }
    }

    /// Build a [`Searcher`] from this builder's configuration.
    pub fn build(&self) -> Searcher {
        Searcher {
            config: Config {
                line_terminator: self.line_terminator,
                invert_match: self.invert_match,
                line_number: self.line_number,
                multi_line: self.multi_line,
                before_context: self.before_context,
                after_context: self.after_context,
                passthru: self.passthru,
                binary_detection: self.binary_detection.clone(),
                encoding: self.encoding,
                memory_map: self.memory_map,
                max_count: self.max_count,
                stop_on_nonmatch: self.stop_on_nonmatch,
            },
        }
    }

    /// Set the line terminator used to split lines.
    pub fn line_terminator(&mut self, lt: LineTerminator) -> &mut Self {
        self.line_terminator = lt;
        self
    }

    /// When enabled, lines that do NOT match are reported as matches,
    /// and lines that DO match are suppressed.
    pub fn invert_match(&mut self, yes: bool) -> &mut Self {
        self.invert_match = yes;
        self
    }

    /// When enabled (the default), line numbers are tracked and reported.
    pub fn line_number(&mut self, yes: bool) -> &mut Self {
        self.line_number = yes;
        self
    }

    /// When enabled, the entire input is searched as a single buffer and
    /// the matcher is run against the full content (useful for patterns
    /// that span multiple lines).
    pub fn multi_line(&mut self, yes: bool) -> &mut Self {
        self.multi_line = yes;
        self
    }

    /// Set the number of lines of context to show before each match.
    pub fn before_context(&mut self, count: usize) -> &mut Self {
        self.before_context = count;
        self
    }

    /// Set the number of lines of context to show after each match.
    pub fn after_context(&mut self, count: usize) -> &mut Self {
        self.after_context = count;
        self
    }

    /// When enabled, all non-matching lines are reported as context with
    /// kind [`SinkContextKind::Other`]. This is mutually exclusive with
    /// before/after context.
    pub fn passthru(&mut self, yes: bool) -> &mut Self {
        self.passthru = yes;
        self
    }

    /// Configure binary data detection.
    pub fn binary_detection(&mut self, detection: BinaryDetection) -> &mut Self {
        self.binary_detection = detection;
        self
    }

    /// Set the encoding for transcoding input before searching.
    ///
    /// When set to `Some(encoding)`, input is transcoded from `encoding`
    /// to UTF-8 before searching. When `None` (the default), no transcoding
    /// is performed.
    pub fn encoding(
        &mut self,
        encoding: Option<&'static encoding_rs::Encoding>,
    ) -> &mut Self {
        self.encoding = encoding;
        self
    }

    /// Set the memory mapping strategy.
    pub fn memory_map(&mut self, strategy: MemoryMap) -> &mut Self {
        self.memory_map = strategy;
        self
    }

    /// Set the maximum number of matching lines to report per search.
    pub fn max_count(&mut self, count: Option<u64>) -> &mut Self {
        self.max_count = count;
        self
    }

    /// When enabled, the search stops after the first non-matching line
    /// that follows a match.
    pub fn stop_on_nonmatch(&mut self, yes: bool) -> &mut Self {
        self.stop_on_nonmatch = yes;
        self
    }
}

impl Default for SearcherBuilder {
    fn default() -> SearcherBuilder {
        SearcherBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Config (internal)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Config {
    line_terminator: LineTerminator,
    invert_match: bool,
    line_number: bool,
    multi_line: bool,
    before_context: usize,
    after_context: usize,
    passthru: bool,
    binary_detection: BinaryDetection,
    encoding: Option<&'static encoding_rs::Encoding>,
    memory_map: MemoryMap,
    max_count: Option<u64>,
    stop_on_nonmatch: bool,
}

// ---------------------------------------------------------------------------
// Searcher
// ---------------------------------------------------------------------------

/// The main search driver.
///
/// A `Searcher` reads input, splits it into lines, tests each line with a
/// [`Matcher`], and reports results through a [`Sink`].
///
/// Construct a `Searcher` via [`SearcherBuilder`].
#[derive(Clone, Debug)]
pub struct Searcher {
    config: Config,
}

impl Searcher {
    /// Create a new `Searcher` with default configuration.
    pub fn new() -> Searcher {
        SearcherBuilder::new().build()
    }

    /// Return the configured line terminator.
    #[inline]
    pub fn line_terminator(&self) -> LineTerminator {
        self.config.line_terminator
    }

    /// Return the configured binary detection strategy.
    #[inline]
    pub fn binary_detection(&self) -> &BinaryDetection {
        &self.config.binary_detection
    }

    /// Return true if multiline mode is enabled.
    #[inline]
    pub fn multi_line(&self) -> bool {
        self.config.multi_line
    }

    /// Return true if invert-match mode is enabled.
    #[inline]
    pub fn invert_match(&self) -> bool {
        self.config.invert_match
    }

    /// Return the number of after-context lines.
    #[inline]
    pub fn after_context(&self) -> usize {
        self.config.after_context
    }

    /// Return the number of before-context lines.
    #[inline]
    pub fn before_context(&self) -> usize {
        self.config.before_context
    }

    /// Return true if passthru mode is enabled.
    #[inline]
    pub fn passthru(&self) -> bool {
        self.config.passthru
    }

    /// Return true if line numbers are being tracked.
    #[inline]
    pub fn line_number(&self) -> bool {
        self.config.line_number
    }

    /// Return the max count, if set.
    #[inline]
    pub fn max_count(&self) -> Option<u64> {
        self.config.max_count
    }

    /// Return true if stop-on-nonmatch is enabled.
    #[inline]
    pub fn stop_on_nonmatch(&self) -> bool {
        self.config.stop_on_nonmatch
    }

    // -----------------------------------------------------------------------
    // Public search entry points
    // -----------------------------------------------------------------------

    /// Search a file by path.
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
        self.search_file(matcher, &file, sink)
    }

    /// Search an already-opened file.
    pub fn search_file<M, S>(
        &mut self,
        matcher: &M,
        file: &File,
        sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        // Try memory mapping if configured.
        if self.should_mmap(file) {
            // SAFETY: The file must not be concurrently modified. This is
            // the same assumption ripgrep makes.
            let mmap = unsafe {
                memmap2::Mmap::map(file).map_err(S::Error::error_io)?
            };
            return self.search_slice(matcher, &mmap, sink);
        }
        self.search_reader(matcher, file, sink)
    }

    /// Search an arbitrary reader.
    pub fn search_reader<M, R, S>(
        &mut self,
        matcher: &M,
        rdr: R,
        sink: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        R: Read,
        S: Sink,
    {
        let buf = self.read_to_vec(rdr)?;
        self.search_slice(matcher, &buf, sink)
    }

    /// Search a byte slice.
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
        if !sink.begin(self)? {
            let finish = SinkFinish {
                byte_count: 0,
                binary_byte_offset: None,
            };
            sink.finish(self, &finish)?;
            return Ok(());
        }
        if self.config.multi_line {
            self.search_slice_multi_line(matcher, slice, &mut sink)
        } else {
            self.search_slice_by_line(matcher, slice, &mut sink)
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Determine whether to mmap the file.
    fn should_mmap(&self, file: &File) -> bool {
        match self.config.memory_map {
            MemoryMap::Always => true,
            MemoryMap::Never => false,
            MemoryMap::Auto => {
                // Heuristic: mmap files larger than 1 MiB.
                match file.metadata() {
                    Ok(md) => md.len() >= 1024 * 1024,
                    Err(_) => false,
                }
            }
        }
    }

    /// Read a reader into a Vec<u8>, optionally transcoding.
    fn read_to_vec<R: Read, E: SinkError>(&self, rdr: R) -> Result<Vec<u8>, E> {
        let mut buf = Vec::new();
        if let Some(encoding) = self.config.encoding {
            let mut decoder = DecodeReaderBytesBuilder::new()
                .encoding(Some(encoding))
                .build(rdr);
            decoder.read_to_end(&mut buf).map_err(E::error_io)?;
        } else {
            let mut rdr = rdr;
            rdr.read_to_end(&mut buf).map_err(E::error_io)?;
        }
        Ok(buf)
    }

    /// Detect binary data (NUL bytes) in a chunk.
    ///
    /// Returns `Some(offset)` if a NUL byte is found (offset relative to
    /// the start of the provided slice).
    fn detect_binary(&self, data: &[u8]) -> Option<usize> {
        if self.config.binary_detection.is_none() {
            return None;
        }
        memchr::memchr(0x00, data)
    }

    /// Apply binary conversion: replace NUL bytes with the replacement byte.
    fn apply_binary_conversion(&self, data: &mut Vec<u8>) {
        if let BinaryDetection::Convert(replacement) = self.config.binary_detection {
            for b in data.iter_mut() {
                if *b == 0x00 {
                    *b = replacement;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Line-by-line search
    // -----------------------------------------------------------------------

    fn search_slice_by_line<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        // Handle binary conversion by working on an owned copy if needed.
        let (buf, binary_offset_from_conversion);
        match self.config.binary_detection {
            BinaryDetection::Convert(_) => {
                let mut owned = slice.to_vec();
                let nul_pos = memchr::memchr(0x00, &owned);
                self.apply_binary_conversion(&mut owned);
                binary_offset_from_conversion = nul_pos.map(|p| p as u64);
                buf = owned;
            }
            _ => {
                binary_offset_from_conversion = None;
                buf = slice.to_vec();
            }
        }

        let line_term = self.config.line_terminator.as_byte();
        let has_context = self.config.before_context > 0
            || self.config.after_context > 0
            || self.config.passthru;

        // Split into lines (keeping terminators).
        let lines = split_lines(&buf, line_term);

        let mut line_number: u64 = 0; // 0-based counter, displayed as 1-based
        let mut match_count: u64 = 0;
        let mut binary_byte_offset: Option<u64> = binary_offset_from_conversion;
        let mut absolute_offset: u64 = 0;

        // Context tracking
        // before_ring stores (line_bytes_range, absolute_offset, line_number_1based)
        let before_cap = self.config.before_context;
        let mut before_ring: Vec<(Range<usize>, u64, u64)> = Vec::with_capacity(before_cap);

        let mut after_remaining: usize = 0;
        let mut had_match = false;
        // Track whether we need a context_break before the next group
        let mut need_separator = false;
        let mut saw_non_match_after_match = false;

        for line_range in &lines {
            let line = &buf[line_range.clone()];
            line_number += 1;
            let current_line_number = line_number;
            let current_absolute_offset = absolute_offset;

            // Test whether this line matches.
            let is_match = self.line_matches(matcher, line)?;
            let is_match = if self.config.invert_match {
                !is_match
            } else {
                is_match
            };

            // Binary detection (Quit mode).
            if matches!(self.config.binary_detection, BinaryDetection::Quit) {
                if let Some(nul_pos) = memchr::memchr(0x00, line) {
                    let nul_offset = current_absolute_offset + nul_pos as u64;
                    if binary_byte_offset.is_none() {
                        binary_byte_offset = Some(nul_offset);
                    }
                    if is_match {
                        let sink_match = SinkMatch {
                            bytes: line,
                            absolute_byte_offset: current_absolute_offset,
                            line_number: if self.config.line_number {
                                Some(current_line_number)
                            } else {
                                None
                            },
                            buffer: &buf,
                            bytes_range_in_buffer: line_range.clone(),
                        };
                        let _ = sink.matched(self, &sink_match);
                    }
                    let keep_going = sink.binary_data(self, nul_offset)?;
                    if !keep_going {
                        let finish = SinkFinish {
                            byte_count: current_absolute_offset + line.len() as u64,
                            binary_byte_offset,
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                }
            }

            if is_match {
                // Check max count.
                if let Some(max) = self.config.max_count {
                    if match_count >= max {
                        absolute_offset += line.len() as u64;
                        break;
                    }
                }

                // Emit context_break before this group's before-context
                // if we had a previous match and there's a gap.
                if has_context && need_separator {
                    if !sink.context_break(self)? {
                        break;
                    }
                }
                need_separator = false;

                // Emit before-context lines.
                if before_cap > 0 {
                    for (br, boff, bln) in before_ring.drain(..) {
                        let ctx_bytes = &buf[br.clone()];
                        let ctx = SinkContext {
                            bytes: ctx_bytes,
                            absolute_byte_offset: boff,
                            line_number: if self.config.line_number {
                                Some(bln)
                            } else {
                                None
                            },
                            kind: SinkContextKind::Before,
                        };
                        if !sink.context(self, &ctx)? {
                            let finish = SinkFinish {
                                byte_count: absolute_offset + line.len() as u64,
                                binary_byte_offset,
                            };
                            sink.finish(self, &finish)?;
                            return Ok(());
                        }
                    }
                }

                // Emit the match.
                let sm = SinkMatch {
                    bytes: line,
                    absolute_byte_offset: current_absolute_offset,
                    line_number: if self.config.line_number {
                        Some(current_line_number)
                    } else {
                        None
                    },
                    buffer: &buf,
                    bytes_range_in_buffer: line_range.clone(),
                };
                if !sink.matched(self, &sm)? {
                    absolute_offset += line.len() as u64;
                    break;
                }

                match_count += 1;
                had_match = true;
                after_remaining = self.config.after_context;
                saw_non_match_after_match = false;
            } else {
                // Non-matching line.
                if self.config.stop_on_nonmatch && had_match {
                    saw_non_match_after_match = true;
                    // If there is no after_context remaining, stop.
                    if after_remaining == 0 {
                        absolute_offset += line.len() as u64;
                        break;
                    }
                }

                if after_remaining > 0 {
                    // Emit as after-context.
                    let ctx = SinkContext {
                        bytes: line,
                        absolute_byte_offset: current_absolute_offset,
                        line_number: if self.config.line_number {
                            Some(current_line_number)
                        } else {
                            None
                        },
                        kind: SinkContextKind::After,
                    };
                    if !sink.context(self, &ctx)? {
                        absolute_offset += line.len() as u64;
                        break;
                    }
                    after_remaining -= 1;
                    if after_remaining == 0 {
                        // We've exhausted after-context. A separator is needed
                        // before the next group.
                        need_separator = true;
                        if self.config.stop_on_nonmatch && saw_non_match_after_match {
                            absolute_offset += line.len() as u64;
                            break;
                        }
                    }
                } else if self.config.passthru && had_match {
                    // In passthru mode, emit non-matching lines as Other context.
                    let ctx = SinkContext {
                        bytes: line,
                        absolute_byte_offset: current_absolute_offset,
                        line_number: if self.config.line_number {
                            Some(current_line_number)
                        } else {
                            None
                        },
                        kind: SinkContextKind::Other,
                    };
                    if !sink.context(self, &ctx)? {
                        absolute_offset += line.len() as u64;
                        break;
                    }
                } else {
                    // Store in before-context ring if applicable.
                    if before_cap > 0 && had_match {
                        // We've already emitted matches, so set need_separator
                        need_separator = true;
                    }
                    if before_cap > 0 {
                        if before_ring.len() >= before_cap {
                            before_ring.remove(0);
                        }
                        before_ring.push((
                            line_range.clone(),
                            current_absolute_offset,
                            current_line_number,
                        ));
                    }
                }
            }

            absolute_offset += line.len() as u64;
        }

        let finish = SinkFinish {
            byte_count: buf.len() as u64,
            binary_byte_offset,
        };
        sink.finish(self, &finish)?;
        Ok(())
    }

    /// Test whether a single line matches.
    fn line_matches<M: Matcher, E: SinkError>(
        &self,
        matcher: &M,
        line: &[u8],
    ) -> Result<bool, E> {
        // Strip the line terminator for matching purposes.
        let line_term = self.config.line_terminator.as_byte();
        let mut end = line.len();
        if end > 0 && line[end - 1] == line_term {
            end -= 1;
        }
        // Also strip \r if CRLF mode.
        if self.config.line_terminator.is_crlf() && end > 0 && line[end - 1] == b'\r' {
            end -= 1;
        }
        let content = &line[..end];

        matcher
            .is_match(content)
            .map_err(|e| E::error_message(e))
    }

    // -----------------------------------------------------------------------
    // Multiline search
    // -----------------------------------------------------------------------

    fn search_slice_multi_line<M, S>(
        &mut self,
        matcher: &M,
        slice: &[u8],
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        // Handle binary conversion.
        let (buf, binary_byte_offset_init);
        match self.config.binary_detection {
            BinaryDetection::Convert(_) => {
                let mut owned = slice.to_vec();
                let nul_pos = memchr::memchr(0x00, &owned);
                self.apply_binary_conversion(&mut owned);
                binary_byte_offset_init = nul_pos.map(|p| p as u64);
                buf = owned;
            }
            BinaryDetection::Quit => {
                if let Some(nul_pos) = memchr::memchr(0x00, slice) {
                    let nul_offset = nul_pos as u64;
                    let keep_going = sink.binary_data(self, nul_offset)?;
                    if !keep_going {
                        let finish = SinkFinish {
                            byte_count: slice.len() as u64,
                            binary_byte_offset: Some(nul_offset),
                        };
                        sink.finish(self, &finish)?;
                        return Ok(());
                    }
                }
                binary_byte_offset_init = None;
                buf = slice.to_vec();
            }
            BinaryDetection::None => {
                binary_byte_offset_init = None;
                buf = slice.to_vec();
            }
        }

        let line_term = self.config.line_terminator.as_byte();
        let mut binary_byte_offset = binary_byte_offset_init;
        let mut match_count: u64 = 0;

        // Find all matches in the full buffer.
        let mut matches: Vec<grep_matcher::Match> = Vec::new();
        let find_result: Result<(), _> = matcher.find_iter(&buf, |m| {
            matches.push(m);
            true
        });
        find_result.map_err(|e| S::Error::error_message(e))?;

        if self.config.invert_match {
            // Invert: report lines that contain NO match.
            let lines = split_lines(&buf, line_term);
            let mut line_number: u64 = 0;
            let mut absolute_offset: u64 = 0;

            for line_range in &lines {
                line_number += 1;
                let line = &buf[line_range.clone()];
                let line_start = line_range.start;
                let line_end = line_range.end;

                // Check if any match overlaps this line.
                let has_match = matches.iter().any(|m| {
                    m.start() < line_end && m.end() > line_start
                });

                if !has_match {
                    if let Some(max) = self.config.max_count {
                        if match_count >= max {
                            break;
                        }
                    }
                    let sm = SinkMatch {
                        bytes: line,
                        absolute_byte_offset: absolute_offset,
                        line_number: if self.config.line_number {
                            Some(line_number)
                        } else {
                            None
                        },
                        buffer: &buf,
                        bytes_range_in_buffer: line_range.clone(),
                    };
                    if !sink.matched(self, &sm)? {
                        break;
                    }
                    match_count += 1;
                }

                absolute_offset += line.len() as u64;
            }
        } else {
            // Report each match.
            // Group overlapping/adjacent matches that fall on the same lines.
            // For simplicity, report per match, expanding to line boundaries.
            let mut reported_lines: std::collections::HashSet<usize> =
                std::collections::HashSet::new();

            for m in &matches {
                if let Some(max) = self.config.max_count {
                    if match_count >= max {
                        break;
                    }
                }

                // Find the line boundaries that contain this match.
                let line_start = buf[..m.start()]
                    .rfind_byte(line_term)
                    .map(|p| p + 1)
                    .unwrap_or(0);
                let line_end = buf[m.end()..]
                    .find_byte(line_term)
                    .map(|p| m.end() + p + 1)
                    .unwrap_or(buf.len());

                // Avoid reporting the same line region multiple times.
                if !reported_lines.insert(line_start) {
                    continue;
                }

                let line_bytes = &buf[line_start..line_end];

                // Compute line number.
                let line_number_val = if self.config.line_number {
                    Some(
                        buf[..line_start]
                            .iter()
                            .filter(|&&b| b == line_term)
                            .count() as u64
                            + 1,
                    )
                } else {
                    None
                };

                let sm = SinkMatch {
                    bytes: line_bytes,
                    absolute_byte_offset: line_start as u64,
                    line_number: line_number_val,
                    buffer: &buf,
                    bytes_range_in_buffer: line_start..line_end,
                };
                if !sink.matched(self, &sm)? {
                    break;
                }
                match_count += 1;
            }
        }

        let finish = SinkFinish {
            byte_count: buf.len() as u64,
            binary_byte_offset,
        };
        sink.finish(self, &finish)?;
        Ok(())
    }
}

impl Default for Searcher {
    fn default() -> Searcher {
        Searcher::new()
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Split a byte slice into line ranges, keeping line terminators.
///
/// Each returned range `[start..end)` includes the trailing `term` byte
/// (if present). The last segment is included even if it does not end with
/// a terminator.
fn split_lines(data: &[u8], term: u8) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < data.len() {
        match memchr::memchr(term, &data[start..]) {
            Some(pos) => {
                let end = start + pos + 1;
                lines.push(start..end);
                start = end;
            }
            None => {
                lines.push(start..data.len());
                break;
            }
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Match, Matcher, NoCaptures, NoError};

    /// A trivial matcher that matches lines containing a given literal.
    struct LiteralMatcher {
        literal: Vec<u8>,
    }

    impl LiteralMatcher {
        fn new(literal: &[u8]) -> LiteralMatcher {
            LiteralMatcher {
                literal: literal.to_vec(),
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
        ) -> Result<Option<Match>, NoError> {
            if at > haystack.len() {
                return Ok(None);
            }
            let haystack = &haystack[at..];
            match haystack
                .windows(self.literal.len())
                .position(|w| w == self.literal.as_slice())
            {
                Some(pos) => {
                    Ok(Some(Match::new(at + pos, at + pos + self.literal.len())))
                }
                None => Ok(None),
            }
        }

        fn new_captures(&self) -> Result<NoCaptures, NoError> {
            Ok(NoCaptures::new())
        }
    }

    /// A simple sink that collects matched lines as Strings.
    struct CollectSink {
        matches: Vec<(u64, Option<u64>, String)>,
        contexts: Vec<(SinkContextKind, u64, Option<u64>, String)>,
        context_breaks: usize,
        binary_offset: Option<u64>,
        finish_byte_count: u64,
    }

    impl CollectSink {
        fn new() -> CollectSink {
            CollectSink {
                matches: Vec::new(),
                contexts: Vec::new(),
                context_breaks: 0,
                binary_offset: None,
                finish_byte_count: 0,
            }
        }
    }

    impl Sink for CollectSink {
        type Error = io::Error;

        fn matched(
            &mut self,
            _searcher: &Searcher,
            mat: &SinkMatch<'_>,
        ) -> Result<bool, io::Error> {
            let text = String::from_utf8_lossy(mat.bytes()).to_string();
            self.matches
                .push((mat.absolute_byte_offset(), mat.line_number(), text));
            Ok(true)
        }

        fn context(
            &mut self,
            _searcher: &Searcher,
            ctx: &SinkContext<'_>,
        ) -> Result<bool, io::Error> {
            let text = String::from_utf8_lossy(ctx.bytes()).to_string();
            self.contexts.push((
                ctx.kind().clone(),
                ctx.absolute_byte_offset(),
                ctx.line_number(),
                text,
            ));
            Ok(true)
        }

        fn context_break(
            &mut self,
            _searcher: &Searcher,
        ) -> Result<bool, io::Error> {
            self.context_breaks += 1;
            Ok(true)
        }

        fn binary_data(
            &mut self,
            _searcher: &Searcher,
            binary_byte_offset: u64,
        ) -> Result<bool, io::Error> {
            self.binary_offset = Some(binary_byte_offset);
            Ok(false)
        }

        fn finish(
            &mut self,
            _searcher: &Searcher,
            summary: &SinkFinish,
        ) -> Result<(), io::Error> {
            self.finish_byte_count = summary.byte_count;
            Ok(())
        }
    }

    #[test]
    fn test_simple_match() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\ngoodbye world\nhello again\n";
        let mut searcher = Searcher::new();
        let sink = CollectSink::new();
        let mut sink = sink;
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
        assert_eq!(sink.matches[0].2, "hello world\n");
        assert_eq!(sink.matches[0].1, Some(1));
        assert_eq!(sink.matches[1].2, "hello again\n");
        assert_eq!(sink.matches[1].1, Some(3));
    }

    #[test]
    fn test_invert_match() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\ngoodbye world\nhello again\n";
        let mut searcher = SearcherBuilder::new()
            .invert_match(true)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].2, "goodbye world\n");
        assert_eq!(sink.matches[0].1, Some(2));
    }

    #[test]
    fn test_no_line_numbers() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\n";
        let mut searcher = SearcherBuilder::new()
            .line_number(false)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].1, None);
    }

    #[test]
    fn test_max_count() {
        let matcher = LiteralMatcher::new(b"line");
        let data = b"line 1\nline 2\nline 3\nline 4\n";
        let mut searcher = SearcherBuilder::new()
            .max_count(Some(2))
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
    }

    #[test]
    fn test_before_context() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"line 1\nline 2\nmatch here\nline 4\n";
        let mut searcher = SearcherBuilder::new()
            .before_context(2)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].2, "match here\n");
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].0, SinkContextKind::Before);
        assert_eq!(sink.contexts[0].3, "line 1\n");
        assert_eq!(sink.contexts[1].0, SinkContextKind::Before);
        assert_eq!(sink.contexts[1].3, "line 2\n");
    }

    #[test]
    fn test_after_context() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"match here\nline 2\nline 3\nline 4\n";
        let mut searcher = SearcherBuilder::new()
            .after_context(2)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.contexts.len(), 2);
        assert_eq!(sink.contexts[0].0, SinkContextKind::After);
        assert_eq!(sink.contexts[0].3, "line 2\n");
        assert_eq!(sink.contexts[1].0, SinkContextKind::After);
        assert_eq!(sink.contexts[1].3, "line 3\n");
    }

    #[test]
    fn test_no_context_break_without_context() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"match 1\nno\nmatch 2\n";
        let mut searcher = Searcher::new();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
        assert_eq!(
            sink.context_breaks, 0,
            "No context breaks when no context is configured"
        );
    }

    #[test]
    fn test_context_break_between_groups() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"match 1\nskip\nskip\nmatch 2\n";
        let mut searcher = SearcherBuilder::new()
            .before_context(1)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
        // There should be exactly 1 context break between the two groups.
        assert_eq!(sink.context_breaks, 1);
    }

    #[test]
    fn test_binary_detection_quit() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello\n\x00binary data\nhello again\n";
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit())
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        // Should have matched first line, then quit on binary.
        assert_eq!(sink.matches.len(), 1);
        assert!(sink.binary_offset.is_some());
    }

    #[test]
    fn test_multiline_search() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\ngoodbye\nhello again\n";
        let mut searcher = SearcherBuilder::new()
            .multi_line(true)
            .build();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 2);
    }

    #[test]
    fn test_empty_input() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"";
        let mut searcher = Searcher::new();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 0);
        assert_eq!(sink.finish_byte_count, 0);
    }

    #[test]
    fn test_no_trailing_newline() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello";
        let mut searcher = Searcher::new();
        let mut sink = CollectSink::new();
        searcher
            .search_slice(&matcher, data, &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].2, "hello");
    }

    #[test]
    fn test_split_lines() {
        let data = b"line1\nline2\nline3\n";
        let lines = split_lines(data, b'\n');
        assert_eq!(lines.len(), 3);
        assert_eq!(&data[lines[0].clone()], b"line1\n");
        assert_eq!(&data[lines[1].clone()], b"line2\n");
        assert_eq!(&data[lines[2].clone()], b"line3\n");
    }

    #[test]
    fn test_split_lines_no_trailing() {
        let data = b"line1\nline2";
        let lines = split_lines(data, b'\n');
        assert_eq!(lines.len(), 2);
        assert_eq!(&data[lines[0].clone()], b"line1\n");
        assert_eq!(&data[lines[1].clone()], b"line2");
    }

    #[test]
    fn test_sink_match_accessors() {
        let buf = b"hello world\n";
        let sm = SinkMatch {
            bytes: &buf[..],
            absolute_byte_offset: 42,
            line_number: Some(7),
            buffer: &buf[..],
            bytes_range_in_buffer: 0..buf.len(),
        };
        assert_eq!(sm.bytes(), b"hello world\n");
        assert_eq!(sm.absolute_byte_offset(), 42);
        assert_eq!(sm.line_number(), Some(7));
        assert_eq!(sm.buffer(), b"hello world\n");
        assert_eq!(sm.bytes_range_in_buffer(), 0..12);
    }

    #[test]
    fn test_builder_defaults() {
        let searcher = SearcherBuilder::new().build();
        assert_eq!(searcher.line_terminator(), LineTerminator::default());
        assert!(!searcher.invert_match());
        assert!(searcher.line_number());
        assert!(!searcher.multi_line());
        assert_eq!(searcher.before_context(), 0);
        assert_eq!(searcher.after_context(), 0);
        assert!(!searcher.passthru());
        assert_eq!(searcher.max_count(), None);
    }
}
