/*!
The `grep-printer` crate provides formatting and output of search results for
the ripgrep search tool.

It provides three main printer types:

- [`Standard`] — classic grep-like output (`path:line:content`).
- [`Summary`] — summary output (counts, file lists).
- [`JSON`] — JSON Lines structured output.

Each printer implements [`grep_searcher::Sink`] so it can be passed directly
to a [`grep_searcher::Searcher`].

Supporting types include [`ColorSpecs`] for controlling colorized output,
[`Stats`] for aggregating search statistics, [`HyperlinkConfig`] for
terminal hyperlinks, and [`UserColorSpec`] for parsing user-provided color
specifications.
*/

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use grep_matcher::{Match, Matcher};
use grep_searcher::{
    Searcher, Sink, SinkContext, SinkError, SinkFinish, SinkMatch,
};
use serde::Serialize;
use termcolor::{Color, ColorSpec, WriteColor};

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Aggregate search statistics collected across one or more searches.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    matched_lines: u64,
    matches: u64,
    files_with_matches: u64,
    files_searched: u64,
    bytes_searched: u64,
    bytes_printed: u64,
    elapsed: Duration,
}

impl Stats {
    /// Create a new empty `Stats`.
    pub fn new() -> Stats {
        Stats::default()
    }

    /// Return the total number of lines that contained at least one match.
    pub fn matched_lines(&self) -> u64 {
        self.matched_lines
    }

    /// Return the total number of individual matches.
    pub fn matches(&self) -> u64 {
        self.matches
    }

    /// Return the total number of files that contained at least one match.
    pub fn files_with_matches(&self) -> u64 {
        self.files_with_matches
    }

    /// Return the total number of files searched.
    pub fn files_searched(&self) -> u64 {
        self.files_searched
    }

    /// Return the total number of bytes searched.
    pub fn bytes_searched(&self) -> u64 {
        self.bytes_searched
    }

    /// Return the total number of bytes printed.
    pub fn bytes_printed(&self) -> u64 {
        self.bytes_printed
    }

    /// Return the total elapsed time.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Merge another `Stats` into this one.
    pub fn add(&mut self, other: &Stats) {
        self.matched_lines += other.matched_lines;
        self.matches += other.matches;
        self.files_with_matches += other.files_with_matches;
        self.files_searched += other.files_searched;
        self.bytes_searched += other.bytes_searched;
        self.bytes_printed += other.bytes_printed;
        self.elapsed += other.elapsed;
    }
}

// ---------------------------------------------------------------------------
// ColorSpecs
// ---------------------------------------------------------------------------

/// Parsed color configuration for search output.
///
/// Controls the colors used for paths, line numbers, column numbers, and
/// matched text.
#[derive(Clone, Debug)]
pub struct ColorSpecs {
    path: ColorSpec,
    line: ColorSpec,
    column: ColorSpec,
    matched: ColorSpec,
}

impl ColorSpecs {
    /// Create a `ColorSpecs` from a list of user-provided color
    /// specifications.
    pub fn new(specs: &[UserColorSpec]) -> ColorSpecs {
        let mut cs = ColorSpecs::default();
        for spec in specs {
            match spec.ty.as_str() {
                "path" | "fn" => spec.apply_to(&mut cs.path),
                "line" | "ln" => spec.apply_to(&mut cs.line),
                "column" | "cn" => spec.apply_to(&mut cs.column),
                "match" | "ms" | "mc" | "mt" => spec.apply_to(&mut cs.matched),
                _ => {}
            }
        }
        cs
    }

    /// Return the color spec for file paths.
    pub fn path(&self) -> &ColorSpec {
        &self.path
    }

    /// Return the color spec for line numbers.
    pub fn line(&self) -> &ColorSpec {
        &self.line
    }

    /// Return the color spec for column numbers.
    pub fn column(&self) -> &ColorSpec {
        &self.column
    }

    /// Return the color spec for matched text.
    pub fn matched(&self) -> &ColorSpec {
        &self.matched
    }
}

impl Default for ColorSpecs {
    fn default() -> ColorSpecs {
        let mut path = ColorSpec::new();
        path.set_fg(Some(Color::Magenta));

        let mut line = ColorSpec::new();
        line.set_fg(Some(Color::Green));

        let mut column = ColorSpec::new();
        column.set_fg(Some(Color::Green));

        let mut matched = ColorSpec::new();
        matched.set_fg(Some(Color::Red)).set_bold(true);

        ColorSpecs {
            path,
            line,
            column,
            matched,
        }
    }
}

// ---------------------------------------------------------------------------
// UserColorSpec
// ---------------------------------------------------------------------------

/// A user-provided color specification parsed from a string like
/// `match:fg:red` or `path:style:bold`.
#[derive(Clone, Debug)]
pub struct UserColorSpec {
    ty: String,
    attr: String,
    value: String,
}

