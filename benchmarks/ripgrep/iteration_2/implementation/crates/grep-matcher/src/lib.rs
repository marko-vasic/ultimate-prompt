/*!
The `grep-matcher` crate provides the foundational trait abstractions for
regex matching used throughout the ripgrep search tool. It defines the
[`Matcher`] trait, which provides a generic interface over regex
implementations, as well as supporting types like [`Match`],
[`LineTerminator`], [`ByteSet`], [`Captures`], and [`LineMatchKind`].
*/

use std::fmt;
use std::io;

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

/// A `Match` records a (start, end) byte-offset pair representing a match
/// location in a haystack. The range is half-open: `[start, end)`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Match {
    start: usize,
    end: usize,
}

impl Match {
    /// Create a new match with the given byte offsets.
    ///
    /// # Panics
    ///
    /// This panics if `start > end`.
    #[inline]
    pub fn new(start: usize, end: usize) -> Match {
        assert!(start <= end, "start ({}) must be <= end ({})", start, end);
        Match { start, end }
    }

    /// Return a zero-width match at the given offset.
    #[inline]
    pub fn zero(offset: usize) -> Match {
        Match {
            start: offset,
            end: offset,
        }
    }

    /// Return the start byte offset of this match.
    #[inline]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Return the end byte offset of this match (exclusive).
    #[inline]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Return the length of this match, in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Return true if this match is empty (zero-width).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Offset this match by the given amount. Both the start and end are
    /// shifted by `amount`.
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

/// A line terminator configuration.
///
/// A line terminator determines how lines are segmented. Typically this is
/// `\n` on Unix and `\r\n` on Windows. The default line terminator is `\n`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LineTerminator {
    /// A single byte line terminator, such as `\n`.
    Byte(u8),
    /// CRLF (`\r\n`) line termination. When this mode is active, the primary
    /// line terminator byte is still `\n`, but lines ending with `\r\n` will
    /// have the `\r` stripped.
    CRLF,
}

impl Default for LineTerminator {
    #[inline]
    fn default() -> LineTerminator {
        LineTerminator::Byte(b'\n')
    }
}

impl LineTerminator {
    /// Return the primary byte value of this line terminator.
    ///
    /// For CRLF mode, this returns `\n`.
    #[inline]
    pub fn byte(&self) -> u8 {
        match *self {
            LineTerminator::Byte(b) => b,
            LineTerminator::CRLF => b'\n',
        }
    }

    /// Return true if this is CRLF mode.
    #[inline]
    pub fn is_crlf(&self) -> bool {
        matches!(*self, LineTerminator::CRLF)
    }

    /// Return this line terminator as a single byte.
    ///
    /// For CRLF, this returns `\n`.
    #[inline]
    pub fn as_byte(&self) -> u8 {
        self.byte()
    }

    /// Return the raw bytes that represent this line terminator.
    ///
    /// For CRLF, this returns `\r\n`. Otherwise, this returns a single byte.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match *self {
            LineTerminator::Byte(ref b) => std::slice::from_ref(b),
            LineTerminator::CRLF => &[b'\r', b'\n'],
        }
    }
}

// ---------------------------------------------------------------------------
// ByteSet
// ---------------------------------------------------------------------------

/// A set of bytes, useful for fast membership testing.
///
/// This is typically used to express the set of bytes that can never appear
/// in a match produced by a matcher.
#[derive(Clone, Debug)]
pub struct ByteSet(
    // 256 bits = 32 bytes, one bit per byte value.
    [u8; 32],
);

impl ByteSet {
    /// Create an empty byte set.
    pub fn empty() -> ByteSet {
        ByteSet([0; 32])
    }

    /// Create a full byte set (all 256 byte values present).
    pub fn full() -> ByteSet {
        ByteSet([0xFF; 32])
    }

    /// Insert a single byte into this set.
    #[inline]
    pub fn insert(&mut self, byte: u8) {
        let bucket = byte / 8;
        let bit = byte % 8;
        self.0[bucket as usize] |= 1 << bit;
    }

    /// Remove a single byte from this set.
    #[inline]
    pub fn remove(&mut self, byte: u8) {
        let bucket = byte / 8;
        let bit = byte % 8;
        self.0[bucket as usize] &= !(1 << bit);
    }

