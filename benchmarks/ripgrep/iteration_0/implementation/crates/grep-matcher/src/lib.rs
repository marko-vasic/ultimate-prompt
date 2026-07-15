//! # grep-matcher
//!
//! This crate defines the abstract regex interface — trait definitions that all
//! regex engines must implement. It provides the foundational types and traits
//! used throughout the grep pipeline for matching patterns in byte strings.
//!
//! The core abstraction is the [`Matcher`] trait, which defines how a regex
//! engine finds matches, iterates over them, and captures groups. Concrete
//! regex implementations (e.g., based on `regex` or `regex-automata`) implement
//! this trait to plug into the grep pipeline.
//!
//! This crate has **zero** external dependencies.

#![deny(missing_docs)]

use std::fmt;
use std::hash::Hash;

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

/// Represents a contiguous match in a haystack, defined by byte offsets.
///
/// A `Match` value stores the start (inclusive) and end (exclusive) byte
/// offsets of a match within a haystack. This is the same half-open interval
/// convention used by `std::ops::Range<usize>`.
///
/// # Example
///
/// ```
/// use grep_matcher::Match;
///
/// let m = Match::new(2, 5);
/// assert_eq!(m.start(), 2);
/// assert_eq!(m.end(), 5);
/// assert_eq!(m.len(), 3);
/// assert!(!m.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Match {
    start: usize,
    end: usize,
}

impl Match {
    /// Create a new `Match` from the given byte offsets.
    ///
    /// `start` is the inclusive start offset and `end` is the exclusive end
    /// offset. It is the caller's responsibility to ensure `start <= end`.
    #[inline]
    pub fn new(start: usize, end: usize) -> Match {
        debug_assert!(start <= end, "Match::new requires start <= end");
        Match { start, end }
    }

    /// Returns the inclusive start byte offset of this match.
    #[inline]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Returns the exclusive end byte offset of this match.
    #[inline]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Returns the length of this match in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns `true` if this match is empty (zero-width).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

// ---------------------------------------------------------------------------
// LineTerminator
// ---------------------------------------------------------------------------

/// Internal representation of the line terminator kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum LineTerminatorKind {
    /// A single byte line terminator.
    Byte(u8),
    /// CRLF line terminator (`\r\n`). The "primary" byte is `\n`.
    Crlf,
}

/// A configurable line terminator.
///
/// A line terminator determines how lines are separated in the input being
/// searched. By default, the line terminator is `\n` (Unix-style). The
/// alternative is CRLF (`\r\n`), which is common on Windows.
///
/// Some operations in the grep pipeline need to know the line terminator in
/// order to correctly split input into lines or to avoid matching across line
/// boundaries.
///
/// # Example
///
/// ```
/// use grep_matcher::LineTerminator;
///
/// let lt = LineTerminator::byte(b'\n');
/// assert_eq!(lt.as_byte(), b'\n');
/// assert!(!lt.is_crlf());
///
/// let crlf = LineTerminator::crlf();
/// assert_eq!(crlf.as_byte(), b'\n');
/// assert!(crlf.is_crlf());
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LineTerminator {
    kind: LineTerminatorKind,
}

impl LineTerminator {
    /// Create a line terminator from a single byte value.
    ///
    /// The most common value is `b'\n'`.
    #[inline]
    pub fn byte(b: u8) -> LineTerminator {
        LineTerminator {
            kind: LineTerminatorKind::Byte(b),
        }
    }

    /// Create a CRLF line terminator.
    ///
    /// In CRLF mode, the primary byte is `\n`, but line splitting should
    /// also strip trailing `\r` characters.
    #[inline]
    pub fn crlf() -> LineTerminator {
        LineTerminator {
            kind: LineTerminatorKind::Crlf,
        }
    }

    /// Returns the primary byte for this line terminator.
    ///
    /// For single-byte terminators this returns the byte itself. For CRLF
    /// mode this returns `\n`.
    #[inline]
    pub fn as_byte(&self) -> u8 {
        match self.kind {
            LineTerminatorKind::Byte(b) => b,
            LineTerminatorKind::Crlf => b'\n',
        }
    }

    /// Returns `true` if this line terminator is in CRLF mode.
    #[inline]
    pub fn is_crlf(&self) -> bool {
        matches!(self.kind, LineTerminatorKind::Crlf)
    }

