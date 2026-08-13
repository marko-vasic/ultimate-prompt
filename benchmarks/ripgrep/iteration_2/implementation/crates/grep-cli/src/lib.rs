/*!
CLI utility functions for ripgrep.

This crate provides a variety of helper routines useful for building
command line tools, with a special focus on the needs of search tools
like ripgrep.
*/

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, BufRead, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use bstr::ByteVec;
pub use termcolor::ColorChoice;
use termcolor::{Color, ColorSpec, StandardStream};

// -----------------------------------------------------------------------
// Terminal detection
// -----------------------------------------------------------------------

/// Returns `true` if and only if stdout is connected to a terminal.
pub fn is_tty_stdout() -> bool {
    std::io::stdout().is_terminal()
}

/// Returns `true` if and only if stderr is connected to a terminal.
pub fn is_tty_stderr() -> bool {
    std::io::stderr().is_terminal()
}

/// Returns `true` if and only if stdin is connected to a terminal.
pub fn is_tty_stdin() -> bool {
    std::io::stdin().is_terminal()
}

/// Returns `true` if and only if stdin is readable (e.g. a pipe or a regular file redirected to stdin).
pub fn is_readable_stdin() -> bool {
    #[cfg(unix)]
    {
        unsafe {
            let mut stat = std::mem::zeroed();
            if libc::fstat(0, &mut stat) == 0 {
                let mode = stat.st_mode & libc::S_IFMT;
                return mode == libc::S_IFIFO || mode == libc::S_IFREG;
            }
        }
        false
    }
    #[cfg(not(unix))]
    {
        !std::io::stdin().is_terminal()
    }
}

use std::io::IsTerminal;

// -----------------------------------------------------------------------
// Output buffering
// -----------------------------------------------------------------------

/// Create a `StandardStream` for stdout with the given color choice.
pub fn stdout(color_choice: ColorChoice) -> StandardStream {
    StandardStream::stdout(color_choice)
}

/// Create a line-buffered `BufferedStandardStream` for stdout.
pub fn stdout_buffered_line(
    color_choice: ColorChoice,
) -> termcolor::BufferedStandardStream {
    termcolor::BufferedStandardStream::stdout(color_choice)
}

/// Create a block-buffered `BufferedStandardStream` for stdout.
///
/// Block buffering is typically used when stdout is not connected to a
/// terminal, where line-by-line flushing is not necessary.
pub fn stdout_buffered_block(
    color_choice: ColorChoice,
) -> termcolor::BufferedStandardStream {
    termcolor::BufferedStandardStream::stdout(color_choice)
}

// -----------------------------------------------------------------------
// CommandError
// -----------------------------------------------------------------------

/// An error that occurs when a subprocess command fails.
#[derive(Debug)]
pub enum CommandError {
    /// An I/O error occurred while spawning or interacting with the process.
    Io(io::Error),
    /// No decompression command is available for the given file extension.
    UnknownExtension(String),
    /// The process exited with a non-zero status or was killed by a signal.
    Failed(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Io(err) => write!(f, "I/O error: {}", err),
            CommandError::UnknownExtension(ext) => {
                write!(f, "unknown file extension: '{}'", ext)
            }
            CommandError::Failed(msg) => write!(f, "command failed: {}", msg),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for CommandError {
    fn from(err: io::Error) -> CommandError {
        CommandError::Io(err)
    }
}

// -----------------------------------------------------------------------
// DecompressionReader
// -----------------------------------------------------------------------

/// A reader that decompresses a file by running an external decompression
/// command and reading from its stdout.
pub struct DecompressionReader {
    child: Child,
}

/// A builder for constructing a `DecompressionReader`.
///
/// The builder selects the appropriate decompression command based on the
/// file extension.
pub struct DecompressionReaderBuilder {
    // Currently no configurable options, but the builder pattern allows
    // future extension.
    _priv: (),
}

impl DecompressionReaderBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> DecompressionReaderBuilder {
        DecompressionReaderBuilder { _priv: () }
    }

    /// Build a `DecompressionReader` for the given file path.
    ///
    /// The decompression command is selected based on the file extension.
    /// Returns an error if the extension is not recognized or if the
    /// command fails to spawn.
    pub fn build(
        &self,
        path: &Path,
    ) -> Result<DecompressionReader, CommandError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let (cmd, args) = decompress_command(ext)?;

        let child = Command::new(cmd)
            .args(args)
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(DecompressionReader { child })
    }
}

