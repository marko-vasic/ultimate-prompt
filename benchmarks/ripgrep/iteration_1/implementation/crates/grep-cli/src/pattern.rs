/// Pattern reading utilities.
///
/// This module provides functions for reading search patterns from various
/// sources (files, stdin, or any `Read` implementor), one pattern per line.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

/// Read patterns from a reader, one per line.
///
/// Each line is treated as a separate pattern. Trailing `\r\n` or `\n`
/// line endings are stripped. Empty lines are skipped.
///
/// # Examples
///
/// ```
/// use grep_cli::patterns_from_reader;
///
/// let input = b"foo\nbar\n\nbaz\n";
/// let patterns = patterns_from_reader(&input[..]).unwrap();
/// assert_eq!(patterns, vec!["foo", "bar", "baz"]);
/// ```
pub fn patterns_from_reader<R: Read>(rdr: R) -> io::Result<Vec<String>> {
    let mut patterns = Vec::new();
    let reader = BufReader::new(rdr);
    for line in reader.lines() {
        let line = line?;
        // Strip trailing \r if present (handles \r\n on readers that
        // don't normalize line endings, though BufReader::lines() already
        // strips \n).
        let line = line.strip_suffix('\r').unwrap_or(&line).to_string();
        if !line.is_empty() {
            patterns.push(line);
        }
    }
    Ok(patterns)
}

/// Read patterns from a file, one per line.
///
/// This is a convenience wrapper around [`patterns_from_reader`] that
/// opens the given file path.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub fn patterns_from_path(path: &Path) -> io::Result<Vec<String>> {
    let file = File::open(path)?;
    patterns_from_reader(file)
}

/// Read patterns from stdin, one per line.
///
/// This is a convenience wrapper around [`patterns_from_reader`] that
/// reads from standard input. Note that this will block until EOF is
/// reached on stdin.
pub fn patterns_from_stdin() -> io::Result<Vec<String>> {
    let stdin = io::stdin();
    let handle = stdin.lock();
    patterns_from_reader(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_patterns() {
        let input = b"foo\nbar\nbaz\n";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert_eq!(patterns, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_empty_lines_skipped() {
        let input = b"foo\n\nbar\n\n\nbaz\n";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert_eq!(patterns, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_crlf_endings() {
        let input = b"foo\r\nbar\r\nbaz\r\n";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert_eq!(patterns, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_no_trailing_newline() {
        let input = b"foo\nbar";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert_eq!(patterns, vec!["foo", "bar"]);
    }

    #[test]
    fn test_empty_input() {
        let input = b"";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_only_empty_lines() {
        let input = b"\n\n\n";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert!(patterns.is_empty());
    }
}