    /// Returns the byte sequence for this line terminator.
    ///
    /// For a single-byte terminator, this returns a single-element slice.
    /// For CRLF, this returns `b"\r\n"`.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match self.kind {
            LineTerminatorKind::Byte(ref b) => std::slice::from_ref(b),
            LineTerminatorKind::Crlf => &[b'\r', b'\n'],
        }
    }
}

impl Default for LineTerminator {
    /// The default line terminator is `\n`.
    #[inline]
    fn default() -> LineTerminator {
        LineTerminator::byte(b'\n')
    }
}

// ---------------------------------------------------------------------------
// ByteSet
// ---------------------------------------------------------------------------

/// A fast set for byte values using a 256-bit bitmap.
///
/// This data structure provides O(1) membership testing for any byte value.
/// It is used, for example, to describe the set of bytes that can never
/// appear in a match — enabling fast prefiltering.
///
/// # Example
///
/// ```
/// use grep_matcher::ByteSet;
///
/// let mut set = ByteSet::empty();
/// assert!(!set.contains(b'a'));
/// set.add(b'a');
/// assert!(set.contains(b'a'));
/// set.remove(b'a');
/// assert!(!set.contains(b'a'));
/// ```
#[derive(Clone, Debug)]
pub struct ByteSet {
    /// 256 bits stored as four `u64` values.
    bits: [u64; 4],
}

impl ByteSet {
    /// Create an empty byte set (no bytes are members).
    #[inline]
    pub fn empty() -> ByteSet {
        ByteSet { bits: [0; 4] }
    }

    /// Create a full byte set (all 256 byte values are members).
    #[inline]
    pub fn full() -> ByteSet {
        ByteSet {
            bits: [!0u64; 4],
        }
    }

    /// Add a byte to this set.
    #[inline]
    pub fn add(&mut self, byte: u8) {
        let (bucket, bit) = Self::position(byte);
        self.bits[bucket] |= 1u64 << bit;
    }

    /// Remove a byte from this set.
    #[inline]
    pub fn remove(&mut self, byte: u8) {
        let (bucket, bit) = Self::position(byte);
        self.bits[bucket] &= !(1u64 << bit);
    }

    /// Returns `true` if the given byte is a member of this set.
    #[inline]
    pub fn contains(&self, byte: u8) -> bool {
        let (bucket, bit) = Self::position(byte);
        (self.bits[bucket] >> bit) & 1 == 1
    }

    /// Compute the bucket index (0–3) and bit position (0–63) for a byte.
    #[inline]
    fn position(byte: u8) -> (usize, u32) {
        let byte = byte as usize;
        (byte / 64, (byte % 64) as u32)
    }
}

// ---------------------------------------------------------------------------
// Captures trait
// ---------------------------------------------------------------------------

/// A trait for accessing capture group information after a match.
///
/// Capture groups are numbered starting at 0, where capture group 0
/// conventionally refers to the overall match. Implementations should
/// return `None` for capture groups that did not participate in the match.
pub trait Captures {
    /// Returns the total number of capture groups.
    ///
    /// This should include capture group 0 (the overall match) if it exists.
    fn len(&self) -> usize;

    /// Returns the match for the `i`-th capture group, or `None` if the group
    /// did not participate in the match or `i` is out of bounds.
    fn get(&self, i: usize) -> Option<Match>;

    /// Returns `true` if there are no capture groups.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// NoCaptures
// ---------------------------------------------------------------------------

/// A dummy implementation of [`Captures`] that always returns `None`.
///
/// This is useful as a placeholder when captures are not needed. It reports
/// a length of 0 and always returns `None` for any capture group index.
///
/// # Example
///
/// ```
/// use grep_matcher::{Captures, NoCaptures};
///
/// let caps = NoCaptures::new();
/// assert_eq!(caps.len(), 0);
/// assert!(caps.is_empty());
/// assert!(caps.get(0).is_none());
/// ```
#[derive(Clone, Debug, Default)]
pub struct NoCaptures(());

impl NoCaptures {
    /// Create a new `NoCaptures` value.
    #[inline]
    pub fn new() -> NoCaptures {
        NoCaptures(())
    }
}

impl Captures for NoCaptures {
    #[inline]
    fn len(&self) -> usize {
        0
    }