    /// Return true if this set contains the given byte.
    #[inline]
    pub fn contains(&self, byte: u8) -> bool {
        let bucket = byte / 8;
        let bit = byte % 8;
        (self.0[bucket as usize] >> bit) & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// LineMatchKind
// ---------------------------------------------------------------------------

/// The type of a match reported by a fast candidate line detection method.
///
/// This enum is used by [`Matcher::find_candidate_line`] to report either
/// a confirmed match or a candidate match that needs further verification.
#[derive(Clone, Copy, Debug)]
pub enum LineMatchKind {
    /// A confirmed match was found at the given byte offset. No further
    /// verification is needed.
    Confirmed(usize),
    /// A candidate match was found at the given byte offset. This may or
    /// may not be an actual match and requires further verification with
    /// the full regex.
    Candidate(usize),
}

// ---------------------------------------------------------------------------
// NoError
// ---------------------------------------------------------------------------

/// A zero-size error type that can be used when a `Matcher` implementation
/// can never produce errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoError(());

impl NoError {
    /// Create a new `NoError`.
    pub fn new() -> NoError {
        NoError(())
    }
}

impl fmt::Display for NoError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A NoError can never actually be constructed in a meaningful way,
        // but we still need to satisfy Display.
        Ok(())
    }
}

impl std::error::Error for NoError {}

impl From<NoError> for io::Error {
    fn from(_: NoError) -> io::Error {
        // This should never actually be called.
        panic!("NoError should never be converted to io::Error")
    }
}

// ---------------------------------------------------------------------------
// Captures trait
// ---------------------------------------------------------------------------

/// A trait that describes implementations of capturing groups.
///
/// When a search is executed, a `Captures` value can be used to report the
/// offsets of each matching capture group in the last search performed.
///
/// Capture group `0` always corresponds to the overall match. Other capture
/// groups are indexed starting from `1`.
pub trait Captures {
    /// Return the total number of capture groups. This includes the overall
    /// match, so the minimum value is `1` for matchers that support captures,
    /// and `0` for matchers that do not.
    fn len(&self) -> usize;

    /// Return the match for the capture group at the given index. If no match
    /// was found for that group, or if the index exceeds the number of groups,
    /// then `None` is returned.
    fn get(&self, i: usize) -> Option<Match>;

    /// Return true if this set of captures is empty (has no groups).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// NoCaptures
// ---------------------------------------------------------------------------

/// A `Captures` implementation that has zero groups.
///
/// This is useful for matchers that don't support capturing groups.
#[derive(Clone, Debug)]
pub struct NoCaptures(());

impl NoCaptures {
    /// Create a new empty set of captures.
    pub fn new() -> NoCaptures {
        NoCaptures(())
    }
}

impl Captures for NoCaptures {
    fn len(&self) -> usize {
        0
    }

    fn get(&self, _: usize) -> Option<Match> {
        None
    }
}

// ---------------------------------------------------------------------------
// Matcher trait
// ---------------------------------------------------------------------------

/// The `Matcher` trait defines an abstract interface for regular expression
/// matching on raw byte strings.
///
/// The trait provides two required methods: [`Matcher::find_at`] and
/// [`Matcher::new_captures`]. All other methods have default implementations
/// built on top of these two methods.
///
/// Implementors should provide more efficient implementations of the default
/// methods when possible, especially for methods like `captures_at`.
pub trait Matcher {
    /// The concrete type of capturing groups used by this matcher.
    type Captures: Captures;

    /// The error type used by this matcher.
    type Error: fmt::Display + fmt::Debug + Send + Sync + 'static;

    // ----- Required methods -----

    /// Execute a search starting at byte offset `at` in `haystack` and return
    /// the first match found, if any.
    ///
    /// The search should only consider matches that start at or after `at`,
    /// but may scan bytes before `at` for look-behind assertions.
    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Self::Error>;

    /// Create a new set of empty capture groups. The caller can then pass
    /// them to methods like [`Matcher::captures_at`].
    fn new_captures(&self) -> Result<Self::Captures, Self::Error>;

    // ----- Provided methods -----

    /// Find the first match in `haystack`.
    ///
    /// By default this calls `find_at(haystack, 0)`.
    fn find(
        &self,
        haystack: &[u8],
    ) -> Result<Option<Match>, Self::Error> {
        self.find_at(haystack, 0)
    }

    /// Execute a search and call `matched` for each successive non-overlapping
    /// match in `haystack`. If `matched` returns `false`, then iteration stops.
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