impl Default for DecompressionReaderBuilder {
    fn default() -> DecompressionReaderBuilder {
        DecompressionReaderBuilder::new()
    }
}

/// Returns the decompression command and arguments for the given extension.
fn decompress_command(
    ext: &str,
) -> Result<(&'static str, &'static [&'static str]), CommandError> {
    match ext {
        "gz" => Ok(("gzip", &["-d", "-c"])),
        "bz2" => Ok(("bzip2", &["-d", "-c"])),
        "xz" => Ok(("xz", &["-d", "-c"])),
        "lz4" => Ok(("lz4", &["-d", "-c"])),
        "lzma" => Ok(("xz", &["--format=lzma", "-d", "-c"])),
        "zst" => Ok(("zstd", &["-d", "-c", "-q"])),
        "Z" => Ok(("uncompress", &["-c"])),
        _ => Err(CommandError::UnknownExtension(ext.to_string())),
    }
}

impl Read for DecompressionReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.child
            .stdout
            .as_mut()
            .expect("child process stdout was not captured")
            .read(buf)
    }
}

impl Drop for DecompressionReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// -----------------------------------------------------------------------
// PreprocessorReader
// -----------------------------------------------------------------------

/// A reader that runs a preprocessor command on a file and reads from
/// the command's stdout.
pub struct PreprocessorReader {
    child: Child,
}

impl PreprocessorReader {
    /// Create a new preprocessor reader that runs the given command on
    /// the given file path. The `command` string is the name of the
    /// program to run. The file path is passed as the first argument
    /// to the command.
    pub fn new(
        command: &str,
        path: &Path,
    ) -> Result<PreprocessorReader, CommandError> {
        let child = Command::new(command)
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(PreprocessorReader { child })
    }
}

impl Read for PreprocessorReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.child
            .stdout
            .as_mut()
            .expect("child process stdout was not captured")
            .read(buf)
    }
}

impl Drop for PreprocessorReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// -----------------------------------------------------------------------
// Human-readable size parsing
// -----------------------------------------------------------------------

/// Parse a human-readable size string into a byte count.
///
/// The following suffixes are supported (case-insensitive):
///
/// * `K` or `KB` — multiply by 1024
/// * `M` or `MB` — multiply by 1024²
/// * `G` or `GB` — multiply by 1024³
/// * `T` or `TB` — multiply by 1024⁴
///
/// A plain number without a suffix is interpreted as bytes.
///
/// # Examples
///
/// ```
/// use grep_cli::parse_human_readable_size;
///
/// assert_eq!(parse_human_readable_size("1024").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("1K").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("1KB").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("10M").unwrap(), 10 * 1024 * 1024);
/// assert_eq!(parse_human_readable_size("500GB").unwrap(), 500 * 1024 * 1024 * 1024);
/// ```
pub fn parse_human_readable_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty string is not a valid size".to_string());
    }

    // Find the boundary between digits and suffix.
    let num_end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());

    if num_end == 0 {
        return Err(format!(
            "invalid size '{}': no numeric component",
            s
        ));
    }

    let num_str = &s[..num_end];
    let suffix = s[num_end..].trim();

    let base: u64 = num_str.parse().map_err(|e| {
        format!("invalid size '{}': {}", s, e)
    })?;

    let multiplier = match suffix.to_uppercase().as_str() {
        "" => 1u64,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        "T" | "TB" => 1024 * 1024 * 1024 * 1024,
        _ => {
            return Err(format!(
                "invalid size suffix '{}' in '{}'",
                suffix, s
            ));
        }
    };

    base.checked_mul(multiplier).ok_or_else(|| {
        format!("size '{}' overflows u64", s)
    })
}