    #[inline]
    fn get(&self, _i: usize) -> Option<Match> {
        None
    }
}

// ---------------------------------------------------------------------------
// NoError
// ---------------------------------------------------------------------------

/// An error type that can never be constructed.
///
/// This is useful as the `Error` associated type for [`Matcher`]
/// implementations that are infallible and can never produce an error.
/// Because this enum has no variants, it can never be instantiated.
///
/// # Example
///
/// ```
/// use grep_matcher::NoError;
///
/// fn never_fails() -> Result<(), NoError> {
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub enum NoError {}

impl fmt::Display for NoError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This can never be called because NoError has no variants.
        match *self {}
    }
}

impl std::error::Error for NoError {
    fn description(&self) -> &str {
        // This can never be called because NoError has no variants.
        match *self {}
    }
}

// ---------------------------------------------------------------------------
// Matcher trait
// ---------------------------------------------------------------------------

/// The core matching abstraction for the grep pipeline.
///
/// The `Matcher` trait defines how a regex engine finds matches in a byte
/// string (haystack). Implementors provide the fundamental `find_at` and
/// `new_captures` methods, while many convenience methods have default
/// implementations built on top of those primitives.
///
/// # Associated Types
///
/// - `Captures`: The type used to report capture group matches.
/// - `Error`: The error type returned by matching operations.
///
/// # Required Methods
///
/// - [`find_at`](Matcher::find_at): Find the next match starting at a given
///   byte position.
/// - [`new_captures`](Matcher::new_captures): Create a fresh captures object.
///
/// # Provided Methods
///
/// Many convenience methods have default implementations. Implementors are
/// free to override any of them for performance.
pub trait Matcher {
    /// The type used for reporting capture group information.
    type Captures: Captures;

    /// The error type produced by this matcher.
    type Error: fmt::Display + fmt::Debug + Send + Sync + 'static;

    // ----- Required methods -----

    /// Find the next match in `haystack` starting at byte position `at`.
    ///
    /// Returns `Ok(Some(m))` if a match was found, where `m` describes the
    /// byte offsets of the match within the haystack. Returns `Ok(None)` if
    /// no match exists at or after position `at`. Returns `Err(...)` if an
    /// error occurred during matching.
    ///
    /// The match offsets are always relative to the start of `haystack`, not
    /// relative to `at`.
    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Self::Error>;

    /// Create a new, empty captures object appropriate for this matcher.
    ///
    /// The returned captures object can be reused across multiple calls to
    /// [`captures_at`](Matcher::captures_at).
    fn new_captures(&self) -> Result<Self::Captures, Self::Error>;

    // ----- Provided methods -----

    /// Find the first match in the entire haystack.
    ///
    /// This is a convenience method equivalent to `self.find_at(haystack, 0)`.
    #[inline]
    fn find(
        &self,
        haystack: &[u8],
    ) -> Result<Option<Match>, Self::Error> {
        self.find_at(haystack, 0)
    }

    /// Iterate over all non-overlapping matches in `haystack`, calling
    /// `matched` for each one.
    ///
    /// Iteration stops when `matched` returns `false` or when no more
    /// matches are found.
    ///
    /// This is equivalent to `self.find_iter_at(haystack, 0, matched)`.
    #[inline]
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

    /// Iterate over all non-overlapping matches in `haystack` starting at
    /// byte position `at`, calling `matched` for each one.
    ///
    /// Iteration stops when `matched` returns `false` or when no more
    /// matches are found.
    ///
    /// The default implementation correctly handles zero-width matches by
    /// advancing the search position by one byte to prevent infinite loops.
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
                    // Handle zero-width matches: advance by at least 1 byte
                    // to avoid an infinite loop.
                    if m.is_empty() {
                        pos = m.end() + 1;
                        // If we've gone past the haystack, stop.
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

    /// Populate `caps` with capture group information for a match starting
    /// at byte position `at`.
    ///
    /// Returns `Ok(true)` if a match was found and captures were populated,
    /// or `Ok(false)` if no match was found.
    ///
    /// The default implementation simply calls [`find_at`](Matcher::find_at)
    /// and, if a match is found, does not populate any captures beyond the
    /// overall match. Implementors should override this to provide real
    /// capture group data.
    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        _caps: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        Ok(self.find_at(haystack, at)?.is_some())
    }

    /// Populate `caps` with capture group information for the first match
    /// in the entire haystack.
    ///
    /// This is a convenience method equivalent to
    /// `self.captures_at(haystack, 0, caps)`.
    #[inline]
    fn captures(
        &self,
        haystack: &[u8],
        caps: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        self.captures_at(haystack, 0, caps)
    }

