/// Escape and unescape utilities for byte strings.
///
/// These functions convert between raw byte sequences and their
/// human-readable escaped representations, using `\xNN` notation for
/// non-printable bytes and standard escape sequences like `\n`, `\t`, etc.

use std::ffi::OsStr;

/// Escape a byte slice into a human-readable string.
///
/// Printable ASCII bytes are left as-is. Non-printable bytes are escaped
/// using `\xNN` hexadecimal notation. Standard escape sequences are used
/// for common control characters:
///
/// - `\0` for null (0x00)
/// - `\t` for tab (0x09)
/// - `\n` for newline (0x0A)
/// - `\r` for carriage return (0x0D)
/// - `\\` for backslash (0x5C)
///
/// # Examples
///
/// ```
/// use grep_cli::escape;
///
/// assert_eq!(escape(b"hello"), "hello");
/// assert_eq!(escape(b"hello\nworld"), r"hello\nworld");
/// assert_eq!(escape(b"\x00\x01\x02"), r"\0\x01\x02");
/// ```
pub fn escape(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for &b in bytes {
        match b {
            0x00 => escaped.push_str(r"\0"),
            0x09 => escaped.push_str(r"\t"),
            0x0A => escaped.push_str(r"\n"),
            0x0D => escaped.push_str(r"\r"),
            0x5C => escaped.push_str(r"\\"),
            b if b.is_ascii_graphic() || b == b' ' => {
                escaped.push(b as char);
            }
            b => {
                escaped.push_str(&format!(r"\x{:02X}", b));
            }
        }
    }
    escaped
}

/// Escape an OS string into a human-readable string.
///
/// This converts the OS string to bytes and then escapes non-printable
/// bytes. On Unix, this uses the raw bytes of the OS string. On other
/// platforms, this converts the OS string to UTF-8 (lossily) first.
///
/// # Examples
///
/// ```
/// use std::ffi::OsStr;
/// use grep_cli::escape_os;
///
/// assert_eq!(escape_os(OsStr::new("hello")), "hello");
/// ```
pub fn escape_os(s: &OsStr) -> String {
    escape(os_str_to_bytes(s).as_ref())
}

/// Parse a string with escape sequences into raw bytes.
///
/// The following escape sequences are recognized:
///
/// - `\\` — literal backslash
/// - `\n` — newline (0x0A)
/// - `\r` — carriage return (0x0D)
/// - `\t` — tab (0x09)
/// - `\0` — null byte (0x00)
/// - `\xNN` — arbitrary byte with hex value NN
///
/// All other characters are passed through as their UTF-8 encoding.
/// A backslash followed by an unrecognized character is treated as the
/// literal backslash followed by that character.
///
/// # Examples
///
/// ```
/// use grep_cli::unescape;
///
/// assert_eq!(unescape(r"hello\nworld"), b"hello\nworld");
/// assert_eq!(unescape(r"\x41\x42"), b"AB");
/// assert_eq!(unescape(r"\\"), b"\\");
/// assert_eq!(unescape(r"\0"), b"\0");
/// ```
pub fn unescape(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            None => {
                bytes.push(b'\\');
            }
            Some('\\') => bytes.push(b'\\'),
            Some('n') => bytes.push(b'\n'),
            Some('r') => bytes.push(b'\r'),
            Some('t') => bytes.push(b'\t'),
            Some('0') => bytes.push(0x00),
            Some('x') => {
                let hi = chars.next();
                let lo = chars.next();
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        let hex_str: String =
                            [h, l].iter().collect();
                        match u8::from_str_radix(&hex_str, 16) {
                            Ok(byte) => bytes.push(byte),
                            Err(_) => {
                                // Not valid hex, emit literally.
                                bytes.push(b'\\');
                                bytes.push(b'x');
                                let mut buf = [0u8; 4];
                                bytes.extend_from_slice(
                                    h.encode_utf8(&mut buf).as_bytes(),
                                );
                                bytes.extend_from_slice(
                                    l.encode_utf8(&mut buf).as_bytes(),
                                );
                            }
                        }
                    }
                    (Some(h), None) => {
                        // Only one character after \x, emit literally.
                        bytes.push(b'\\');
                        bytes.push(b'x');
                        let mut buf = [0u8; 4];
                        bytes.extend_from_slice(
                            h.encode_utf8(&mut buf).as_bytes(),
                        );
                    }
                    _ => {
                        // Nothing after \x, emit literally.
                        bytes.push(b'\\');
                        bytes.push(b'x');
                    }
                }
            }
            Some(other) => {
                bytes.push(b'\\');
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(
                    other.encode_utf8(&mut buf).as_bytes(),
                );
            }
        }
    }
    bytes
}

/// Convert an `OsStr` to bytes.
#[cfg(unix)]
fn os_str_to_bytes(s: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

/// Convert an `OsStr` to bytes (non-Unix fallback).
#[cfg(not(unix))]
fn os_str_to_bytes(s: &OsStr) -> Vec<u8> {
    s.to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_printable() {
        assert_eq!(escape(b"hello world"), "hello world");
    }

    #[test]
    fn test_escape_control_chars() {
        assert_eq!(escape(b"\0"), r"\0");
        assert_eq!(escape(b"\t"), r"\t");
        assert_eq!(escape(b"\n"), r"\n");
        assert_eq!(escape(b"\r"), r"\r");
        assert_eq!(escape(b"\\"), r"\\");
    }

    #[test]
    fn test_escape_non_printable() {
        assert_eq!(escape(b"\x01"), r"\x01");
        assert_eq!(escape(b"\xFF"), r"\xFF");
    }

    #[test]
    fn test_escape_mixed() {
        assert_eq!(escape(b"hello\nworld"), r"hello\nworld");
    }

    #[test]
    fn test_unescape_basic() {
        assert_eq!(unescape("hello"), b"hello");
        assert_eq!(unescape(r"hello\nworld"), b"hello\nworld");
        assert_eq!(unescape(r"\\"), b"\\");
        assert_eq!(unescape(r"\t"), b"\t");
        assert_eq!(unescape(r"\r"), b"\r");
        assert_eq!(unescape(r"\0"), b"\0");
    }

    #[test]
    fn test_unescape_hex() {
        assert_eq!(unescape(r"\x41"), b"A");
        assert_eq!(unescape(r"\x41\x42\x43"), b"ABC");
        assert_eq!(unescape(r"\xFF"), &[0xFF]);
        assert_eq!(unescape(r"\x00"), &[0x00]);
    }

    #[test]
    fn test_unescape_trailing_backslash() {
        assert_eq!(unescape(r"\"), b"\\");
    }

    #[test]
    fn test_unescape_unknown_escape() {
        assert_eq!(unescape(r"\q"), b"\\q");
    }

    #[test]
    fn test_escape_os_basic() {
        assert_eq!(escape_os(OsStr::new("hello")), "hello");
    }

    #[test]
    fn test_roundtrip() {
        let original = b"hello\nworld\t\x00\xFF";
        let escaped = escape(original);
        let unescaped = unescape(&escaped);
        assert_eq!(unescaped, original);
    }
}