// -----------------------------------------------------------------------
// Pattern file reading
// -----------------------------------------------------------------------

/// Read patterns from a file, one per line.
///
/// Empty lines are skipped.
pub fn patterns_from_path(
    path: &Path,
) -> Result<Vec<String>, io::Error> {
    let file = std::fs::File::open(path)?;
    patterns_from_reader(file)
}

/// Read patterns from any reader, one per line.
///
/// Empty lines are skipped.
pub fn patterns_from_reader<R: Read>(
    reader: R,
) -> Result<Vec<String>, io::Error> {
    let buf = io::BufReader::new(reader);
    let mut patterns = Vec::new();
    for line in buf.lines() {
        let line = line?;
        if !line.is_empty() {
            patterns.push(line);
        }
    }
    Ok(patterns)
}

/// Read patterns from stdin, one per line.
///
/// Empty lines are skipped.
pub fn patterns_from_stdin() -> Result<Vec<String>, io::Error> {
    patterns_from_reader(io::stdin().lock())
}

// -----------------------------------------------------------------------
// Hostname detection
// -----------------------------------------------------------------------

/// Returns the hostname of the current system, or `None` if it cannot
/// be determined.
#[cfg(unix)]
pub fn hostname() -> Option<String> {
    let mut buf = vec![0u8; 256];
    let ret = unsafe {
        libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len())
    };
    if ret != 0 {
        return None;
    }
    // Find the NUL terminator.
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec()).ok()
}

#[cfg(not(unix))]
pub fn hostname() -> Option<String> {
    None
}

// -----------------------------------------------------------------------
// Escape utilities
// -----------------------------------------------------------------------

/// Escape non-printable bytes in the given byte slice.
///
/// Printable ASCII bytes are passed through unchanged. All other bytes
/// are escaped to `\xNN` format.
pub fn escape(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for &b in bytes {
        if b == b'\\' {
            escaped.push_str("\\\\");
        } else if b >= 0x20 && b <= 0x7E {
            escaped.push(b as char);
        } else {
            escaped.push_str(&format!("\\x{:02X}", b));
        }
    }
    escaped
}

/// Escape non-printable bytes in the given `OsStr`.
///
/// On Unix, the raw bytes of the `OsStr` are used. On other platforms,
/// the lossy UTF-8 representation is used.
pub fn escape_os(s: &OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        escape(s.as_bytes())
    }
    #[cfg(not(unix))]
    {
        escape(s.to_string_lossy().as_bytes())
    }
}

/// Unescape a string containing escape sequences.
///
/// The following escape sequences are recognized:
///
/// * `\\n` → newline (0x0A)
/// * `\\t` → tab (0x09)
/// * `\\r` → carriage return (0x0D)
/// * `\\\\` → backslash (0x5C)
/// * `\\0` → NUL (0x00)
/// * `\\xNN` → the byte with hex value NN
///
/// All other characters are passed through as their UTF-8 encoding.
pub fn unescape(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            bytes.extend_from_slice(encoded.as_bytes());
            continue;
        }
        match chars.next() {
            None => {
                bytes.push(b'\\');
            }
            Some('n') => bytes.push(b'\n'),
            Some('t') => bytes.push(b'\t'),
            Some('r') => bytes.push(b'\r'),
            Some('\\') => bytes.push(b'\\'),
            Some('0') => bytes.push(0),
            Some('x') => {
                let mut hex = String::new();
                if let Some(h1) = chars.next() {
                    hex.push(h1);
                }
                if let Some(h2) = chars.next() {
                    hex.push(h2);
                }
                match u8::from_str_radix(&hex, 16) {
                    Ok(b) => bytes.push(b),
                    Err(_) => {
                        bytes.extend_from_slice(b"\\x");
                        bytes.extend_from_slice(hex.as_bytes());
                    }
                }
            }
            Some(c) => {
                bytes.push(b'\\');
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                bytes.extend_from_slice(encoded.as_bytes());
            }
        }
    }
    bytes
}

