/*!
CLI utilities for grep-like tools.

This crate provides a collection of small utilities that are commonly needed
when building command-line search tools similar to `ripgrep`. These include:

- **Terminal detection**: Determine whether stdout/stderr is connected to a
  terminal (TTY).
- **Standard stream constructors**: Convenience wrappers around `termcolor`'s
  color-aware output streams with various buffering strategies.
- **Hostname detection**: Read the machine's hostname from the OS.
- **Human-readable size parsing**: Parse strings like `"10M"` or `"1KB"` into
  byte counts.
- **Escape/unescape helpers**: Convert between raw bytes and human-readable
  escape sequences (`\n`, `\t`, `\xHH`).
- **Pattern file reading**: Read search patterns from files or stdin.
- **Decompression**: Transparently decompress files based on their extension
  by spawning external tools (gzip, bzip2, xz, etc.).
*/

use std::io::{self, BufRead, IsTerminal, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

// Re-export key termcolor types for convenience.
pub use termcolor::{
    BufferWriter, BufferedStandardStream, Color, ColorChoice, ColorSpec,
    StandardStream, WriteColor,
};

// ---------------------------------------------------------------------------
// 1. Terminal Detection
// ---------------------------------------------------------------------------

/// Returns `true` if stdout is connected to a terminal (TTY).
///
/// This is useful for deciding whether to enable colored output, interactive
/// progress indicators, or other terminal-specific features.
///
/// # Example
///
/// ```no_run
/// if grep_cli::is_tty_stdout() {
///     println!("We are in a terminal — colors are welcome!");
/// }
/// ```
pub fn is_tty_stdout() -> bool {
    io::stdout().is_terminal()
}

/// Returns `true` if stderr is connected to a terminal (TTY).
///
/// # Example
///
/// ```no_run
/// if grep_cli::is_tty_stderr() {
///     eprintln!("stderr is a terminal");
/// }
/// ```
pub fn is_tty_stderr() -> bool {
    io::stderr().is_terminal()
}

// ---------------------------------------------------------------------------
// 2. Standard Stream Constructors
// ---------------------------------------------------------------------------

/// Create a `termcolor::StandardStream` for stdout with the given color
/// choice.
///
/// This is a thin wrapper around [`termcolor::StandardStream::stdout`] that
/// provides a shorter import path when using this crate.
///
/// # Example
///
/// ```no_run
/// use grep_cli::ColorChoice;
/// let mut out = grep_cli::stdout(ColorChoice::Auto);
/// ```
pub fn stdout(color_choice: ColorChoice) -> StandardStream {
    StandardStream::stdout(color_choice)
}

/// Create a line-buffered `termcolor::BufferedStandardStream` for stdout.
///
/// Line-buffered output is appropriate when writing to a terminal, because
/// each line is flushed automatically, giving the user immediate feedback.
///
/// # Example
///
/// ```no_run
/// use grep_cli::ColorChoice;
/// let mut out = grep_cli::stdout_buffered_line(ColorChoice::Auto);
/// ```
pub fn stdout_buffered_line(color_choice: ColorChoice) -> BufferedStandardStream {
    BufferedStandardStream::stdout(color_choice)
}

/// Create a block-buffered `termcolor::BufferWriter` for stdout.
///
/// Block buffering is appropriate when output is piped to another process or
/// a file, since it reduces the number of write syscalls. The `BufferWriter`
/// uses an internal lock-free buffer that is flushed in large chunks.
///
/// # Example
///
/// ```no_run
/// use grep_cli::ColorChoice;
/// let writer = grep_cli::stdout_buffered_block(ColorChoice::Never);
/// ```
pub fn stdout_buffered_block(color_choice: ColorChoice) -> BufferWriter {
    BufferWriter::stdout(color_choice)
}

// ---------------------------------------------------------------------------
// 3. Hostname Detection
// ---------------------------------------------------------------------------

/// Attempt to determine the hostname of the current machine.
///
/// On Linux this first tries to read `/etc/hostname`. If that fails (e.g. the
/// file does not exist, or the contents are empty), it falls back to reading
/// `/proc/sys/kernel/hostname`.
///
/// Returns `None` if no hostname could be determined.
///
/// # Example
///
/// ```no_run
/// if let Some(host) = grep_cli::hostname() {
///     println!("Running on: {host}");
/// }
/// ```
pub fn hostname() -> Option<String> {
    // Try /etc/hostname first (most common on Linux).
    if let Ok(contents) = std::fs::read_to_string("/etc/hostname") {
        let name = contents.trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    // Fallback: /proc/sys/kernel/hostname (Linux-specific, no libc needed).
    if let Ok(contents) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let name = contents.trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 4. Human-Readable Size Parsing
// ---------------------------------------------------------------------------

/// Parse a human-readable size string into a byte count.
///
/// Recognised suffixes (case-insensitive):
///
/// | Suffix     | Multiplier |
/// |------------|------------|
/// | *(none)*   | 1          |
/// | `K` / `KB` | 1 024      |
/// | `M` / `MB` | 1 048 576  |
/// | `G` / `GB` | 1 073 741 824 |
///
/// The numeric part must be a non-negative integer. Whitespace between the
/// number and the suffix is **not** allowed.
///
/// # Errors
///
/// Returns an `Err` with a human-readable message if:
/// - The input is empty.
/// - The numeric part cannot be parsed as `u64`.
/// - The suffix is not recognised.
/// - The multiplication would overflow `u64`.
///
/// # Examples
///
/// ```
/// assert_eq!(grep_cli::parse_human_readable_size("1K").unwrap(), 1024);
/// assert_eq!(grep_cli::parse_human_readable_size("10M").unwrap(), 10 * 1024 * 1024);
/// assert_eq!(grep_cli::parse_human_readable_size("500").unwrap(), 500);
/// assert_eq!(grep_cli::parse_human_readable_size("2GB").unwrap(), 2 * 1024 * 1024 * 1024);
/// ```
pub fn parse_human_readable_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size string".to_string());
    }

    // Find the boundary between numeric and suffix parts.
    let num_end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());
    if num_end == 0 {
        return Err(format!("no numeric component in size string: {s:?}"));
    }

    let num_str = &s[..num_end];
    let suffix = s[num_end..].to_ascii_uppercase();

    let base: u64 = num_str
        .parse()
        .map_err(|e| format!("invalid number in size string {s:?}: {e}"))?;

    let multiplier: u64 = match suffix.as_str() {
        "" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        other => {
            return Err(format!("unrecognised size suffix: {other:?}"));
        }
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflows u64: {s:?}"))
}

// ---------------------------------------------------------------------------
// 5. Escape / Unescape Helpers
// ---------------------------------------------------------------------------

/// Unescape a string, converting escape sequences into their raw byte values.
///
/// Supported sequences:
///
/// | Sequence   | Byte value          |
/// |------------|---------------------|
/// | `\\`       | `0x5C` (backslash)  |
/// | `\n`       | `0x0A` (newline)    |
/// | `\r`       | `0x0D` (carriage return) |
/// | `\t`       | `0x09` (tab)       |
/// | `\0`       | `0x00` (null)      |
/// | `\a`       | `0x07` (bell)      |
/// | `\x{HH}`  | arbitrary hex byte  |
///
/// Any other character following a backslash is passed through literally
/// (both the backslash and the character are emitted).
///
/// # Examples
///
/// ```
/// assert_eq!(grep_cli::unescape(r"hello\nworld"), b"hello\nworld");
/// assert_eq!(grep_cli::unescape(r"\x41\x42"), b"AB");
/// assert_eq!(grep_cli::unescape(r"\\slash"), b"\\slash");
/// ```
pub fn unescape(s: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c != '\\' {
            // Fast path: push UTF-8 bytes of the literal character.
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            result.extend_from_slice(encoded.as_bytes());
            continue;
        }
        // We saw a backslash — look at the next character.
        match chars.next() {
            None => {
                // Trailing backslash — emit it literally.
                result.push(b'\\');
            }
            Some('\\') => result.push(b'\\'),
            Some('n') => result.push(b'\n'),
            Some('r') => result.push(b'\r'),
            Some('t') => result.push(b'\t'),
            Some('0') => result.push(0),
            Some('a') => result.push(0x07),
            Some('x') => {
                // Expect exactly two hex digits (with or without braces).
                let hex = parse_hex_escape(&mut chars);
                match hex {
                    Some(byte) => result.push(byte),
                    None => {
                        // Invalid hex escape — emit the literal `\x`.
                        result.push(b'\\');
                        result.push(b'x');
                    }
                }
            }
            Some(other) => {
                // Unknown escape — pass through literally.
                result.push(b'\\');
                let mut buf = [0u8; 4];
                let encoded = other.encode_utf8(&mut buf);
                result.extend_from_slice(encoded.as_bytes());
            }
        }
    }
    result
}