    /// Returns the total number of capture groups in this matcher's pattern.
    ///
    /// The default implementation returns `0`, indicating no capture groups
    /// (beyond the implicit overall match).
    #[inline]
    fn capture_count(&self) -> usize {
        0
    }

    /// Returns the capture group index for the given capture group name.
    ///
    /// Returns `None` if no capture group with the given name exists. The
    /// default implementation always returns `None`.
    #[inline]
    fn capture_index(&self, _name: &str) -> Option<usize> {
        None
    }

    /// Iterate over all non-overlapping matches, calling a fallible
    /// callback for each match.
    ///
    /// Unlike [`find_iter`](Matcher::find_iter), the `matched` callback
    /// returns a `Result<bool, E>`. If the callback returns `Err(e)`, the
    /// error is propagated as `Ok(Err(e))`. Matcher errors are propagated
    /// as `Err(...)`.
    fn try_find_iter<F, E>(
        &self,
        haystack: &[u8],
        mut matched: F,
    ) -> Result<Result<(), E>, Self::Error>
    where
        F: FnMut(Match) -> Result<bool, E>,
    {
        let mut pos = 0;
        loop {
            match self.find_at(haystack, pos)? {
                None => return Ok(Ok(())),
                Some(m) => {
                    match matched(m) {
                        Err(e) => return Ok(Err(e)),
                        Ok(false) => return Ok(Ok(())),
                        Ok(true) => {}
                    }
                    // Handle zero-width matches.
                    if m.is_empty() {
                        pos = m.end() + 1;
                        if pos > haystack.len() {
                            return Ok(Ok(()));
                        }
                    } else {
                        pos = m.end();
                    }
                }
            }
        }
    }

    /// Returns the end byte offset of the shortest match starting at
    /// position 0.
    ///
    /// The default implementation calls [`find`](Matcher::find) and returns
    /// the end offset of the match, if any.
    #[inline]
    fn shortest_match(
        &self,
        haystack: &[u8],
    ) -> Result<Option<usize>, Self::Error> {
        self.shortest_match_at(haystack, 0)
    }

    /// Returns the end byte offset of the shortest match starting at
    /// the given position `at`.
    ///
    /// The default implementation calls [`find_at`](Matcher::find_at) and
    /// returns the end offset of the match, if any.
    #[inline]
    fn shortest_match_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<usize>, Self::Error> {
        Ok(self.find_at(haystack, at)?.map(|m| m.end()))
    }

    /// Returns the set of bytes that are guaranteed to never appear in any
    /// match produced by this matcher.
    ///
    /// This information can be used as a fast prefilter to skip portions of
    /// the haystack. The default implementation returns `None`, indicating
    /// that no such information is available.
    #[inline]
    fn non_matching_bytes(&self) -> Option<&ByteSet> {
        None
    }

    /// Returns the line terminator used by this matcher, if any.
    ///
    /// When set, the matcher guarantees that it will never produce a match
    /// that contains the line terminator. This allows line-oriented
    /// searching to work correctly.
    ///
    /// The default implementation returns `None`.
    #[inline]
    fn line_terminator(&self) -> Option<LineTerminator> {
        None
    }

    /// Returns a candidate match for use as a fast prefilter.
    ///
    /// A candidate match is a potential match that may or may not be a
    /// true match. Callers should verify candidates with a full match
    /// operation. The default implementation simply calls
    /// [`find`](Matcher::find).
    #[inline]
    fn find_candidate(
        &self,
        haystack: &[u8],
    ) -> Result<Option<Match>, Self::Error> {
        self.find(haystack)
    }