// -----------------------------------------------------------------------
// Color configuration
// -----------------------------------------------------------------------

/// The type of output for which a color specification applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutType {
    /// The search match itself.
    Match,
    /// The file path.
    Path,
    /// The line number.
    Line,
    /// The column number.
    Column,
}

impl OutType {
    fn from_str(s: &str) -> Result<OutType, String> {
        match s.to_lowercase().as_str() {
            "match" => Ok(OutType::Match),
            "path" => Ok(OutType::Path),
            "line" => Ok(OutType::Line),
            "column" => Ok(OutType::Column),
            _ => Err(format!("unknown output type: '{}'", s)),
        }
    }
}

/// Parsed color specifications.
///
/// This contains color specs for each output type (match, path, line,
/// column) and provides default colors that match ripgrep's defaults.
#[derive(Clone, Debug)]
pub struct ColorSpecs {
    match_spec: ColorSpec,
    path_spec: ColorSpec,
    line_spec: ColorSpec,
    column_spec: ColorSpec,
}

impl Default for ColorSpecs {
    fn default() -> ColorSpecs {
        let mut match_spec = ColorSpec::new();
        match_spec.set_fg(Some(Color::Red)).set_bold(true);

        let mut path_spec = ColorSpec::new();
        path_spec.set_fg(Some(Color::Magenta));

        let mut line_spec = ColorSpec::new();
        line_spec.set_fg(Some(Color::Green));

        let mut column_spec = ColorSpec::new();
        column_spec.set_fg(Some(Color::Green));

        ColorSpecs {
            match_spec,
            path_spec,
            line_spec,
            column_spec,
        }
    }
}

impl ColorSpecs {
    /// Get the `ColorSpec` for the given output type.
    pub fn get(&self, out_type: OutType) -> ColorSpec {
        match out_type {
            OutType::Match => self.match_spec.clone(),
            OutType::Path => self.path_spec.clone(),
            OutType::Line => self.line_spec.clone(),
            OutType::Column => self.column_spec.clone(),
        }
    }
}

/// Parse a list of color specification strings.
///
/// Each specification has the format `{type}:{attribute}:{value}` where:
///
/// * `type` is one of: `match`, `path`, `line`, `column`
/// * `attribute` is one of: `fg`, `bg`, `style`, `none`
/// * `value` depends on the attribute:
///   - For `fg`/`bg`: a named color (red, green, blue, cyan, magenta,
///     yellow, white, black) or `{r},{g},{b}` for an RGB color.
///   - For `style`: bold, italic, underline, nobold, noitalic, nounderline
///   - For `none`: no value (resets the color spec)
///
/// The specs are applied on top of the default color settings.
pub fn parse_color_specs(
    specs: &[String],
) -> Result<ColorSpecs, String> {
    let mut color_specs = ColorSpecs::default();

    for spec_str in specs {
        let parts: Vec<&str> = spec_str.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(format!(
                "invalid color spec '{}': expected format 'type:attribute:value'",
                spec_str
            ));
        }

        let out_type = OutType::from_str(parts[0])?;
        let attribute = parts[1].to_lowercase();

        let target = match out_type {
            OutType::Match => &mut color_specs.match_spec,
            OutType::Path => &mut color_specs.path_spec,
            OutType::Line => &mut color_specs.line_spec,
            OutType::Column => &mut color_specs.column_spec,
        };

        match attribute.as_str() {
            "none" => {
                *target = ColorSpec::new();
            }
            "fg" => {
                let value = parts.get(2).ok_or_else(|| {
                    format!("missing value for 'fg' in '{}'", spec_str)
                })?;
                let color = parse_color(value)?;
                target.set_fg(Some(color));
            }
            "bg" => {
                let value = parts.get(2).ok_or_else(|| {
                    format!("missing value for 'bg' in '{}'", spec_str)
                })?;
                let color = parse_color(value)?;
                target.set_bg(Some(color));
            }
            "style" => {
                let value = parts.get(2).ok_or_else(|| {
                    format!("missing value for 'style' in '{}'", spec_str)
                })?;
                apply_style(target, value)?;
            }
            _ => {
                return Err(format!(
                    "unknown attribute '{}' in '{}'",
                    attribute, spec_str
                ));
            }
        }
    }

    Ok(color_specs)
}