/// Parse a hex escape after `\x`. Supports both `\xHH` (bare) and `\x{HH}`
/// (braced) forms.
fn parse_hex_escape(chars: &mut std::str::Chars<'_>) -> Option<u8> {
    // Peek at whether we have a brace.
    let mut peekable = chars.clone();
    let first = peekable.next()?;

    if first == '{' {
        // Braced form: \x{HH}
        // Advance the real iterator past '{'.
        chars.next();
        let h1 = chars.next()?;
        let h2 = chars.next()?;
        let close = chars.next()?;
        if close != '}' {
            return None;
        }
        let high = hex_digit(h1)?;
        let low = hex_digit(h2)?;
        Some(high << 4 | low)
    } else {
        // Bare form: \xHH
        let h1 = chars.next()?;
        let h2 = chars.next()?;
        let high = hex_digit(h1)?;
        let low = hex_digit(h2)?;
        Some(high << 4 | low)
    }
}

/// Convert a single hex character to its numeric value (0–15).
fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

/// Escape a byte slice for display.
///
/// Printable ASCII bytes (0x20–0x7E) are emitted as-is. All other bytes are
/// emitted in `\xHH` notation.
///
/// # Examples
///
/// ```
/// assert_eq!(grep_cli::escape(b"hello\nworld"), r"hello\x0aworld");
/// assert_eq!(grep_cli::escape(b"\x00\xff"), r"\x00\xff");
/// assert_eq!(grep_cli::escape(b"plain"), "plain");
/// ```
pub fn escape(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'\\' {
            result.push_str(r"\\");
        } else if b.is_ascii_graphic() || b == b' ' {
            result.push(b as char);
        } else {
            result.push_str(&format!("\\x{b:02x}"));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 6. Pattern File Reading
// ---------------------------------------------------------------------------

/// Read search patterns from a file, one per line.
///
/// Every line in the file is treated as a pattern. Lines starting with `#`
/// are **not** treated as comments — they are valid patterns. Empty lines
/// produce empty patterns (which typically match everything in regex
/// engines).
///
/// Line endings (`\n` and `\r\n`) are stripped.
///
/// # Errors
///
/// Returns an `io::Error` if the file cannot be opened or read.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// let patterns = grep_cli::read_patterns_from_file(Path::new("patterns.txt"))
///     .expect("failed to read patterns");
/// for pat in &patterns {
///     println!("pattern: {pat:?}");
/// }
/// ```
pub fn read_patterns_from_file(path: &Path) -> Result<Vec<String>, io::Error> {
    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    read_patterns_from_reader(reader)
}

/// Read search patterns from standard input, one per line.
///
/// Behaves identically to [`read_patterns_from_file`] but reads from stdin.
///
/// # Errors
///
/// Returns an `io::Error` if stdin cannot be read.
///
/// # Example
///
/// ```no_run
/// let patterns = grep_cli::read_patterns_from_stdin()
///     .expect("failed to read patterns from stdin");
/// ```
pub fn read_patterns_from_stdin() -> Result<Vec<String>, io::Error> {
    let stdin = io::stdin();
    let reader = stdin.lock();
    read_patterns_from_reader(reader)
}

/// Shared implementation: read patterns from any `BufRead`.
fn read_patterns_from_reader<R: BufRead>(reader: R) -> Result<Vec<String>, io::Error> {
    let mut patterns = Vec::new();
    for line_result in reader.lines() {
        let line = line_result?;
        patterns.push(line);
    }
    Ok(patterns)
}

// ---------------------------------------------------------------------------
// 7. Decompression Reader
// ---------------------------------------------------------------------------

/// A reader that transparently decompresses data by spawning an external
/// decompression tool and reading from its stdout.
///
/// The following file extensions are mapped to decompression commands:
///
/// | Extension | Command                  |
/// |-----------|--------------------------|
/// | `.gz`     | `gzip -d -c`             |
/// | `.bz2`    | `bzip2 -d -c`            |
/// | `.xz`     | `xz -d -c`               |
/// | `.lz4`    | `lz4 -d -c`              |
/// | `.lzma`   | `xz --format=lzma -d -c` |
/// | `.zst`    | `zstd -d -c`             |
/// | `.Z`      | `uncompress -c`           |
/// | `.br`     | `brotli -d -c`            |
///
/// When the `DecompressionReader` is dropped, the child process is killed (if
/// still running) to avoid leaked processes.
pub struct DecompressionReader {
    child: Child,
    stdout: ChildStdout,
}

impl DecompressionReader {
    /// Returns a mutable reference to the underlying [`Child`] process.
    ///
    /// This can be used to check the exit status after reading completes.
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Read for DecompressionReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl Drop for DecompressionReader {
    fn drop(&mut self) {
        // Attempt to kill the child if it is still running. We intentionally
        // ignore errors here because the child may have already exited.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Create a [`DecompressionReader`] for the file at `path`, based on its
/// extension.
///
/// If the file extension is recognised as a compressed format, the
/// appropriate decompression command is spawned and a reader wrapping its
/// stdout is returned. If the extension is not recognised, `Ok(None)` is
/// returned.
///
/// # Errors
///
/// Returns an `io::Error` if the decompression command cannot be spawned
/// (e.g. the tool is not installed).
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use std::io::Read;
///
/// let path = Path::new("data.gz");
/// if let Some(mut reader) = grep_cli::decompress_reader(path).unwrap() {
///     let mut contents = Vec::new();
///     reader.read_to_end(&mut contents).unwrap();
/// }
/// ```
pub fn decompress_reader(path: &Path) -> Result<Option<DecompressionReader>, io::Error> {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.to_ascii_lowercase(),
        None => return Ok(None),
    };

    let (program, args) = match ext.as_str() {
        "gz" => ("gzip", vec!["-d", "-c"]),
        "bz2" => ("bzip2", vec!["-d", "-c"]),
        "xz" => ("xz", vec!["-d", "-c"]),
        "lz4" => ("lz4", vec!["-d", "-c"]),
        "lzma" => ("xz", vec!["--format=lzma", "-d", "-c"]),
        "zst" | "zstd" => ("zstd", vec!["-d", "-c"]),
        "z" => ("uncompress", vec!["-c"]),
        "br" => ("brotli", vec!["-d", "-c"]),
        _ => return Ok(None),
    };

    log::debug!(
        "spawning decompression command: {program} {} {}",
        args.join(" "),
        path.display(),
    );

    let mut child = Command::new(program)
        .args(&args)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "failed to spawn decompression command '{program}' \
                     for '{}': {e}",
                    path.display()
                ),
            )
        })?;

    let stdout = child
        .stdout
        .take()
        .expect("child stdout was piped but is None — this is a bug");

    Ok(Some(DecompressionReader { child, stdout }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_human_readable_size --

    #[test]
    fn test_parse_plain_number() {
        assert_eq!(parse_human_readable_size("0").unwrap(), 0);
        assert_eq!(parse_human_readable_size("500").unwrap(), 500);
        assert_eq!(parse_human_readable_size("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_kilobytes() {
        assert_eq!(parse_human_readable_size("1K").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1k").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1KB").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1kb").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("2K").unwrap(), 2048);
    }

    #[test]
    fn test_parse_megabytes() {
        assert_eq!(parse_human_readable_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_human_readable_size("10M").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_human_readable_size("1MB").unwrap(), 1024 * 1024);
        assert_eq!(parse_human_readable_size("1mb").unwrap(), 1024 * 1024);
    }

    #[test]
    fn test_parse_gigabytes() {
        assert_eq!(
            parse_human_readable_size("1G").unwrap(),
            1024 * 1024 * 1024
        );
        assert_eq!(
            parse_human_readable_size("2GB").unwrap(),
            2 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse_human_readable_size("").is_err());
        assert!(parse_human_readable_size("K").is_err());
        assert!(parse_human_readable_size("abc").is_err());
        assert!(parse_human_readable_size("1T").is_err());
        assert!(parse_human_readable_size("1TB").is_err());
    }

    // -- unescape --

    #[test]
    fn test_unescape_plain() {
        assert_eq!(unescape("hello"), b"hello");
    }

    #[test]
    fn test_unescape_newline() {
        assert_eq!(unescape(r"hello\nworld"), b"hello\nworld");
    }

    #[test]
    fn test_unescape_tab() {
        assert_eq!(unescape(r"a\tb"), b"a\tb");
    }

    #[test]
    fn test_unescape_carriage_return() {
        assert_eq!(unescape(r"a\rb"), b"a\rb");
    }

    #[test]
    fn test_unescape_null() {
        assert_eq!(unescape(r"a\0b"), b"a\0b");
    }

    #[test]
    fn test_unescape_bell() {
        assert_eq!(unescape(r"\a"), &[0x07]);
    }

    #[test]
    fn test_unescape_backslash() {
        assert_eq!(unescape(r"\\"), b"\\");
    }

    #[test]
    fn test_unescape_hex_bare() {
        assert_eq!(unescape(r"\x41\x42"), b"AB");
        assert_eq!(unescape(r"\x00"), &[0x00]);
        assert_eq!(unescape(r"\xff"), &[0xff]);
    }

    #[test]
    fn test_unescape_hex_braced() {
        assert_eq!(unescape(r"\x{41}"), b"A");
        assert_eq!(unescape(r"\x{0a}"), &[0x0a]);
    }

    #[test]
    fn test_unescape_trailing_backslash() {
        assert_eq!(unescape(r"end\"), b"end\\");
    }

    #[test]
    fn test_unescape_unknown_escape() {
        // Unknown escape like \q is passed through as \q.
        assert_eq!(unescape(r"\q"), b"\\q");
    }

    // -- escape --

    #[test]
    fn test_escape_plain() {
        assert_eq!(escape(b"hello"), "hello");
    }

    #[test]
    fn test_escape_non_printable() {
        assert_eq!(escape(b"\x00\xff"), r"\x00\xff");
    }

    #[test]
    fn test_escape_newline() {
        assert_eq!(escape(b"a\nb"), r"a\x0ab");
    }

    #[test]
    fn test_escape_backslash() {
        assert_eq!(escape(b"a\\b"), r"a\\b");
    }

    #[test]
    fn test_escape_space() {
        assert_eq!(escape(b"a b"), "a b");
    }

    // -- pattern reading --

    #[test]
    fn test_read_patterns_from_reader() {
        let input = b"foo\nbar\n#not-a-comment\n\nbaz\n";
        let cursor = io::BufReader::new(&input[..]);
        let patterns = read_patterns_from_reader(cursor).unwrap();
        assert_eq!(
            patterns,
            vec!["foo", "bar", "#not-a-comment", "", "baz"]
        );
    }

    #[test]
    fn test_read_patterns_empty_input() {
        let input = b"";
        let cursor = io::BufReader::new(&input[..]);
        let patterns = read_patterns_from_reader(cursor).unwrap();
        assert!(patterns.is_empty());
    }

    // -- DecompressionReader extension mapping --

    #[test]
    fn test_decompress_unrecognised_extension() {
        let path = Path::new("file.txt");
        let result = decompress_reader(path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_decompress_no_extension() {
        let path = Path::new("file");
        let result = decompress_reader(path).unwrap();
        assert!(result.is_none());
    }
}