    /// Returns a candidate match starting at position `at` for use as a
    /// fast prefilter.
    ///
    /// The default implementation simply calls
    /// [`find_at`](Matcher::find_at).
    #[inline]
    fn find_candidate_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Self::Error> {
        self.find_at(haystack, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Match tests ---

    #[test]
    fn test_match_basic() {
        let m = Match::new(2, 5);
        assert_eq!(m.start(), 2);
        assert_eq!(m.end(), 5);
        assert_eq!(m.len(), 3);
        assert!(!m.is_empty());
    }

    #[test]
    fn test_match_empty() {
        let m = Match::new(3, 3);
        assert_eq!(m.len(), 0);
        assert!(m.is_empty());
    }

    #[test]
    fn test_match_eq() {
        let a = Match::new(1, 4);
        let b = Match::new(1, 4);
        let c = Match::new(1, 5);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_match_copy() {
        let a = Match::new(0, 10);
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn test_match_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Match::new(0, 1));
        set.insert(Match::new(0, 1));
        assert_eq!(set.len(), 1);
    }

    // --- LineTerminator tests ---

    #[test]
    fn test_line_terminator_byte() {
        let lt = LineTerminator::byte(b'\n');
        assert_eq!(lt.as_byte(), b'\n');
        assert!(!lt.is_crlf());
        assert_eq!(lt.as_bytes(), &[b'\n']);
    }

    #[test]
    fn test_line_terminator_crlf() {
        let lt = LineTerminator::crlf();
        assert_eq!(lt.as_byte(), b'\n');
        assert!(lt.is_crlf());
        assert_eq!(lt.as_bytes(), b"\r\n");
    }

    #[test]
    fn test_line_terminator_default() {
        let lt = LineTerminator::default();
        assert_eq!(lt.as_byte(), b'\n');
        assert!(!lt.is_crlf());
    }

    #[test]
    fn test_line_terminator_custom_byte() {
        let lt = LineTerminator::byte(b'\0');
        assert_eq!(lt.as_byte(), b'\0');
        assert!(!lt.is_crlf());
        assert_eq!(lt.as_bytes(), &[b'\0']);
    }

    // --- ByteSet tests ---

    #[test]
    fn test_byte_set_empty() {
        let set = ByteSet::empty();
        for b in 0u8..=255 {
            assert!(!set.contains(b));
        }
    }

    #[test]
    fn test_byte_set_full() {
        let set = ByteSet::full();
        for b in 0u8..=255 {
            assert!(set.contains(b));
        }
    }

    #[test]
    fn test_byte_set_add_remove() {
        let mut set = ByteSet::empty();
        set.add(b'a');
        set.add(b'z');
        set.add(0);
        set.add(255);
        assert!(set.contains(b'a'));
        assert!(set.contains(b'z'));
        assert!(set.contains(0));
        assert!(set.contains(255));
        assert!(!set.contains(b'b'));

        set.remove(b'a');
        assert!(!set.contains(b'a'));
        assert!(set.contains(b'z'));
    }

    #[test]
    fn test_byte_set_boundary_values() {
        let mut set = ByteSet::empty();
        // Test values at bucket boundaries: 0, 63, 64, 127, 128, 191, 192, 255
        let boundaries = [0u8, 63, 64, 127, 128, 191, 192, 255];
        for &b in &boundaries {
            set.add(b);
        }
        for &b in &boundaries {
            assert!(set.contains(b), "expected {} to be in set", b);
        }
        // Spot check some non-boundary values are not in the set
        assert!(!set.contains(1));
        assert!(!set.contains(100));
    }

    // --- NoCaptures tests ---

    #[test]
    fn test_no_captures() {
        let caps = NoCaptures::new();
        assert_eq!(caps.len(), 0);
        assert!(caps.is_empty());
        assert!(caps.get(0).is_none());
        assert!(caps.get(100).is_none());
    }

    // --- NoError tests ---

    #[test]
    fn test_no_error_is_uninhabited() {
        // We can't construct a NoError, but we can verify the type exists
        // and works with Result.
        let result: Result<u32, NoError> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    // --- Matcher trait integration test ---

    /// A trivial matcher that searches for a literal byte sequence.
    struct LiteralMatcher {
        needle: Vec<u8>,
    }

    impl LiteralMatcher {
        fn new(needle: &[u8]) -> LiteralMatcher {
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

    #[test]
    fn test_matcher_find() {
        let m = LiteralMatcher::new(b"fox");
        let hay = b"the quick brown fox jumps";
        let result = m.find(hay).unwrap().unwrap();
        assert_eq!(result.start(), 16);
        assert_eq!(result.end(), 19);
    }

    #[test]
    fn test_matcher_find_at() {
        let m = LiteralMatcher::new(b"ab");
        let hay = b"ababab";
        let r1 = m.find_at(hay, 0).unwrap().unwrap();
        assert_eq!(r1.start(), 0);
        assert_eq!(r1.end(), 2);

        let r2 = m.find_at(hay, 1).unwrap().unwrap();
        assert_eq!(r2.start(), 2);
        assert_eq!(r2.end(), 4);

        let r3 = m.find_at(hay, 5).unwrap();
        assert!(r3.is_none());
    }

    #[test]
    fn test_matcher_find_no_match() {
        let m = LiteralMatcher::new(b"xyz");
        let hay = b"hello world";
        assert!(m.find(hay).unwrap().is_none());
    }

    #[test]
    fn test_matcher_find_iter() {
        let m = LiteralMatcher::new(b"ab");
        let hay = b"abcabc";
        let mut matches = vec![];
        m.find_iter(hay, |mat| {
            matches.push(mat);
            true
        })
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], Match::new(0, 2));
        assert_eq!(matches[1], Match::new(3, 5));
    }

    #[test]
    fn test_matcher_find_iter_early_stop() {
        let m = LiteralMatcher::new(b"a");
        let hay = b"aaaa";
        let mut matches = vec![];
        m.find_iter(hay, |mat| {
            matches.push(mat);
            matches.len() < 2
        })
        .unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_matcher_find_iter_zero_width() {
        // With an empty needle, every position matches. The iterator
        // should advance by 1 byte for each zero-width match and not loop
        // infinitely.
        let m = LiteralMatcher::new(b"");
        let hay = b"ab";
        let mut matches = vec![];
        m.find_iter(hay, |mat| {
            matches.push(mat);
            true
        })
        .unwrap();
        // Positions 0, 1, 2 are valid for zero-width matches in "ab" (len=2)
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0], Match::new(0, 0));
        assert_eq!(matches[1], Match::new(1, 1));
        assert_eq!(matches[2], Match::new(2, 2));
    }

    #[test]
    fn test_matcher_captures() {
        let m = LiteralMatcher::new(b"hi");
        let hay = b"say hi there";
        let mut caps = m.new_captures().unwrap();
        assert!(m.captures(hay, &mut caps).unwrap());
    }

    #[test]
    fn test_matcher_captures_no_match() {
        let m = LiteralMatcher::new(b"xyz");
        let hay = b"hello";
        let mut caps = m.new_captures().unwrap();
        assert!(!m.captures(hay, &mut caps).unwrap());
    }

    #[test]
    fn test_matcher_shortest_match() {
        let m = LiteralMatcher::new(b"oo");
        let hay = b"foobar";
        let end = m.shortest_match(hay).unwrap().unwrap();
        assert_eq!(end, 3);
    }

    #[test]
    fn test_matcher_shortest_match_at() {
        let m = LiteralMatcher::new(b"oo");
        let hay = b"foobar";
        assert!(m.shortest_match_at(hay, 2).unwrap().is_none());
    }

    #[test]
    fn test_matcher_capture_count_default() {
        let m = LiteralMatcher::new(b"x");
        assert_eq!(m.capture_count(), 0);
    }

    #[test]
    fn test_matcher_capture_index_default() {
        let m = LiteralMatcher::new(b"x");
        assert!(m.capture_index("foo").is_none());
    }

    #[test]
    fn test_matcher_non_matching_bytes_default() {
        let m = LiteralMatcher::new(b"x");
        assert!(m.non_matching_bytes().is_none());
    }

    #[test]
    fn test_matcher_line_terminator_default() {
        let m = LiteralMatcher::new(b"x");
        assert!(m.line_terminator().is_none());
    }

    #[test]
    fn test_matcher_find_candidate_default() {
        let m = LiteralMatcher::new(b"ab");
        let hay = b"xxab";
        let r = m.find_candidate(hay).unwrap().unwrap();
        assert_eq!(r, Match::new(2, 4));
    }

    #[test]
    fn test_matcher_find_candidate_at_default() {
        let m = LiteralMatcher::new(b"ab");
        let hay = b"abxxab";
        let r = m.find_candidate_at(hay, 1).unwrap().unwrap();
        assert_eq!(r, Match::new(4, 6));
    }

    #[test]
    fn test_matcher_try_find_iter() {
        let m = LiteralMatcher::new(b"x");
        let hay = b"xyx";
        let mut matches = vec![];
        let result: Result<Result<(), &str>, NoError> =
            m.try_find_iter(hay, |mat| -> Result<bool, &str> {
                matches.push(mat);
                Ok(true)
            });
        assert!(result.unwrap().is_ok());
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_matcher_try_find_iter_error() {
        let m = LiteralMatcher::new(b"x");
        let hay = b"xyx";
        let result: Result<Result<(), &str>, NoError> =
            m.try_find_iter(hay, |_mat| -> Result<bool, &str> {
                Err("callback error")
            });
        assert_eq!(result.unwrap().unwrap_err(), "callback error");
    }
}