/// Parse a color name or RGB triple.
fn parse_color(s: &str) -> Result<Color, String> {
    let lower = s.to_lowercase();
    match lower.as_str() {
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "blue" => Ok(Color::Blue),
        "cyan" => Ok(Color::Cyan),
        "magenta" => Ok(Color::Magenta),
        "yellow" => Ok(Color::Yellow),
        "white" => Ok(Color::White),
        "black" => Ok(Color::Black),
        _ => {
            // Try parsing as r,g,b
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() == 3 {
                let r: u8 = parts[0].trim().parse().map_err(|_| {
                    format!("invalid red component '{}' in color '{}'", parts[0], s)
                })?;
                let g: u8 = parts[1].trim().parse().map_err(|_| {
                    format!(
                        "invalid green component '{}' in color '{}'",
                        parts[1], s
                    )
                })?;
                let b: u8 = parts[2].trim().parse().map_err(|_| {
                    format!(
                        "invalid blue component '{}' in color '{}'",
                        parts[2], s
                    )
                })?;
                Ok(Color::Rgb(r, g, b))
            } else {
                Err(format!("unknown color '{}'", s))
            }
        }
    }
}

/// Apply a style attribute to a `ColorSpec`.
fn apply_style(spec: &mut ColorSpec, style: &str) -> Result<(), String> {
    match style.to_lowercase().as_str() {
        "bold" => {
            spec.set_bold(true);
        }
        "nobold" => {
            spec.set_bold(false);
        }
        "italic" => {
            spec.set_italic(true);
        }
        "noitalic" => {
            spec.set_italic(false);
        }
        "underline" => {
            spec.set_underline(true);
        }
        "nounderline" => {
            spec.set_underline(false);
        }
        _ => {
            return Err(format!("unknown style '{}'", style));
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_human_readable_size_plain() {
        assert_eq!(parse_human_readable_size("0").unwrap(), 0);
        assert_eq!(parse_human_readable_size("1024").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("100").unwrap(), 100);
    }

    #[test]
    fn test_parse_human_readable_size_suffixes() {
        assert_eq!(parse_human_readable_size("1K").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1KB").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1k").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1kb").unwrap(), 1024);
        assert_eq!(
            parse_human_readable_size("10M").unwrap(),
            10 * 1024 * 1024
        );
        assert_eq!(
            parse_human_readable_size("10MB").unwrap(),
            10 * 1024 * 1024
        );
        assert_eq!(
            parse_human_readable_size("2G").unwrap(),
            2 * 1024 * 1024 * 1024
        );
        assert_eq!(
            parse_human_readable_size("500GB").unwrap(),
            500 * 1024 * 1024 * 1024
        );
        assert_eq!(
            parse_human_readable_size("1T").unwrap(),
            1024u64 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_parse_human_readable_size_errors() {
        assert!(parse_human_readable_size("").is_err());
        assert!(parse_human_readable_size("abc").is_err());
        assert!(parse_human_readable_size("10X").is_err());
    }

    #[test]
    fn test_escape_roundtrip() {
        let input = b"hello\x00world\ntest";
        let escaped = escape(input);
        assert_eq!(escaped, "hello\\x00world\\x0Atest");
    }

    #[test]
    fn test_escape_printable() {
        let input = b"hello world!";
        let escaped = escape(input);
        assert_eq!(escaped, "hello world!");
    }

    #[test]
    fn test_escape_backslash() {
        let input = b"a\\b";
        let escaped = escape(input);
        assert_eq!(escaped, "a\\\\b");
    }

    #[test]
    fn test_unescape_sequences() {
        assert_eq!(unescape("\\n"), vec![b'\n']);
        assert_eq!(unescape("\\t"), vec![b'\t']);
        assert_eq!(unescape("\\r"), vec![b'\r']);
        assert_eq!(unescape("\\\\"), vec![b'\\']);
        assert_eq!(unescape("\\0"), vec![0]);
        assert_eq!(unescape("\\x41"), vec![b'A']);
    }

    #[test]
    fn test_unescape_mixed() {
        let result = unescape("hello\\nworld");
        assert_eq!(result, b"hello\nworld");
    }

    #[test]
    fn test_unescape_plain() {
        let result = unescape("hello");
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_patterns_from_reader() {
        let input = b"pattern1\n\npattern2\npattern3\n";
        let patterns =
            patterns_from_reader(&input[..]).unwrap();
        assert_eq!(patterns, vec!["pattern1", "pattern2", "pattern3"]);
    }

    #[test]
    fn test_patterns_from_reader_empty() {
        let input = b"";
        let patterns = patterns_from_reader(&input[..]).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_default_color_specs() {
        let specs = ColorSpecs::default();
        let match_spec = specs.get(OutType::Match);
        assert_eq!(match_spec.fg(), Some(&Color::Red));
        assert!(match_spec.bold());

        let path_spec = specs.get(OutType::Path);
        assert_eq!(path_spec.fg(), Some(&Color::Magenta));

        let line_spec = specs.get(OutType::Line);
        assert_eq!(line_spec.fg(), Some(&Color::Green));

        let column_spec = specs.get(OutType::Column);
        assert_eq!(column_spec.fg(), Some(&Color::Green));
    }

    #[test]
    fn test_parse_color_specs_fg() {
        let specs = parse_color_specs(&[
            "match:fg:blue".to_string(),
        ])
        .unwrap();
        let match_spec = specs.get(OutType::Match);
        assert_eq!(match_spec.fg(), Some(&Color::Blue));
        // bold should still be true from default
        assert!(match_spec.bold());
    }

    #[test]
    fn test_parse_color_specs_none() {
        let specs = parse_color_specs(&[
            "match:none".to_string(),
        ])
        .unwrap();
        let match_spec = specs.get(OutType::Match);
        assert_eq!(match_spec.fg(), None);
        assert!(!match_spec.bold());
    }

    #[test]
    fn test_parse_color_specs_style() {
        let specs = parse_color_specs(&[
            "path:style:bold".to_string(),
            "path:style:underline".to_string(),
        ])
        .unwrap();
        let path_spec = specs.get(OutType::Path);
        assert!(path_spec.bold());
        assert!(path_spec.underline());
    }

    #[test]
    fn test_parse_color_specs_rgb() {
        let specs = parse_color_specs(&[
            "match:fg:255,128,0".to_string(),
        ])
        .unwrap();
        let match_spec = specs.get(OutType::Match);
        assert_eq!(match_spec.fg(), Some(&Color::Rgb(255, 128, 0)));
    }

    #[test]
    fn test_parse_color_specs_bg() {
        let specs = parse_color_specs(&[
            "line:bg:yellow".to_string(),
        ])
        .unwrap();
        let line_spec = specs.get(OutType::Line);
        assert_eq!(line_spec.bg(), Some(&Color::Yellow));
    }

    #[test]
    fn test_parse_color_specs_error() {
        assert!(parse_color_specs(&["invalid".to_string()]).is_err());
        assert!(
            parse_color_specs(&["match:fg:notacolor".to_string()]).is_err()
        );
        assert!(
            parse_color_specs(&["badtype:fg:red".to_string()]).is_err()
        );
    }

    #[test]
    fn test_escape_os() {
        let s = OsStr::new("hello");
        assert_eq!(escape_os(s), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn test_hostname_returns_something() {
        // On a Unix system, hostname should return Some value.
        let h = hostname();
        assert!(h.is_some());
        assert!(!h.unwrap().is_empty());
    }
}
