/*!
Trait definitions for regular expression matching in grep-like programs.

This crate provides the [`Matcher`] trait, which defines an abstract interface
for regex matching that all other grep crates depend on. By coding against this
trait, the grep pipeline is decoupled from any particular regex engine.

The key types are:

- [`Match`] — a span representing a match (start and end byte offsets).
- [`LineTerminator`] — how line endings are represented.
- [`ByteSet`] — a compact set for byte membership testing.
- [`Matcher`] — the core trait for finding matches in a haystack.
- [`Captures`] — trait for capture group access.
- [`NoCaptures`] — a no-op implementation of `Captures`.
- [`NoError`] — an uninhabitable error type for matchers that never fail.
*/

use std::fmt;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

/// Represents a contiguous range of bytes in a haystack that matched.
///
/// A `Match` value is a half-open interval `[start, end)`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Match {
    start: usize,
    end: usize,
}

impl Match {
    /// Create a new match spanning `[start, end)`.
    ///
    /// # Panics
    ///
    /// Panics if `start > end`.
    #[inline]
    pub fn new(start: usize, end: usize) -> Match {
        assert!(start <= end, "start ({start}) must be <= end ({end})");
        Match { start, end }
    }

    /// Returns the start byte offset of this match.
    #[inline]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Returns the end byte offset of this match (exclusive).
    #[inline]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Returns the length, in bytes, of this match.
    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns `true` if and only if this match is empty (i.e., has zero
    /// length).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a new `Match` with both `start` and `end` offset by `amount`.
    ///
    /// This is useful when a match was found relative to some sub-slice and
    /// you need to adjust it to be relative to a larger slice.
    #[inline]
    pub fn offset(&self, amount: usize) -> Match {
        Match {
            start: self.start + amount,
            end: self.end + amount,
        }
    }
}

// ---------------------------------------------------------------------------
// LineTerminator
// ---------------------------------------------------------------------------

/// Represents the line terminator convention used during searching.
///
/// The default line terminator is `\n`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LineTerminator {
    /// A single byte line terminator.
    Byte(u8),
    /// Carriage-return line-feed (`\r\n`). When this variant is used,
    /// `as_byte()` returns `\n` because `\n` is still the primary
    /// line-boundary marker.
    CRLF,
}

impl LineTerminator {
    /// Returns the byte that should be used for line-boundary detection.
    ///
    /// For `Byte(b)` this returns `b`; for `CRLF` this returns `b'\n'`.
    #[inline]
    pub fn as_byte(&self) -> u8 {
        match *self {
            LineTerminator::Byte(b) => b,
            LineTerminator::CRLF => b'\n',
        }
    }

    /// Returns `true` if and only if this line terminator is CRLF.
    #[inline]
    pub fn is_crlf(&self) -> bool {
        matches!(*self, LineTerminator::CRLF)
    }
}

impl Default for LineTerminator {
    #[inline]
    fn default() -> LineTerminator {
        LineTerminator::Byte(b'\n')
    }
}

// ---------------------------------------------------------------------------
// ByteSet
// ---------------------------------------------------------------------------

/// A compact set of bytes for membership testing.
///
/// This is backed by a simple `[bool; 256]` array and supports constant-time
/// membership queries.
#[derive(Clone, Debug)]
pub struct ByteSet {
    bits: [bool; 256],
}

impl ByteSet {
    /// Creates a new empty byte set (equivalent to [`ByteSet::empty`]).
    #[inline]
    pub fn new() -> ByteSet {
        ByteSet::empty()
    }

    /// Creates a byte set that contains no bytes.
    #[inline]
    pub fn empty() -> ByteSet {
        ByteSet {
            bits: [false; 256],
        }
    }

    /// Creates a byte set that contains every byte.
    #[inline]
    pub fn full() -> ByteSet {
        ByteSet {
            bits: [true; 256],
        }
    }

    /// Adds `byte` to this set.
    #[inline]
    pub fn add(&mut self, byte: u8) {
        self.bits[byte as usize] = true;
    }

