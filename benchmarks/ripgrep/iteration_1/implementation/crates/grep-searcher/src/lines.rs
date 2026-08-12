/*!
Line-oriented utility functions.

This module provides helpers for working with lines in a byte buffer:
finding line boundaries, stripping terminators, and locating preceding lines.
*/

use grep_matcher::{LineTerminator, Match};

/// Strips the line terminator from the end of `bytes`, if present.
///
/// If the line terminator is CRLF, both `\r\n` and a lone `\n` are stripped.
/// For single-byte terminators, only that byte is stripped.
pub fn without_terminator(bytes: &[u8], lt: LineTerminator) -> &[u8] {
    let mut end = bytes.len();
    if end == 0 {
        return bytes;
    }
    if lt.is_crlf() {
        if end >= 1 && bytes[end - 1] == b'\n' {
            end -= 1;
        }
        if end >= 1 && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    } else {
        let term = lt.as_byte();
        if bytes[end - 1] == term {
            end -= 1;
        }
    }
    &bytes[..end]
}

/// Expands a match range to cover the full line(s) containing it.
///
/// Given a match `range` within `bytes`, this function expands the range
/// backwards to the byte after the previous line terminator (or start of buffer)
/// and forwards to include the line terminator at the end (or end of buffer).
///
/// `lt` is the line terminator byte (e.g. `b'\n'`).
pub fn locate(bytes: &[u8], lt: u8, range: Match) -> Match {
    let start = match memchr::memrchr(lt, &bytes[..range.start()]) {
        Some(pos) => pos + 1,
        None => 0,
    };
    let end = match memchr::memchr(lt, &bytes[range.end()..]) {
        Some(pos) => range.end() + pos + 1,
        None => bytes.len(),
    };
    Match::new(start, end)
}

/// Returns the byte offset within `bytes` of the start of the line that is
/// `count` lines before the end of `bytes`.
///
/// If there are fewer than `count` lines before the end, returns `0`.
///
/// `lt` is the line terminator byte.
pub fn preceding(bytes: &[u8], lt: u8, count: usize) -> usize {
    if count == 0 || bytes.is_empty() {
        return bytes.len();
    }

    let mut pos = bytes.len();
    let mut remaining = count;

    // If the last byte is a line terminator, skip it so we don't count an
    // empty trailing "line".
    if pos > 0 && bytes[pos - 1] == lt {
        pos -= 1;
    }

    while remaining > 0 {
        match memchr::memrchr(lt, &bytes[..pos]) {
            Some(i) => {
                remaining -= 1;
                if remaining == 0 {
                    return i + 1;
                }
                pos = i;
            }
            None => {
                return 0;
            }
        }
    }
    0
}

/// An iterator over lines in a byte slice.
///
/// Each yielded item is a `(start, end)` pair indicating the byte range of
/// the line, including its terminator if present.
pub struct LineIter<'a> {
    bytes: &'a [u8],
    lt: u8,
    pos: usize,
}

impl<'a> LineIter<'a> {
    /// Creates a new line iterator over the given bytes using the specified
    /// line terminator byte.
    pub fn new(bytes: &'a [u8], lt: u8) -> Self {
        LineIter { bytes, lt, pos: 0 }
    }
}

impl<'a> Iterator for LineIter<'a> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let start = self.pos;
        match memchr::memchr(self.lt, &self.bytes[self.pos..]) {
            Some(rel) => {
                let end = self.pos + rel + 1;
                self.pos = end;
                Some((start, end))
            }
            None => {
                let end = self.bytes.len();
                self.pos = end;
                Some((start, end))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_without_terminator_lf() {
        let lt = LineTerminator::default();
        assert_eq!(without_terminator(b"hello\n", lt), b"hello");
        assert_eq!(without_terminator(b"hello", lt), b"hello");
        assert_eq!(without_terminator(b"\n", lt), b"");
        assert_eq!(without_terminator(b"", lt), b"");
    }

    #[test]
    fn test_without_terminator_crlf() {
        let lt = LineTerminator::CRLF;
        assert_eq!(without_terminator(b"hello\r\n", lt), b"hello");
        assert_eq!(without_terminator(b"hello\n", lt), b"hello");
        assert_eq!(without_terminator(b"hello", lt), b"hello");
    }

    #[test]
    fn test_locate() {
        let bytes = b"aaa\nbbb\nccc\n";
        // Match within "bbb\n"
        let m = Match::new(4, 6);
        let expanded = locate(bytes, b'\n', m);
        assert_eq!(expanded.start(), 4);
        assert_eq!(expanded.end(), 8);
    }

    #[test]
    fn test_locate_at_start() {
        let bytes = b"aaa\nbbb\n";
        let m = Match::new(0, 2);
        let expanded = locate(bytes, b'\n', m);
        assert_eq!(expanded.start(), 0);
        assert_eq!(expanded.end(), 4);
    }

    #[test]
    fn test_preceding() {
        let bytes = b"aaa\nbbb\nccc\n";
        assert_eq!(preceding(bytes, b'\n', 1), 8);
        assert_eq!(preceding(bytes, b'\n', 2), 4);
        assert_eq!(preceding(bytes, b'\n', 3), 0);
        assert_eq!(preceding(bytes, b'\n', 10), 0);
    }

    #[test]
    fn test_line_iter() {
        let bytes = b"a\nbb\nccc";
        let lines: Vec<_> = LineIter::new(bytes, b'\n').collect();
        assert_eq!(lines, vec![(0, 2), (2, 5), (5, 8)]);
    }

    #[test]
    fn test_line_iter_trailing_newline() {
        let bytes = b"a\nb\n";
        let lines: Vec<_> = LineIter::new(bytes, b'\n').collect();
        assert_eq!(lines, vec![(0, 2), (2, 4)]);
    }
}