    /// Execute a search starting at `at` and call `matched` for each
    /// successive non-overlapping match. If `matched` returns `false`, then
    /// iteration stops.
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
                    // Advance past this match. If the match is empty, always
                    // advance by 1 byte to avoid an infinite loop.
                    if m.is_empty() {
                        pos = m.end() + 1;
                    } else {
                        pos = m.end();
                    }
                    if pos > haystack.len() {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Execute a search and call `matched` for each successive non-overlapping
    /// match. The callback may return an error, in which case the error is
    /// propagated through the outer `Result`.
    fn try_find_iter<F, E>(
        &self,
        haystack: &[u8],
        mut matched: F,
    ) -> Result<Result<(), E>, Self::Error>
    where
        F: FnMut(Match) -> Result<bool, E>,
    {
        let mut err = None;
        self.find_iter(haystack, |m| match matched(m) {
            Ok(cont) => cont,
            Err(e) => {
                err = Some(e);
                false
            }
        })?;
        match err {
            None => Ok(Ok(())),
            Some(e) => Ok(Err(e)),
        }
    }

    /// Populate capture groups for a match at byte offset `at` in `haystack`.
    ///
    /// Returns `true` if and only if a match was found.
    ///
    /// The default implementation simply calls `find_at` and, if a match is
    /// found, sets capture group 0 to the returned match. Matchers with
    /// real capture group support should override this.
    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        _caps: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        // By default, we just check if there's a match. Implementors with
        // real capture group support should override this.
        Ok(self.find_at(haystack, at)?.is_some())
    }

    /// Populate capture groups for the first match in `haystack`.
    fn captures(
        &self,
        haystack: &[u8],
        caps: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        self.captures_at(haystack, 0, caps)
    }

    /// Execute a search and call `matched` for each successive non-overlapping
    /// match, populating `caps` for each match. If `matched` returns `false`,
    /// iteration stops.
    fn captures_iter<F>(
        &self,
        haystack: &[u8],
        caps: &mut Self::Captures,
        matched: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self::Captures) -> bool,
    {
        self.captures_iter_at(haystack, 0, caps, matched)
    }

    /// Execute a search starting at `at` and call `matched` for each
    /// successive non-overlapping match, populating `caps` for each match.
    /// If `matched` returns `false`, iteration stops.
    fn captures_iter_at<F>(
        &self,
        haystack: &[u8],
        at: usize,
        caps: &mut Self::Captures,
        mut matched: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self::Captures) -> bool,
    {
        let mut pos = at;
        loop {
            // First, find the next match position so we can advance.
            let found = match self.find_at(haystack, pos)? {
                None => return Ok(()),
                Some(m) => m,
            };
            // Now populate captures for this match.
            if !self.captures_at(haystack, pos, caps)? {
                return Ok(());
            }
            if !matched(caps) {
                return Ok(());
            }
            // Advance past this match.
            if found.is_empty() {
                pos = found.end() + 1;
            } else {
                pos = found.end();
            }
            if pos > haystack.len() {
                return Ok(());
            }
        }
    }