    /// Removes `byte` from this set.
    #[inline]
    pub fn remove(&mut self, byte: u8) {
        self.bits[byte as usize] = false;
    }

    /// Returns `true` if and only if `byte` is a member of this set.
    #[inline]
    pub fn contains(&self, byte: u8) -> bool {
        self.bits[byte as usize]
    }
}

impl Default for ByteSet {
    fn default() -> ByteSet {
        ByteSet::new()
    }
}

// ---------------------------------------------------------------------------
// NoError
// ---------------------------------------------------------------------------

/// An error type that can never be instantiated.
///
/// This is useful as the `Error` associated type for [`Matcher`]
/// implementations that are infallible.
#[derive(Debug)]
pub enum NoError {}

impl fmt::Display for NoError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for NoError {}

// ---------------------------------------------------------------------------
// Captures trait
// ---------------------------------------------------------------------------

/// A trait describing capture groups for a [`Matcher`].
///
/// The main purpose is to provide access to the spans of individual capture
/// groups after a successful match.
pub trait Captures {
    /// Returns the total number of capture groups (including the implicit
    /// group for the overall match, if applicable).
    fn group_len(&self) -> usize;

    /// Returns the span of the capture group at `index`, or `None` if the
    /// group did not participate in the match or `index` is out of bounds.
    fn group(&self, index: usize) -> Option<Match>;

    /// Returns `true` if there are no capture groups.
    fn is_empty(&self) -> bool {
        self.group_len() == 0
    }
}

// ---------------------------------------------------------------------------
// NoCaptures
// ---------------------------------------------------------------------------

/// A no-op implementation of [`Captures`] that never contains any groups.
///
/// This is suitable for matchers that do not support capture groups.
#[derive(Clone, Debug)]
pub struct NoCaptures(());

impl NoCaptures {
    /// Creates a new `NoCaptures` value.
    pub fn new() -> NoCaptures {
        NoCaptures(())
    }
}

impl Default for NoCaptures {
    fn default() -> NoCaptures {
        NoCaptures::new()
    }
}

impl Captures for NoCaptures {
    fn group_len(&self) -> usize {
        0
    }

    fn group(&self, _index: usize) -> Option<Match> {
        None
    }
}

// ---------------------------------------------------------------------------
// Matcher trait
// ---------------------------------------------------------------------------

/// A global, lazily-initialized empty byte set returned by the default
/// `non_matching_bytes` implementation.
static EMPTY_BYTE_SET: LazyLock<ByteSet> = LazyLock::new(ByteSet::empty);

/// The core matching trait.
///
/// Implementations of this trait provide regex-like matching over byte strings
/// (haystacks). The primary required method is [`find_at`](Matcher::find_at),
/// which locates the next match starting at a given byte offset.
///
/// Most provided methods have sensible default implementations built on top of
/// `find_at`, but implementors may override them for better performance.
pub trait Matcher {
    /// The error type produced by this matcher.
    type Error: fmt::Display + fmt::Debug + Send + 'static;

    /// The capture type used by this matcher.
    type Captures: Captures;

    /// Finds the next match in `haystack` starting at byte offset `at`.
    ///
    /// If no match is found, returns `Ok(None)`.
    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Self::Error>;

    /// Creates a new, empty [`Captures`] value for use with
    /// [`captures_at`](Matcher::captures_at).
    fn new_captures(&self) -> Result<Self::Captures, Self::Error>;

    /// Finds the next match in `haystack`, starting at the beginning.
    ///
    /// This is equivalent to `self.find_at(haystack, 0)`.
    fn find(&self, haystack: &[u8]) -> Result<Option<Match>, Self::Error> {
        self.find_at(haystack, 0)
    }

