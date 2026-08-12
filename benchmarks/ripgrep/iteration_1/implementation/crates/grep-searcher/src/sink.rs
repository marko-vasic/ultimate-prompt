/*!
The Sink trait and related types for consuming search results.

The [`Sink`] trait is the primary interface through which the searcher
reports matches and context lines to the caller. Callers implement `Sink`
to collect, format, or otherwise process search results.
*/

use std::io;

use crate::Searcher;

/// The result of a Sink operation. If an error occurs, the search stops.
pub type SinkError = io::Error;

/// Context passed to the [`Sink::context`] callback.
///
/// This contains information about a context line (before-context,
/// after-context, or passthru "other" context).
#[derive(Clone, Debug)]
pub struct SinkContext<'a> {
    pub(crate) kind: SinkContextKind,
    pub(crate) bytes: &'a [u8],
    pub(crate) absolute_byte_offset: u64,
    pub(crate) line_number: Option<u64>,
}

/// The kind of context line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkContextKind {
    /// A line appearing before a match.
    Before,
    /// A line appearing after a match.
    After,
    /// A line that is neither before nor after context (passthru).
    Other,
}

impl<'a> SinkContext<'a> {
    /// Returns the kind of context this line represents.
    pub fn kind(&self) -> &SinkContextKind {
        &self.kind
    }

    /// Returns the bytes of the context line.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the absolute byte offset of this context line within the
    /// searched content.
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// Returns the 1-based line number, if line numbers are enabled.
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }
}

/// A match found by the searcher.
///
/// This is passed to the [`Sink::matched`] callback for every matching line.
#[derive(Clone, Debug)]
pub struct SinkMatch<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) absolute_byte_offset: u64,
    pub(crate) line_number: Option<u64>,
    pub(crate) buffer: &'a [u8],
    pub(crate) bytes_range_in_buffer: std::ops::Range<usize>,
}

impl<'a> SinkMatch<'a> {
    /// Returns the bytes of the matched line (including the line terminator).
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the absolute byte offset of this line within the searched
    /// content.
    pub fn absolute_byte_offset(&self) -> u64 {
        self.absolute_byte_offset
    }

    /// Returns the 1-based line number, if line numbers are enabled.
    pub fn line_number(&self) -> Option<u64> {
        self.line_number
    }

    /// Returns the entire buffer being searched. Useful for extracting
    /// surrounding context directly.
    pub fn buffer(&self) -> &'a [u8] {
        self.buffer
    }

    /// Returns the byte range within [`buffer()`](SinkMatch::buffer) that
    /// corresponds to the matched line.
    pub fn bytes_range_in_buffer(&self) -> std::ops::Range<usize> {
        self.bytes_range_in_buffer.clone()
    }
}

/// Callback data indicating a search has finished.
#[derive(Clone, Debug)]
pub struct SinkFinish {
    pub(crate) byte_count: u64,
    pub(crate) binary_byte_offset: Option<u64>,
}

impl SinkFinish {
    /// Returns the total number of bytes searched.
    pub fn byte_count(&self) -> u64 {
        self.byte_count
    }

    /// If binary content was detected, returns the byte offset where the
    /// first NUL byte was found.
    pub fn binary_byte_offset(&self) -> Option<u64> {
        self.binary_byte_offset
    }
}

/// The Sink trait — consumers of search results implement this.
///
/// The searcher drives the search and calls methods on a `Sink` to report
/// matches, context lines, context breaks, and search lifecycle events.
///
/// # Error Handling
///
/// All methods return a `Result`. If any method returns an error, the
/// search is immediately aborted and the error propagated to the caller.
///
/// Methods that return `Result<bool, _>` use the boolean to indicate
/// whether searching should continue (`true`) or stop early (`false`).
pub trait Sink {
    /// The error type for this sink.
    type Error: std::fmt::Display + std::fmt::Debug + Send + From<io::Error> + 'static;

    /// Called for each matching line.
    ///
    /// Return `Ok(true)` to continue searching or `Ok(false)` to stop.
    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error>;

    /// Called for each context line (before, after, or other/passthru).
    ///
    /// The default implementation returns `Ok(true)` (continue).
    fn context(
        &mut self,
        _searcher: &Searcher,
        _ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called between groups of matches that are not adjacent, to indicate
    /// a break in the output (e.g. `--`).
    ///
    /// The default implementation returns `Ok(true)` (continue).
    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called once at the start of a search, before any matches or context
    /// are reported.
    ///
    /// Return `Ok(true)` to proceed with the search, or `Ok(false)` to skip
    /// it entirely.
    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Called once at the end of a search with summary statistics.
    fn finish(
        &mut self,
        _searcher: &Searcher,
        _finish: &SinkFinish,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Blanket implementation of [`Sink`] for closures.
///
/// This allows passing a simple closure as a sink for quick searches:
///
/// ```ignore
/// searcher.search_slice(&matcher, data, |_searcher, mat: &SinkMatch<'_>| {
///     println!("{}", String::from_utf8_lossy(mat.bytes()));
///     Ok(true)
/// })?;
/// ```
impl<F, E> Sink for F
where
    F: FnMut(&Searcher, &SinkMatch<'_>) -> Result<bool, E>,
    E: std::fmt::Display + std::fmt::Debug + Send + From<io::Error> + 'static,
{
    type Error = E;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, E> {
        (self)(searcher, mat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sink_context_kind() {
        let ctx = SinkContext {
            kind: SinkContextKind::Before,
            bytes: b"context line\n",
            absolute_byte_offset: 42,
            line_number: Some(3),
        };
        assert_eq!(*ctx.kind(), SinkContextKind::Before);
        assert_eq!(ctx.bytes(), b"context line\n");
        assert_eq!(ctx.absolute_byte_offset(), 42);
        assert_eq!(ctx.line_number(), Some(3));
    }

    #[test]
    fn test_sink_match_accessors() {
        let buffer = b"hello\nworld\n";
        let sm = SinkMatch {
            bytes: &buffer[6..12],
            absolute_byte_offset: 6,
            line_number: Some(2),
            buffer,
            bytes_range_in_buffer: 6..12,
        };
        assert_eq!(sm.bytes(), b"world\n");
        assert_eq!(sm.absolute_byte_offset(), 6);
        assert_eq!(sm.line_number(), Some(2));
        assert_eq!(sm.buffer(), buffer);
        assert_eq!(sm.bytes_range_in_buffer(), 6..12);
    }

    #[test]
    fn test_sink_finish_accessors() {
        let sf = SinkFinish {
            byte_count: 1024,
            binary_byte_offset: Some(500),
        };
        assert_eq!(sf.byte_count(), 1024);
        assert_eq!(sf.binary_byte_offset(), Some(500));
    }
}