impl UserColorSpec {
    fn apply_to(&self, spec: &mut ColorSpec) {
        match self.attr.as_str() {
            "fg" => {
                if let Some(c) = parse_color(&self.value) {
                    spec.set_fg(Some(c));
                } else if self.value == "none" {
                    spec.set_fg(None);
                }
            }
            "bg" => {
                if let Some(c) = parse_color(&self.value) {
                    spec.set_bg(Some(c));
                } else if self.value == "none" {
                    spec.set_bg(None);
                }
            }
            "style" | "attr" => match self.value.as_str() {
                "bold" => {
                    spec.set_bold(true);
                }
                "nobold" => {
                    spec.set_bold(false);
                }
                "intense" => {
                    spec.set_intense(true);
                }
                "nointense" => {
                    spec.set_intense(false);
                }
                "underline" => {
                    spec.set_underline(true);
                }
                "nounderline" => {
                    spec.set_underline(false);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

impl FromStr for UserColorSpec {
    type Err = ColorSpecParseError;

    fn from_str(s: &str) -> Result<UserColorSpec, ColorSpecParseError> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(ColorSpecParseError(format!(
                "invalid color spec '{}': expected type:attribute:value",
                s
            )));
        }
        Ok(UserColorSpec {
            ty: parts[0].to_lowercase(),
            attr: parts[1].to_lowercase(),
            value: parts[2].to_lowercase(),
        })
    }
}

/// An error that can occur when parsing a user color spec.
#[derive(Clone, Debug)]
pub struct ColorSpecParseError(String);

impl fmt::Display for ColorSpecParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ColorSpecParseError {}

/// Parse a color name into a `Color`.
fn parse_color(name: &str) -> Option<Color> {
    match name {
        "black" => Some(Color::Black),
        "blue" => Some(Color::Blue),
        "green" => Some(Color::Green),
        "red" => Some(Color::Red),
        "cyan" => Some(Color::Cyan),
        "magenta" => Some(Color::Magenta),
        "yellow" => Some(Color::Yellow),
        "white" => Some(Color::White),
        _ => {
            // Try parsing as ANSI 256 color (a plain number).
            if let Ok(n) = name.parse::<u8>() {
                Some(Color::Ansi256(n))
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HyperlinkConfig
// ---------------------------------------------------------------------------

/// Configuration for terminal hyperlinks in search output.
///
/// Hyperlinks use OSC 8 escape sequences. The format string may contain
/// `{path}`, `{line}`, `{column}`, and `{host}` placeholders.
#[derive(Clone, Debug)]
pub struct HyperlinkConfig {
    format: Option<String>,
}

impl HyperlinkConfig {
    /// Create a new `HyperlinkConfig` from a format string.
    ///
    /// Pass an empty string or a format string with placeholders like
    /// `file://{host}{path}` to enable hyperlinks. Supported placeholders:
    /// `{path}`, `{line}`, `{column}`, `{host}`.
    pub fn new(format: &str) -> HyperlinkConfig {
        if format.is_empty() {
            HyperlinkConfig { format: None }
        } else {
            HyperlinkConfig {
                format: Some(format.to_string()),
            }
        }
    }

    /// Return true if hyperlinks are enabled.
    fn is_enabled(&self) -> bool {
        self.format.is_some()
    }

    /// Render the hyperlink URL for the given path, line, and column.
    fn render(
        &self,
        path: &Path,
        line: Option<u64>,
        column: Option<u64>,
    ) -> Option<String> {
        let fmt = self.format.as_ref()?;
        let path_str = path.to_string_lossy();
        let host = env::var("HOSTNAME")
            .or_else(|_| env::var("COMPUTERNAME"))
            .unwrap_or_default();
        let mut result = fmt.replace("{path}", &path_str);
        result = result.replace("{host}", &host);
        if let Some(l) = line {
            result = result.replace("{line}", &l.to_string());
        } else {
            result = result.replace("{line}", "");
        }
        if let Some(c) = column {
            result = result.replace("{column}", &c.to_string());
        } else {
            result = result.replace("{column}", "");
        }
        Some(result)
    }
}

impl Default for HyperlinkConfig {
    fn default() -> HyperlinkConfig {
        HyperlinkConfig { format: None }
    }
}

// ---------------------------------------------------------------------------
// PrinterError
// ---------------------------------------------------------------------------

/// The error type used by all printers.
///
/// This wraps `io::Error` and implements `SinkError`.
#[derive(Debug)]
pub struct PrinterError(io::Error);

impl fmt::Display for PrinterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for PrinterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl SinkError for PrinterError {
    fn error_message<T: fmt::Display>(message: T) -> PrinterError {
        PrinterError(io::Error::new(io::ErrorKind::Other, message.to_string()))
    }

    fn error_io(err: io::Error) -> PrinterError {
        PrinterError(err)
    }
}

impl From<io::Error> for PrinterError {
    fn from(err: io::Error) -> PrinterError {
        PrinterError(err)
    }
}

// ---------------------------------------------------------------------------
// Utility: trim leading whitespace
// ---------------------------------------------------------------------------

/// Compute the number of leading ASCII whitespace bytes.
fn leading_whitespace_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count()
}

/// Strip trailing line terminators (LF, CRLF).
fn trim_line_terminator(mut bytes: &[u8]) -> &[u8] {
    if bytes.last() == Some(&b'\n') {
        bytes = &bytes[..bytes.len() - 1];
    }
    if bytes.last() == Some(&b'\r') {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

// ---------------------------------------------------------------------------
// StandardBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and constructing a [`Standard`] printer.
#[derive(Clone, Debug)]
pub struct StandardBuilder {
    color_specs: ColorSpecs,
    stats: bool,
    heading: Option<bool>,
    per_match: bool,
    column: bool,
    byte_offset: bool,
    only_matching: bool,
    replacement: Option<Vec<u8>>,
    trim: bool,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    separator_field_match: Vec<u8>,
    separator_field_context: Vec<u8>,
    separator_context: Option<Vec<u8>>,
    separator_path: Option<u8>,
    path_terminator: Option<u8>,
    hyperlink: HyperlinkConfig,
}

impl StandardBuilder {
    /// Create a new `StandardBuilder` with default settings.
    pub fn new() -> StandardBuilder {
        StandardBuilder {
            color_specs: ColorSpecs::default(),
            stats: false,
            heading: None,
            per_match: false,
            column: false,
            byte_offset: false,
            only_matching: false,
            replacement: None,
            trim: false,
            max_columns: None,
            max_columns_preview: false,
            separator_field_match: b":".to_vec(),
            separator_field_context: b"-".to_vec(),
            separator_context: Some(b"--".to_vec()),
            separator_path: None,
            path_terminator: None,
            hyperlink: HyperlinkConfig::default(),
        }
    }

    /// Build a [`Standard`] printer.
    ///
    /// `path` is the file path to display (if any). `matcher` is used to
    /// find sub-match positions for colorizing and `--only-matching`.
    /// `wtr` is the output destination.
    pub fn build<M: Matcher, W: WriteColor>(
        &self,
        path: Option<&Path>,
        matcher: M,
        wtr: W,
    ) -> Standard<M, W> {
        let has_path = path.is_some();
        let heading = self.heading.unwrap_or(has_path);
        Standard {
            matcher,
            wtr,
            path: path.map(|p| p.to_path_buf()),
            color_specs: self.color_specs.clone(),
            stats: if self.stats {
                Some(Stats::new())
            } else {
                None
            },
            heading,
            per_match: self.per_match,
            column: self.column,
            byte_offset: self.byte_offset,
            only_matching: self.only_matching,
            replacement: self.replacement.clone(),
            trim: self.trim,
            max_columns: self.max_columns,
            max_columns_preview: self.max_columns_preview,
            separator_field_match: self.separator_field_match.clone(),
            separator_field_context: self.separator_field_context.clone(),
            separator_context: self.separator_context.clone(),
            separator_path: self.separator_path,
            path_terminator: self.path_terminator,
            hyperlink: self.hyperlink.clone(),
            has_printed: false,
            needs_separator: false,
            match_count: 0,
            binary_byte_offset: None,
            bytes_printed: 0,
        }
    }

    /// Set whether to print the file path as a heading before matches.
    pub fn heading(&mut self, yes: bool) -> &mut Self {
        self.heading = Some(yes);
        self
    }

    /// Set color specifications.
    pub fn color_specs(&mut self, specs: ColorSpecs) -> &mut Self {
        self.color_specs = specs;
        self
    }

    /// Enable or disable statistics tracking.
    pub fn stats(&mut self, yes: bool) -> &mut Self {
        self.stats = yes;
        self
    }

    /// Enable per-match reporting (for `--vimgrep`).
    pub fn per_match(&mut self, yes: bool) -> &mut Self {
        self.per_match = yes;
        self
    }

    /// Enable printing column numbers.
    pub fn column(&mut self, yes: bool) -> &mut Self {
        self.column = yes;
        self
    }

    /// Enable printing byte offsets.
    pub fn byte_offset(&mut self, yes: bool) -> &mut Self {
        self.byte_offset = yes;
        self
    }

    /// Enable only-matching mode.
    pub fn only_matching(&mut self, yes: bool) -> &mut Self {
        self.only_matching = yes;
        self
    }

    /// Set replacement text for matched content.
    pub fn replacement(&mut self, replacement: Option<Vec<u8>>) -> &mut Self {
        self.replacement = replacement;
        self
    }

    /// Enable trimming of leading whitespace.
    pub fn trim(&mut self, yes: bool) -> &mut Self {
        self.trim = yes;
        self
    }

    /// Set the maximum number of columns to print per line.
    pub fn max_columns(&mut self, max: Option<u64>) -> &mut Self {
        self.max_columns = max;
        self
    }

    /// Show a preview of lines that exceed `max_columns`.
    pub fn max_columns_preview(&mut self, yes: bool) -> &mut Self {
        self.max_columns_preview = yes;
        self
    }

    /// Set the field separator for match lines (default `:` ).
    pub fn separator_field_match(&mut self, sep: Vec<u8>) -> &mut Self {
        self.separator_field_match = sep;
        self
    }

    /// Set the field separator for context lines (default `-`).
    pub fn separator_field_context(&mut self, sep: Vec<u8>) -> &mut Self {
        self.separator_field_context = sep;
        self
    }

    /// Set the context group separator (default `--`). `None` disables it.
    pub fn separator_context(&mut self, sep: Option<Vec<u8>>) -> &mut Self {
        self.separator_context = sep;
        self
    }

    /// Set the path separator byte (replaces default OS separator).
    pub fn separator_path(&mut self, sep: Option<u8>) -> &mut Self {
        self.separator_path = sep;
        self
    }

    /// Set the path terminator byte (e.g. NUL for `--null`).
    pub fn path_terminator(&mut self, term: Option<u8>) -> &mut Self {
        self.path_terminator = term;
        self
    }

    /// Set hyperlink configuration.
    pub fn hyperlink(&mut self, config: HyperlinkConfig) -> &mut Self {
        self.hyperlink = config;
        self
    }
}

impl Default for StandardBuilder {
    fn default() -> StandardBuilder {
        StandardBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Standard printer
// ---------------------------------------------------------------------------

/// A printer that outputs search results in the classic grep format.
///
/// This printer implements [`grep_searcher::Sink`].
pub struct Standard<M: Matcher, W: WriteColor> {
    matcher: M,
    wtr: W,
    path: Option<PathBuf>,
    color_specs: ColorSpecs,
    stats: Option<Stats>,
    heading: bool,
    per_match: bool,
    column: bool,
    byte_offset: bool,
    only_matching: bool,
    replacement: Option<Vec<u8>>,
    trim: bool,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    separator_field_match: Vec<u8>,
    separator_field_context: Vec<u8>,
    separator_context: Option<Vec<u8>>,
    separator_path: Option<u8>,
    path_terminator: Option<u8>,
    hyperlink: HyperlinkConfig,
    // Runtime state
    has_printed: bool,
    needs_separator: bool,
    match_count: u64,
    binary_byte_offset: Option<u64>,
    bytes_printed: u64,
}

impl<M: Matcher, W: WriteColor> Standard<M, W> {
    /// Return a reference to the collected stats, if stats tracking was
    /// enabled.
    pub fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }

    /// Return a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    /// Return whether this printer has printed any output.
    pub fn has_printed(&self) -> bool {
        self.has_printed
    }

    // -- internal helpers --

    fn write_path(&mut self, sep: &[u8]) -> Result<(), PrinterError> {
        if let Some(ref path) = self.path {
            let path_bytes = path_bytes_with_separator(path, self.separator_path);
            self.wtr.set_color(&self.color_specs.path)?;
            self.wtr.write_all(&path_bytes)?;
            self.wtr.reset()?;
            if let Some(term) = self.path_terminator {
                self.wtr.write_all(&[term])?;
            } else {
                self.wtr.write_all(sep)?;
            }
        }
        Ok(())
    }

    fn write_line_number(
        &mut self,
        line_number: Option<u64>,
        sep: &[u8],
    ) -> Result<(), PrinterError> {
        if let Some(n) = line_number {
            self.wtr.set_color(&self.color_specs.line)?;
            write!(self.wtr, "{}", n)?;
            self.wtr.reset()?;
            self.wtr.write_all(sep)?;
        }
        Ok(())
    }

    fn write_column(
        &mut self,
        col: u64,
        sep: &[u8],
    ) -> Result<(), PrinterError> {
        self.wtr.set_color(&self.color_specs.column)?;
        write!(self.wtr, "{}", col)?;
        self.wtr.reset()?;
        self.wtr.write_all(sep)?;
        Ok(())
    }

    fn write_byte_offset(
        &mut self,
        offset: u64,
        sep: &[u8],
    ) -> Result<(), PrinterError> {
        self.wtr.set_color(&self.color_specs.line)?;
        write!(self.wtr, "{}", offset)?;
        self.wtr.reset()?;
        self.wtr.write_all(sep)?;
        Ok(())
    }

    /// Find all submatch positions in the given line bytes.
    fn find_submatches(&self, line: &[u8]) -> Vec<Match> {
        let mut matches = Vec::new();
        let _ = self.matcher.find_iter(line, |m| {
            matches.push(m);
            true
        });
        matches
    }

    /// Write line bytes with matched portions highlighted.
    fn write_colored_line(
        &mut self,
        line: &[u8],
        submatches: &[Match],
    ) -> Result<(), PrinterError> {
        if submatches.is_empty() {
            self.wtr.write_all(line)?;
            return Ok(());
        }
        let mut last = 0;
        for m in submatches {
            let start = m.start().min(line.len());
            let end = m.end().min(line.len());
            if start > last {
                self.wtr.write_all(&line[last..start])?;
            }
            if start < end {
                self.wtr.set_color(&self.color_specs.matched)?;
                self.wtr.write_all(&line[start..end])?;
                self.wtr.reset()?;
            }
            last = end;
        }
        if last < line.len() {
            self.wtr.write_all(&line[last..])?;
        }
        Ok(())
    }

    /// Write replaced line content.
    fn write_replaced_line(
        &mut self,
        line: &[u8],
        submatches: &[Match],
        replacement: &[u8],
    ) -> Result<(), PrinterError> {
        if submatches.is_empty() {
            self.wtr.write_all(line)?;
            return Ok(());
        }
        let mut last = 0;
        for m in submatches {
            let start = m.start().min(line.len());
            let end = m.end().min(line.len());
            if start > last {
                self.wtr.write_all(&line[last..start])?;
            }
            self.wtr.set_color(&self.color_specs.matched)?;
            self.wtr.write_all(replacement)?;
            self.wtr.reset()?;
            last = end;
        }
        if last < line.len() {
            self.wtr.write_all(&line[last..])?;
        }
        Ok(())
    }

    fn write_context_separator(&mut self) -> Result<(), PrinterError> {
        if let Some(ref sep) = self.separator_context {
            self.wtr.write_all(sep)?;
            self.wtr.write_all(b"\n")?;
        }
        Ok(())
    }

    /// Handle a matched line, potentially emitting multiple lines in
    /// `--only-matching` or `--per-match` mode.
    fn handle_match(
        &mut self,
        line_number: Option<u64>,
        absolute_offset: u64,
        line_bytes: &[u8],
    ) -> Result<(), PrinterError> {
        // Strip trailing line terminator for matching purposes.
        let content = trim_line_terminator(line_bytes);
        let trim_offset = if self.trim {
            leading_whitespace_len(content)
        } else {
            0
        };
        let trimmed = &content[trim_offset..];

        // Find submatches within the content (not including line terminator).
        let submatches = self.find_submatches(content);

        let sep = self.separator_field_match.clone();
        let replacement = self.replacement.clone();

        // Check max_columns
        if let Some(max_cols) = self.max_columns {
            if trimmed.len() as u64 > max_cols {
                if self.max_columns_preview {
                    // Print truncated preview
                    if !self.heading {
                        self.write_path(&sep)?;
                    }
                    self.write_line_number(line_number, &sep)?;
                    if self.column {
                        let col = submatches.first().map(|m| {
                            let c = m.start() as u64 + 1;
                            if self.trim { c.saturating_sub(trim_offset as u64) } else { c }
                        }).unwrap_or(0);
                        if col > 0 {
                            self.write_column(col, &sep)?;
                        }
                    }
                    if self.byte_offset {
                        self.write_byte_offset(absolute_offset, &sep)?;
                    }
                    let preview_len = max_cols as usize;
                    let preview = &trimmed[..preview_len.min(trimmed.len())];
                    let preview_subs: Vec<Match> = submatches
                        .iter()
                        .filter(|m| m.start() >= trim_offset && m.start() < trim_offset + preview_len)
                        .map(|m| Match::new(
                            m.start().saturating_sub(trim_offset),
                            m.end().saturating_sub(trim_offset).min(preview_len),
                        ))
                        .collect();
                    self.write_colored_line(preview, &preview_subs)?;
                    write!(self.wtr, " [... {} chars omitted]", trimmed.len() - preview_len)?;
                    self.wtr.write_all(b"\n")?;
                } else {
                    // Just print the path/line number and a note
                    if !self.heading {
                        self.write_path(&sep)?;
                    }
                    self.write_line_number(line_number, &sep)?;
                    if self.byte_offset {
                        self.write_byte_offset(absolute_offset, &sep)?;
                    }
                    write!(
                        self.wtr,
                        "[Omitted long line with {} chars]",
                        trimmed.len()
                    )?;
                    self.wtr.write_all(b"\n")?;
                }
                return Ok(());
            }
        }

        if self.only_matching {
            // Emit one line per submatch.
            for m in &submatches {
                let match_bytes = &content[m.start()..m.end()];
                if !self.heading {
                    self.write_path(&sep)?;
                }
                // In only-matching + per_match mode, always emit line number.
                self.write_line_number(line_number, &sep)?;
                if self.column {
                    let col = m.start() as u64 + 1
                        - if self.trim { trim_offset as u64 } else { 0 };
                    self.write_column(col, &sep)?;
                }
                if self.byte_offset {
                    self.write_byte_offset(
                        absolute_offset + m.start() as u64,
                        &sep,
                    )?;
                }
                if let Some(ref rep) = replacement {
                    self.wtr.set_color(&self.color_specs.matched)?;
                    self.wtr.write_all(rep)?;
                    self.wtr.reset()?;
                } else {
                    self.wtr.set_color(&self.color_specs.matched)?;
                    self.wtr.write_all(match_bytes)?;
                    self.wtr.reset()?;
                }
                self.wtr.write_all(b"\n")?;

                if let Some(ref mut stats) = self.stats {
                    stats.matches += 1;
                }
            }
            if let Some(ref mut stats) = self.stats {
                stats.matched_lines += 1;
            }
            return Ok(());
        }

        if self.per_match && submatches.len() > 1 {
            // Emit the full line once per submatch (for vimgrep).
            for _m in &submatches {
                if !self.heading {
                    self.write_path(&sep)?;
                }
                self.write_line_number(line_number, &sep)?;
                if self.column {
                    let col = _m.start() as u64 + 1
                        - if self.trim { trim_offset as u64 } else { 0 };
                    self.write_column(col, &sep)?;
                }
                if self.byte_offset {
                    self.write_byte_offset(absolute_offset, &sep)?;
                }
                if let Some(ref rep) = replacement {
                    self.write_replaced_line(trimmed, &adjust_matches(&submatches, trim_offset), rep)?;
                } else {
                    self.write_colored_line(trimmed, &adjust_matches(&submatches, trim_offset))?;
                }
                self.wtr.write_all(b"\n")?;

                if let Some(ref mut stats) = self.stats {
                    stats.matches += 1;
                }
            }
            if let Some(ref mut stats) = self.stats {
                stats.matched_lines += 1;
            }
            return Ok(());
        }

        // Normal mode: emit the line once.
        if !self.heading {
            self.write_path(&sep)?;
        }
        self.write_line_number(line_number, &sep)?;
        if self.column {
            let col = submatches.first().map(|m| {
                let c = m.start() as u64 + 1;
                if self.trim { c.saturating_sub(trim_offset as u64) } else { c }
            }).unwrap_or(0);
            if col > 0 {
                self.write_column(col, &sep)?;
            }
        }
        if self.byte_offset {
            self.write_byte_offset(absolute_offset, &sep)?;
        }

        let adjusted = adjust_matches(&submatches, trim_offset);
        if let Some(ref rep) = replacement {
            self.write_replaced_line(trimmed, &adjusted, rep)?;
        } else {
            self.write_colored_line(trimmed, &adjusted)?;
        }
        self.wtr.write_all(b"\n")?;

        if let Some(ref mut stats) = self.stats {
            stats.matched_lines += 1;
            stats.matches += submatches.len() as u64;
        }

        Ok(())
    }
}

/// Adjust submatch positions after trimming leading whitespace.
fn adjust_matches(matches: &[Match], trim_offset: usize) -> Vec<Match> {
    if trim_offset == 0 {
        return matches.to_vec();
    }
    matches
        .iter()
        .filter_map(|m| {
            let start = m.start().checked_sub(trim_offset)?;
            let end = m.end().saturating_sub(trim_offset);
            if start <= end {
                Some(Match::new(start, end))
            } else {
                None
            }
        })
        .collect()
}

impl<M: Matcher, W: WriteColor> Sink for Standard<M, W> {
    type Error = PrinterError;

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, PrinterError> {
        self.has_printed = false;
        self.needs_separator = false;
        self.match_count = 0;
        self.binary_byte_offset = None;
        self.bytes_printed = 0;

        if self.heading {
            if let Some(ref path) = self.path {
                let path_bytes = path_bytes_with_separator(path, self.separator_path);
                self.wtr.set_color(&self.color_specs.path)?;
                self.wtr.write_all(&path_bytes)?;
                self.wtr.reset()?;
                if let Some(term) = self.path_terminator {
                    self.wtr.write_all(&[term])?;
                }
                self.wtr.write_all(b"\n")?;
                self.has_printed = true;
            }
        }
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, PrinterError> {
        if self.needs_separator {
            self.write_context_separator()?;
            self.needs_separator = false;
        }
        self.handle_match(
            mat.line_number(),
            mat.absolute_byte_offset(),
            mat.bytes(),
        )?;
        self.has_printed = true;
        self.match_count += 1;
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, PrinterError> {
        let content = trim_line_terminator(ctx.bytes());
        let trim_offset = if self.trim {
            leading_whitespace_len(content)
        } else {
            0
        };
        let trimmed = &content[trim_offset..];
        let sep = &self.separator_field_context.clone();

        if !self.heading {
            self.write_path(sep)?;
        }
        self.write_line_number(ctx.line_number(), sep)?;
        if self.column {
            // Context lines get a 0 column (matching ripgrep behavior).
            self.write_column(0, sep)?;
        }
        if self.byte_offset {
            self.write_byte_offset(ctx.absolute_byte_offset(), sep)?;
        }
        self.wtr.write_all(trimmed)?;
        self.wtr.write_all(b"\n")?;
        self.has_printed = true;
        Ok(true)
    }

    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, PrinterError> {
        self.needs_separator = true;
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), PrinterError> {
        if let Some(ref mut stats) = self.stats {
            stats.bytes_searched += finish.byte_count;
            stats.files_searched += 1;
            if self.match_count > 0 {
                stats.files_with_matches += 1;
            }
        }
        self.binary_byte_offset = finish.binary_byte_offset;
        Ok(())
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, PrinterError> {
        self.binary_byte_offset = Some(binary_byte_offset);
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// SummaryKind
// ---------------------------------------------------------------------------

/// The kind of summary output to produce.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummaryKind {
    /// Print the count of matching lines per file (for `-c`).
    Count,
    /// Print the count of individual matches per file.
    CountMatches,
    /// Print filenames that contain at least one match (for `-l`).
    FilesWithMatches,
    /// Print filenames that contain no matches.
    FilesWithoutMatch,
}

// ---------------------------------------------------------------------------
// SummaryBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and constructing a [`Summary`] printer.
#[derive(Clone, Debug)]
pub struct SummaryBuilder {
    kind: SummaryKind,
    color_specs: ColorSpecs,
    stats: bool,
    path_terminator: Option<u8>,
    separator_field: Vec<u8>,
    separator_path: Option<u8>,
}

impl SummaryBuilder {
    /// Create a new `SummaryBuilder` with default settings.
    pub fn new() -> SummaryBuilder {
        SummaryBuilder {
            kind: SummaryKind::Count,
            color_specs: ColorSpecs::default(),
            stats: false,
            path_terminator: None,
            separator_field: b":".to_vec(),
            separator_path: None,
        }
    }

    /// Build a [`Summary`] printer.
    pub fn build<W: WriteColor>(
        &self,
        path: Option<&Path>,
        wtr: W,
    ) -> Summary<W> {
        Summary {
            wtr,
            path: path.map(|p| p.to_path_buf()),
            kind: self.kind,
            color_specs: self.color_specs.clone(),
            stats: if self.stats {
                Some(Stats::new())
            } else {
                None
            },
            path_terminator: self.path_terminator,
            separator_field: self.separator_field.clone(),
            separator_path: self.separator_path,
            match_count: 0,
            matched_lines: 0,
            binary_byte_offset: None,
        }
    }

    /// Set the summary kind.
    pub fn kind(&mut self, kind: SummaryKind) -> &mut Self {
        self.kind = kind;
        self
    }

    /// Set color specifications.
    pub fn color_specs(&mut self, specs: ColorSpecs) -> &mut Self {
        self.color_specs = specs;
        self
    }

    /// Enable or disable statistics tracking.
    pub fn stats(&mut self, yes: bool) -> &mut Self {
        self.stats = yes;
        self
    }

    /// Set the path terminator byte (for `--null`).
    pub fn path_terminator(&mut self, term: Option<u8>) -> &mut Self {
        self.path_terminator = term;
        self
    }

    /// Set the field separator.
    pub fn separator_field(&mut self, sep: Vec<u8>) -> &mut Self {
        self.separator_field = sep;
        self
    }

    /// Set the path separator byte.
    pub fn separator_path(&mut self, sep: Option<u8>) -> &mut Self {
        self.separator_path = sep;
        self
    }
}

impl Default for SummaryBuilder {
    fn default() -> SummaryBuilder {
        SummaryBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Summary printer
// ---------------------------------------------------------------------------

/// A printer that outputs summary information about search results.
///
/// Supports counts, file lists with/without matches, etc.
pub struct Summary<W: WriteColor> {
    wtr: W,
    path: Option<PathBuf>,
    kind: SummaryKind,
    color_specs: ColorSpecs,
    stats: Option<Stats>,
    path_terminator: Option<u8>,
    separator_field: Vec<u8>,
    separator_path: Option<u8>,
    // Runtime state
    match_count: u64,
    matched_lines: u64,
    binary_byte_offset: Option<u64>,
}

impl<W: WriteColor> Summary<W> {
    /// Return a reference to the collected stats, if enabled.
    pub fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }

    /// Return a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    fn write_path(&mut self) -> Result<(), PrinterError> {
        if let Some(ref path) = self.path {
            let path_bytes = path_bytes_with_separator(path, self.separator_path);
            self.wtr.set_color(&self.color_specs.path)?;
            self.wtr.write_all(&path_bytes)?;
            self.wtr.reset()?;
        }
        Ok(())
    }

    fn write_path_terminator(&mut self) -> Result<(), PrinterError> {
        if let Some(term) = self.path_terminator {
            self.wtr.write_all(&[term])?;
        }
        Ok(())
    }

    /// Count submatches in a line using a simple byte search.
    /// For the Summary printer, we don't have a matcher — we just count
    /// each call to `matched()` as one matched line and track stats
    /// accordingly.
    fn count_matches_in_line(&self, _bytes: &[u8]) -> u64 {
        // We count each SinkMatch callback as 1 matched line.
        // Individual match counting would need a Matcher, but Summary
        // doesn't take one. We approximate by counting 1 per call.
        1
    }
}

impl<W: WriteColor> Sink for Summary<W> {
    type Error = PrinterError;

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, PrinterError> {
        self.match_count = 0;
        self.matched_lines = 0;
        self.binary_byte_offset = None;
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        _mat: &SinkMatch<'_>,
    ) -> Result<bool, PrinterError> {
        self.match_count += 1;
        self.matched_lines += 1;
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), PrinterError> {
        self.binary_byte_offset = finish.binary_byte_offset;

        match self.kind {
            SummaryKind::Count => {
                self.write_path()?;
                if self.path.is_some() {
                    self.write_path_terminator()?;
                    self.wtr.write_all(&self.separator_field.clone())?;
                }
                write!(self.wtr, "{}", self.matched_lines)?;
                self.wtr.write_all(b"\n")?;
            }
            SummaryKind::CountMatches => {
                self.write_path()?;
                if self.path.is_some() {
                    self.write_path_terminator()?;
                    self.wtr.write_all(&self.separator_field.clone())?;
                }
                write!(self.wtr, "{}", self.match_count)?;
                self.wtr.write_all(b"\n")?;
            }
            SummaryKind::FilesWithMatches => {
                if self.match_count > 0 {
                    self.write_path()?;
                    self.write_path_terminator()?;
                    self.wtr.write_all(b"\n")?;
                }
            }
            SummaryKind::FilesWithoutMatch => {
                if self.match_count == 0 {
                    self.write_path()?;
                    self.write_path_terminator()?;
                    self.wtr.write_all(b"\n")?;
                }
            }
        }

        if let Some(ref mut stats) = self.stats {
            stats.bytes_searched += finish.byte_count;
            stats.files_searched += 1;
            stats.matched_lines += self.matched_lines;
            stats.matches += self.match_count;
            if self.match_count > 0 {
                stats.files_with_matches += 1;
            }
        }

        Ok(())
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, PrinterError> {
        self.binary_byte_offset = Some(binary_byte_offset);
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// JSONBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and constructing a [`JSON`] printer.
#[derive(Clone, Debug)]
pub struct JSONBuilder {
    stats: bool,
}

impl JSONBuilder {
    /// Create a new `JSONBuilder` with default settings.
    pub fn new() -> JSONBuilder {
        JSONBuilder { stats: false }
    }

    /// Build a [`JSON`] printer.
    pub fn build<M: Matcher, W: io::Write>(
        &self,
        path: Option<&Path>,
        matcher: M,
        wtr: W,
    ) -> JSON<M, W> {
        JSON {
            matcher,
            wtr,
            path: path.map(|p| p.to_path_buf()),
            stats: if self.stats {
                Some(Stats::new())
            } else {
                None
            },
            match_count: 0,
            matched_lines: 0,
        }
    }

    /// Enable or disable statistics tracking.
    pub fn stats(&mut self, yes: bool) -> &mut Self {
        self.stats = yes;
        self
    }
}

impl Default for JSONBuilder {
    fn default() -> JSONBuilder {
        JSONBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// JSON printer
// ---------------------------------------------------------------------------

/// A printer that outputs search results as JSON Lines.
///
/// Each output line is a JSON object with a `type` field indicating the
/// kind of message.
pub struct JSON<M: Matcher, W: io::Write> {
    matcher: M,
    wtr: W,
    path: Option<PathBuf>,
    stats: Option<Stats>,
    match_count: u64,
    matched_lines: u64,
}

// JSON message types (serialized output)
#[derive(Serialize)]
struct JsonMessage<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    data: serde_json::Value,
}

impl<M: Matcher, W: io::Write> JSON<M, W> {
    /// Return a reference to the collected stats, if enabled.
    pub fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }

    /// Return a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    fn path_data(&self) -> serde_json::Value {
        match self.path {
            Some(ref p) => {
                let path_str = p.to_string_lossy();
                let bytes = p.to_string_lossy().into_owned().into_bytes();
                // Try text representation first.
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    serde_json::json!({"text": s})
                } else {
                    let b64 = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        &bytes,
                    );
                    serde_json::json!({"bytes": b64})
                }
            }
            None => serde_json::Value::Null,
        }
    }

    fn bytes_data(bytes: &[u8]) -> serde_json::Value {
        if let Ok(s) = std::str::from_utf8(bytes) {
            serde_json::json!({"text": s})
        } else {
            let b64 = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                bytes,
            );
            serde_json::json!({"bytes": b64})
        }
    }

    fn write_message(&mut self, msg: &JsonMessage<'_>) -> Result<(), PrinterError> {
        serde_json::to_writer(&mut self.wtr, msg)
            .map_err(|e| PrinterError(io::Error::new(io::ErrorKind::Other, e)))?;
        self.wtr.write_all(b"\n")?;
        Ok(())
    }

    fn find_submatches(&self, line: &[u8]) -> Vec<Match> {
        let mut matches = Vec::new();
        let _ = self.matcher.find_iter(line, |m| {
            matches.push(m);
            true
        });
        matches
    }
}

impl<M: Matcher, W: io::Write> Sink for JSON<M, W> {
    type Error = PrinterError;

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, PrinterError> {
        self.match_count = 0;
        self.matched_lines = 0;

        let data = serde_json::json!({
            "path": self.path_data(),
        });
        let msg = JsonMessage {
            msg_type: "begin",
            data,
        };
        self.write_message(&msg)?;
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, PrinterError> {
        let line_bytes = mat.bytes();
        let content = trim_line_terminator(line_bytes);
        let submatches = self.find_submatches(content);

        let submatch_values: Vec<serde_json::Value> = submatches
            .iter()
            .map(|m| {
                let match_bytes = &content[m.start()..m.end()];
                serde_json::json!({
                    "match": Self::bytes_data(match_bytes),
                    "start": m.start(),
                    "end": m.end(),
                })
            })
            .collect();

        let mut data = serde_json::json!({
            "path": self.path_data(),
            "lines": Self::bytes_data(line_bytes),
            "line_number": mat.line_number(),
            "absolute_offset": mat.absolute_byte_offset(),
            "submatches": submatch_values,
        });

        let msg = JsonMessage {
            msg_type: "match",
            data,
        };
        self.write_message(&msg)?;

        self.match_count += submatches.len().max(1) as u64;
        self.matched_lines += 1;
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, PrinterError> {
        let data = serde_json::json!({
            "path": self.path_data(),
            "lines": Self::bytes_data(ctx.bytes()),
            "line_number": ctx.line_number(),
            "absolute_offset": ctx.absolute_byte_offset(),
            "submatches": [],
        });

        let msg = JsonMessage {
            msg_type: "context",
            data,
        };
        self.write_message(&msg)?;
        Ok(true)
    }

    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, PrinterError> {
        // In JSON mode, context breaks are not emitted as separate messages.
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), PrinterError> {
        let stats_value = serde_json::json!({
            "matched_lines": self.matched_lines,
            "matches": self.match_count,
            "bytes_searched": finish.byte_count,
            "bytes_printed": 0u64,
            "elapsed": {
                "secs": 0u64,
                "nanos": 0u64,
            },
        });

        let data = serde_json::json!({
            "path": self.path_data(),
            "stats": stats_value,
            "binary_offset": finish.binary_byte_offset,
        });

        let msg = JsonMessage {
            msg_type: "end",
            data,
        };
        self.write_message(&msg)?;

        if let Some(ref mut stats) = self.stats {
            stats.bytes_searched += finish.byte_count;
            stats.files_searched += 1;
            stats.matched_lines += self.matched_lines;
            stats.matches += self.match_count;
            if self.match_count > 0 {
                stats.files_with_matches += 1;
            }
        }

        Ok(())
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, PrinterError> {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Convert a `Path` to bytes, optionally replacing the path separator.
fn path_bytes_with_separator(path: &Path, sep: Option<u8>) -> Vec<u8> {
    let path_str = path.to_string_lossy();
    let mut bytes = path_str.as_bytes().to_vec();
    if let Some(sep_byte) = sep {
        for b in bytes.iter_mut() {
            if *b == std::path::MAIN_SEPARATOR as u8 {
                *b = sep_byte;
            }
        }
    }
    bytes
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Match, Matcher, NoCaptures, NoError};
    use grep_searcher::{Searcher, SearcherBuilder};
    use termcolor::NoColor;

    /// A simple literal matcher for testing.
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

    fn get_output(buf: Vec<u8>) -> String {
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_standard_basic_match() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\ngoodbye world\nhello again\n";
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new().build(
            None,
            LiteralMatcher::new(b"hello"),
            buf,
        );
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("hello world"));
        assert!(output.contains("hello again"));
        assert!(!output.contains("goodbye"));
    }

    #[test]
    fn test_standard_with_path() {
        let matcher = LiteralMatcher::new(b"test");
        let data = b"test line\n";
        let path = Path::new("foo.txt");
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new().build(
            Some(path),
            LiteralMatcher::new(b"test"),
            buf,
        );
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        // In heading mode (default with path), path is on first line.
        assert!(output.contains("foo.txt"));
        assert!(output.contains("test line"));
    }

    #[test]
    fn test_standard_no_heading() {
        let matcher = LiteralMatcher::new(b"test");
        let data = b"test line\n";
        let path = Path::new("foo.txt");
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new()
            .heading(false)
            .build(Some(path), LiteralMatcher::new(b"test"), buf);
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        // Path should be inline with the match.
        assert!(output.contains("foo.txt:"));
    }

    #[test]
    fn test_standard_line_numbers() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"line1\nmatch here\nline3\n";
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new().build(
            None,
            LiteralMatcher::new(b"match"),
            buf,
        );
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("2:"));
    }

    #[test]
    fn test_summary_count() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello\nworld\nhello\n";
        let path = Path::new("test.txt");
        let buf = NoColor::new(Vec::new());
        let mut printer = SummaryBuilder::new()
            .kind(SummaryKind::Count)
            .build(Some(path), buf);
        let mut searcher = Searcher::new();
        searcher
            .search_slice(&matcher, data, &mut printer)
            .unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("2"), "Expected count of 2, got: {}", output);
    }

    #[test]
    fn test_summary_files_with_matches() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\n";
        let path = Path::new("found.txt");
        let buf = NoColor::new(Vec::new());
        let mut printer = SummaryBuilder::new()
            .kind(SummaryKind::FilesWithMatches)
            .build(Some(path), buf);
        let mut searcher = Searcher::new();
        searcher
            .search_slice(&matcher, data, &mut printer)
            .unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("found.txt"));
    }

    #[test]
    fn test_summary_files_without_match() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"goodbye world\n";
        let path = Path::new("notfound.txt");
        let buf = NoColor::new(Vec::new());
        let mut printer = SummaryBuilder::new()
            .kind(SummaryKind::FilesWithoutMatch)
            .build(Some(path), buf);
        let mut searcher = Searcher::new();
        searcher
            .search_slice(&matcher, data, &mut printer)
            .unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("notfound.txt"));
    }

    #[test]
    fn test_json_basic() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\n";
        let path = Path::new("test.txt");
        let mut printer = JSONBuilder::new().build(
            Some(path),
            LiteralMatcher::new(b"hello"),
            Vec::new(),
        );
        let mut searcher = Searcher::new();
        searcher
            .search_slice(&matcher, data, &mut printer)
            .unwrap();
        let output = String::from_utf8(printer.get_mut().clone()).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert!(lines.len() >= 3, "Expected at least 3 JSON lines, got: {}", lines.len());

        // First line should be "begin"
        let begin: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(begin["type"], "begin");

        // Second line should be "match"
        let mat: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(mat["type"], "match");

        // Last line should be "end"
        let end: serde_json::Value =
            serde_json::from_str(lines[lines.len() - 1]).unwrap();
        assert_eq!(end["type"], "end");
    }

    #[test]
    fn test_stats_add() {
        let mut s1 = Stats::new();
        s1.matched_lines = 5;
        s1.matches = 10;
        s1.files_with_matches = 2;
        s1.files_searched = 3;
        s1.bytes_searched = 1000;
        s1.bytes_printed = 500;

        let mut s2 = Stats::new();
        s2.matched_lines = 3;
        s2.matches = 6;
        s2.files_with_matches = 1;
        s2.files_searched = 2;
        s2.bytes_searched = 500;
        s2.bytes_printed = 200;

        s1.add(&s2);
        assert_eq!(s1.matched_lines(), 8);
        assert_eq!(s1.matches(), 16);
        assert_eq!(s1.files_with_matches(), 3);
        assert_eq!(s1.files_searched(), 5);
        assert_eq!(s1.bytes_searched(), 1500);
        assert_eq!(s1.bytes_printed(), 700);
    }

    #[test]
    fn test_color_specs_default() {
        let cs = ColorSpecs::default();
        assert_eq!(cs.path().fg(), Some(&Color::Magenta));
        assert_eq!(cs.line().fg(), Some(&Color::Green));
        assert_eq!(cs.column().fg(), Some(&Color::Green));
        assert_eq!(cs.matched().fg(), Some(&Color::Red));
        assert!(cs.matched().bold());
    }

    #[test]
    fn test_user_color_spec_parse() {
        let spec: UserColorSpec = "match:fg:blue".parse().unwrap();
        assert_eq!(spec.ty, "match");
        assert_eq!(spec.attr, "fg");
        assert_eq!(spec.value, "blue");
    }

    #[test]
    fn test_hyperlink_config_default() {
        let hc = HyperlinkConfig::default();
        assert!(!hc.is_enabled());
    }

    #[test]
    fn test_hyperlink_config_custom() {
        let hc = HyperlinkConfig::new("file://{path}#{line}");
        assert!(hc.is_enabled());
    }

    #[test]
    fn test_standard_with_stats() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"hello world\nhello again\n";
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new()
            .stats(true)
            .build(None, LiteralMatcher::new(b"hello"), buf);
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let stats = printer.stats().unwrap();
        assert_eq!(stats.matched_lines(), 2);
        assert_eq!(stats.files_searched(), 1);
        assert_eq!(stats.files_with_matches(), 1);
    }

    #[test]
    fn test_standard_only_matching() {
        let matcher = LiteralMatcher::new(b"hello");
        let data = b"say hello world\n";
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new()
            .only_matching(true)
            .build(None, LiteralMatcher::new(b"hello"), buf);
        let mut searcher = Searcher::new();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        // Only "hello" should appear, not the full line.
        assert_eq!(output.trim(), "1:hello");
    }

    #[test]
    fn test_standard_context_separator() {
        let matcher = LiteralMatcher::new(b"match");
        let data = b"match1\nskip1\nskip2\nmatch2\n";
        let buf = NoColor::new(Vec::new());
        let mut printer = StandardBuilder::new()
            .build(None, LiteralMatcher::new(b"match"), buf);
        let mut searcher = SearcherBuilder::new()
            .after_context(0)
            .before_context(0)
            .build();
        searcher.search_slice(&matcher, data, &mut printer).unwrap();
        let output = get_output(printer.get_mut().get_ref().clone());
        assert!(output.contains("match1"));
        assert!(output.contains("match2"));
    }
}