    /// Replace every match in `haystack` with the result of calling `append`.
    ///
    /// The `append` callback receives the `Match` and a destination buffer
    /// `dst`. It should append the replacement content to `dst`. If `append`
    /// returns `true`, replacement continues. If it returns `false`,
    /// replacement stops and the remainder of `haystack` is copied to `dst`.
    fn replace<F>(
        &self,
        haystack: &[u8],
        dst: &mut Vec<u8>,
        mut append: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(Match, &mut Vec<u8>) -> bool,
    {
        let mut last_match_end = 0;
        self.find_iter(haystack, |m| {
            // Append everything between the last match and this one.
            dst.extend_from_slice(&haystack[last_match_end..m.start()]);
            last_match_end = m.end();
            append(m, dst)
        })?;
        // Append any remaining bytes after the last match.
        dst.extend_from_slice(&haystack[last_match_end..]);
        Ok(())
    }

    /// Replace every match in `haystack` with the result of calling `append`,
    /// with access to capture groups.
    ///
    /// Works like [`Matcher::replace`], but `append` receives `&Self::Captures`
    /// instead of a bare `Match`.
    fn replace_with_captures<F>(
        &self,
        haystack: &[u8],
        caps: &mut Self::Captures,
        dst: &mut Vec<u8>,
        mut append: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self::Captures, &mut Vec<u8>) -> bool,
    {
        let mut last_match_end = 0;
        self.captures_iter(haystack, caps, |caps| {
            // Use capture group 0 (overall match) to determine offsets.
            let m = match caps.get(0) {
                Some(m) => m,
                None => return false,
            };
            dst.extend_from_slice(&haystack[last_match_end..m.start()]);
            last_match_end = m.end();
            append(caps, dst)
        })?;
        dst.extend_from_slice(&haystack[last_match_end..]);
        Ok(())
    }

    /// Return true if and only if `haystack` contains a match.
    fn is_match(
        &self,
        haystack: &[u8],
    ) -> Result<bool, Self::Error> {
        self.is_match_at(haystack, 0)
    }

    /// Return true if and only if `haystack` contains a match starting at or
    /// after byte offset `at`.
    fn is_match_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<bool, Self::Error> {
        Ok(self.find_at(haystack, at)?.is_some())
    }

    /// Return the end offset of the shortest match, if one exists.
    ///
    /// By default, this returns the end of the first match found by `find`.
    fn shortest_match(
        &self,
        haystack: &[u8],
    ) -> Result<Option<usize>, Self::Error> {
        self.shortest_match_at(haystack, 0)
    }

    /// Return the end offset of the shortest match starting at or after `at`,
    /// if one exists.
    fn shortest_match_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<usize>, Self::Error> {
        Ok(self.find_at(haystack, at)?.map(|m| m.end()))
    }

    /// Return the set of bytes that can never appear in any match produced
    /// by this matcher.
    ///
    /// The default implementation returns `None`, meaning that any byte might
    /// appear in a match.
    fn non_matching_bytes(&self) -> Option<&ByteSet> {
        None
    }

    /// Return the line terminator configured in this matcher, if any.
    ///
    /// The line terminator is used by higher-level consumers (such as line
    /// searchers) to segment the haystack into lines. By default this returns
    /// `None`, which typically means `\n` is assumed.
    fn line_terminator(&self) -> Option<LineTerminator> {
        None
    }

    /// Return a fast candidate line match, if one exists.
    ///
    /// This is an optional optimization point. A matcher can use a faster,
    /// less precise search (such as a literal substring search) to quickly
    /// determine if a line is a candidate for matching. The caller can then
    /// run the full regex on the candidate line.
    ///
    /// The default implementation simply calls `find` and wraps the result
    /// in `LineMatchKind::Confirmed`.
    fn find_candidate_line(
        &self,
        haystack: &[u8],
    ) -> Result<Option<LineMatchKind>, Self::Error> {
        Ok(self.find(haystack)?.map(|m| LineMatchKind::Confirmed(m.start())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_new() {
        let m = Match::new(5, 10);
        assert_eq!(m.start(), 5);
        assert_eq!(m.end(), 10);
        assert_eq!(m.len(), 5);
        assert!(!m.is_empty());
    }

    #[test]
    fn test_match_zero() {
        let m = Match::zero(7);
        assert_eq!(m.start(), 7);
        assert_eq!(m.end(), 7);
        assert_eq!(m.len(), 0);
        assert!(m.is_empty());
    }

    #[test]
    fn test_match_offset() {
        let m = Match::new(5, 10);
        let m2 = m.offset(3);
        assert_eq!(m2.start(), 8);
        assert_eq!(m2.end(), 13);
    }

    #[test]
    #[should_panic]
    fn test_match_invalid() {
        Match::new(10, 5);
    }

    #[test]
    fn test_line_terminator_default() {
        let lt = LineTerminator::default();
        assert_eq!(lt.byte(), b'\n');
        assert!(!lt.is_crlf());
    }

    #[test]
    fn test_line_terminator_crlf() {
        let lt = LineTerminator::CRLF;
        assert_eq!(lt.byte(), b'\n');
        assert!(lt.is_crlf());
        assert_eq!(lt.as_bytes(), b"\r\n");
    }

    #[test]
    fn test_line_terminator_single_byte() {
        let lt = LineTerminator::Byte(b'\0');
        assert_eq!(lt.byte(), b'\0');
        assert!(!lt.is_crlf());
        assert_eq!(lt.as_bytes(), &[b'\0']);
    }

    #[test]
    fn test_byte_set() {
        let mut set = ByteSet::empty();
        assert!(!set.contains(b'a'));
        set.insert(b'a');
        assert!(set.contains(b'a'));
        assert!(!set.contains(b'b'));
        set.remove(b'a');
        assert!(!set.contains(b'a'));
    }

    #[test]
    fn test_byte_set_full() {
        let set = ByteSet::full();
        for b in 0..=255u8 {
            assert!(set.contains(b));
        }
    }

    #[test]
    fn test_no_captures() {
        let nc = NoCaptures::new();
        assert_eq!(nc.len(), 0);
        assert!(nc.is_empty());
        assert_eq!(nc.get(0), None);
    }

    #[test]
    fn test_no_error_display() {
        let e = NoError::new();
        assert_eq!(format!("{}", e), "");
    }
}