    /// Iterates over all successive non-overlapping matches in `haystack`.
    ///
    /// The callback `matched` is invoked for each match. If `matched` returns
    /// `false`, iteration stops early.
    fn find_iter<F>(
        &self,
        haystack: &[u8],
        matched: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(Match) -> bool,
    {
        self.find_iter_at(haystack, 0, matched)
    }

    /// Iterates over all successive non-overlapping matches in `haystack`
    /// starting at byte offset `at`.
    ///
    /// The callback `matched` is invoked for each match. If `matched` returns
    /// `false`, iteration stops early.
    fn find_iter_at<F>(
        &self,
        haystack: &[u8],
        at: usize,
        mut matched: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(Match) -> bool,
    {
        let mut pos = at;
        loop {
            match self.find_at(haystack, pos)? {
                None => return Ok(()),
                Some(m) => {
                    if !matched(m) {
                        return Ok(());
                    }
                    // Advance past this match. If the match is empty,
                    // advance by one byte to avoid an infinite loop.
                    if m.is_empty() {
                        pos = m.end() + 1;
                        if pos > haystack.len() {
                            return Ok(());
                        }
                    } else {
                        pos = m.end();
                    }
                }
            }
        }
    }

    /// Returns `true` if any match exists in `haystack`.
    fn is_match(&self, haystack: &[u8]) -> Result<bool, Self::Error> {
        self.is_match_at(haystack, 0)
    }

    /// Returns `true` if any match exists in `haystack` starting at byte
    /// offset `at`.
    fn is_match_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<bool, Self::Error> {
        Ok(self.find_at(haystack, at)?.is_some())
    }

    /// Returns the end byte offset of the shortest match in `haystack`
    /// starting from the beginning, or `None` if no match exists.
    fn shortest_match(
        &self,
        haystack: &[u8],
    ) -> Result<Option<usize>, Self::Error> {
        self.shortest_match_at(haystack, 0)
    }

    /// Returns the end byte offset of the shortest match in `haystack`
    /// starting at byte offset `at`, or `None` if no match exists.
    fn shortest_match_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<usize>, Self::Error> {
        Ok(self.find_at(haystack, at)?.map(|m| m.end()))
    }

    /// Populates `caps` with the capture groups for the first match in
    /// `haystack` starting at byte offset `at`.
    ///
    /// Returns `true` if a match was found and `false` otherwise.
    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        _caps: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        // Default: just check if there is a match; no capture data is
        // populated (the default `NoCaptures` doesn't store anything).
        Ok(self.find_at(haystack, at)?.is_some())
    }

    /// Returns the total number of capture groups supported by this matcher.
    ///
    /// The default is `0`, meaning no capture groups.
    fn capture_count(&self) -> usize {
        0
    }

    /// Returns the index of the capture group with the given `name`, or
    /// `None` if no such group exists.
    fn capture_index(&self, _name: &str) -> Option<usize> {
        None
    }

    /// Returns the line terminator used by this matcher, if one is configured.
    fn line_terminator(&self) -> Option<LineTerminator> {
        None
    }

    /// Returns the set of bytes that are known to never appear in a match
    /// produced by this matcher.
    ///
    /// The default returns an empty set (no bytes are excluded).
    fn non_matching_bytes(&self) -> &ByteSet {
        &EMPTY_BYTE_SET
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Match tests --------------------------------------------------------

    #[test]
    fn test_match_new() {
        let m = Match::new(2, 5);
        assert_eq!(m.start(), 2);
        assert_eq!(m.end(), 5);
        assert_eq!(m.len(), 3);
        assert!(!m.is_empty());
    }

    #[test]
    fn test_match_empty() {
        let m = Match::new(3, 3);
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_match_offset() {
        let m = Match::new(2, 5).offset(10);
        assert_eq!(m.start(), 12);
        assert_eq!(m.end(), 15);
    }

    #[test]
    #[should_panic]
    fn test_match_invalid() {
        Match::new(5, 2);
    }

    // -- LineTerminator tests -----------------------------------------------

    #[test]
    fn test_line_terminator_default() {
        let lt = LineTerminator::default();
        assert_eq!(lt.as_byte(), b'\n');
        assert!(!lt.is_crlf());
    }

    #[test]
    fn test_line_terminator_crlf() {
        let lt = LineTerminator::CRLF;
        assert_eq!(lt.as_byte(), b'\n');
        assert!(lt.is_crlf());
    }

    #[test]
    fn test_line_terminator_custom_byte() {
        let lt = LineTerminator::Byte(b'\0');
        assert_eq!(lt.as_byte(), b'\0');
        assert!(!lt.is_crlf());
    }

    // -- ByteSet tests ------------------------------------------------------

    #[test]
    fn test_byte_set_empty() {
        let s = ByteSet::empty();
        for b in 0..=255u8 {
            assert!(!s.contains(b));
        }
    }

    #[test]
    fn test_byte_set_full() {
        let s = ByteSet::full();
        for b in 0..=255u8 {
            assert!(s.contains(b));
        }
    }

    #[test]
    fn test_byte_set_add_remove() {
        let mut s = ByteSet::new();
        s.add(b'a');
        assert!(s.contains(b'a'));
        assert!(!s.contains(b'b'));
        s.remove(b'a');
        assert!(!s.contains(b'a'));
    }

    // -- NoCaptures tests ---------------------------------------------------

    #[test]
    fn test_no_captures() {
        let nc = NoCaptures::new();
        assert_eq!(nc.group_len(), 0);
        assert!(nc.is_empty());
        assert_eq!(nc.group(0), None);
    }

    // -- Matcher trait tests (simple impl) ----------------------------------

    /// A trivial matcher that matches a fixed literal byte string.
    struct LiteralMatcher {
        needle: Vec<u8>,
    }

    impl Matcher for LiteralMatcher {
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
            let haystack_slice = &haystack[at..];
            match memchr::memmem::find(haystack_slice, &self.needle) {
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

    #[test]
    fn test_literal_matcher_find() {
        let m = LiteralMatcher {
            needle: b"foo".to_vec(),
        };
        let hay = b"hello foo bar foo baz";
        let result = m.find(hay).unwrap();
        assert_eq!(result, Some(Match::new(6, 9)));
    }

    #[test]
    fn test_literal_matcher_find_at() {
        let m = LiteralMatcher {
            needle: b"foo".to_vec(),
        };
        let hay = b"hello foo bar foo baz";
        let result = m.find_at(hay, 9).unwrap();
        assert_eq!(result, Some(Match::new(14, 17)));
    }

    #[test]
    fn test_literal_matcher_is_match() {
        let m = LiteralMatcher {
            needle: b"foo".to_vec(),
        };
        assert!(m.is_match(b"contains foo here").unwrap());
        assert!(!m.is_match(b"no match").unwrap());
    }

    #[test]
    fn test_literal_matcher_find_iter() {
        let m = LiteralMatcher {
            needle: b"ab".to_vec(),
        };
        let hay = b"ab cd ab ef ab";
        let mut matches = vec![];
        m.find_iter(hay, |mat| {
            matches.push(mat);
            true
        })
        .unwrap();
        assert_eq!(
            matches,
            vec![Match::new(0, 2), Match::new(6, 8), Match::new(12, 14)]
        );
    }

    #[test]
    fn test_literal_matcher_find_iter_early_stop() {
        let m = LiteralMatcher {
            needle: b"x".to_vec(),
        };
        let hay = b"x x x x";
        let mut count = 0;
        m.find_iter(hay, |_| {
            count += 1;
            count < 2 // stop after 2nd match
        })
        .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_literal_matcher_shortest_match() {
        let m = LiteralMatcher {
            needle: b"bar".to_vec(),
        };
        let hay = b"foo bar baz";
        assert_eq!(m.shortest_match(hay).unwrap(), Some(7));
    }

    #[test]
    fn test_literal_matcher_no_match() {
        let m = LiteralMatcher {
            needle: b"zzz".to_vec(),
        };
        let hay = b"hello world";
        assert_eq!(m.find(hay).unwrap(), None);
        assert!(!m.is_match(hay).unwrap());
        assert_eq!(m.shortest_match(hay).unwrap(), None);
    }

    #[test]
    fn test_literal_matcher_default_methods() {
        let m = LiteralMatcher {
            needle: b"x".to_vec(),
        };
        assert_eq!(m.capture_count(), 0);
        assert_eq!(m.capture_index("foo"), None);
        assert_eq!(m.line_terminator(), None);
        assert!(!m.non_matching_bytes().contains(b'x'));
    }
}
