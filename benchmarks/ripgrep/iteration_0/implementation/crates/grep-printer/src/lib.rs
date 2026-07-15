//! # grep-printer
//!
//! This crate provides output formatting for grep-like search tools. It
//! implements the [`grep_searcher::Sink`] trait to receive search results
//! and format them for display.
//!
//! Three printer types are provided:
//!
//! - [`Standard`]: Produces traditional grep-like output with optional colors,
//!   line numbers, column numbers, headings, and more.
//! - [`Summary`]: Produces summary output such as match counts or file lists.
//! - [`JSON`]: Produces machine-readable JSON Lines output.
//!
//! Each printer follows a builder pattern: use the corresponding builder
//! (e.g., [`StandardBuilder`]) to configure options, then call `build()` to
//! produce the printer. From the printer, call `sink()` or `sink_with_path()`
//! to create a [`grep_searcher::Sink`] implementation that can be passed to
//! a searcher.

#![deny(missing_docs)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use grep_matcher::{Matcher, Match};
use grep_searcher::{
    Searcher, Sink, SinkContext, SinkFinish, SinkMatch,
};
use serde::Serialize;
use termcolor::{Color, ColorSpec, WriteColor};

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Aggregate statistics about search results.
///
/// This is used by the JSON printer and can also be used by callers to
/// track overall search statistics across many files.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    /// Total number of individual matches found.
    pub matches: u64,
    /// Total number of lines containing at least one match.
    pub matched_lines: u64,
    /// Number of files that contained at least one match.
    pub files_with_matches: u64,
    /// Total number of files searched.
    pub files_searched: u64,
    /// Total bytes of output printed.
    pub bytes_printed: u64,
    /// Total bytes of input searched.
    pub bytes_searched: u64,
    /// Duration spent searching.
    #[serde(skip)]
    pub search_duration: Duration,
    /// Total elapsed duration.
    #[serde(skip)]
    pub total_duration: Duration,
}

impl Stats {
    /// Add another `Stats` into this one.
    pub fn add(&mut self, other: &Stats) {
        self.matches += other.matches;
        self.matched_lines += other.matched_lines;
        self.files_with_matches += other.files_with_matches;
        self.files_searched += other.files_searched;
        self.bytes_printed += other.bytes_printed;
        self.bytes_searched += other.bytes_searched;
        self.search_duration += other.search_duration;
        self.total_duration += other.total_duration;
    }
}

// ---------------------------------------------------------------------------
// Color types
// ---------------------------------------------------------------------------

/// The type of element to color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColorType {
    /// Path/filename color.
    Path,
    /// Line number color.
    Line,
    /// Column number color.
    Column,
    /// Match highlight color.
    Match,
}

/// The attribute of a color specification to set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColorAttribute {
    /// Foreground color.
    Fg,
    /// Background color.
    Bg,
    /// Style (bold, italic, underline, etc.).
    Style,
}

/// A color value that can be applied to a [`ColorSpec`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColorValue {
    /// Black.
    Black,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Red.
    Red,
    /// Cyan.
    Cyan,
    /// Magenta.
    Magenta,
    /// Yellow.
    Yellow,
    /// White.
    White,
    /// Bold style.
    Bold,
    /// Italic style.
    Italic,
    /// Underline style.
    Underline,
    /// An ANSI 256-color index.
    Ansi256(u8),
    /// An RGB color.
    Rgb(u8, u8, u8),
}

/// A user-specified color setting.
///
/// This represents a single directive like "match:fg:red" or "path:style:bold".
#[derive(Clone, Debug)]
pub struct UserColorSpec {
    /// Which element to color.
    pub ty: ColorType,
    /// Which attribute to set.
    pub attr: ColorAttribute,
    /// The color or style value.
    pub value: ColorValue,
}

/// Color specifications for the various output elements.
///
/// This groups together the [`ColorSpec`] for each element of grep output
/// that can be independently colored: the path, line number, column number,
/// and the matched text itself.
#[derive(Clone, Debug)]
pub struct ColorSpecs {
    /// Color spec for the file path.
    pub path: ColorSpec,
    /// Color spec for line numbers.
    pub line: ColorSpec,
    /// Color spec for column numbers.
    pub column: ColorSpec,
    /// Color spec for matched text.
    pub matched: ColorSpec,
}

impl Default for ColorSpecs {
    fn default() -> Self {
        ColorSpecs::default_with_color()
    }
}

impl ColorSpecs {
    /// Create a `ColorSpecs` from a list of user-specified color directives.
    ///
    /// Each [`UserColorSpec`] overrides the default color for the specified
    /// element and attribute. Later entries in the list take precedence over
    /// earlier ones for the same element/attribute pair.
    pub fn new(specs: &[UserColorSpec]) -> ColorSpecs {
        let mut colors = ColorSpecs::default_with_color();
        for spec in specs {
            let target = match spec.ty {
                ColorType::Path => &mut colors.path,
                ColorType::Line => &mut colors.line,
                ColorType::Column => &mut colors.column,
                ColorType::Match => &mut colors.matched,
            };
            match spec.attr {
                ColorAttribute::Fg => {
                    if let Some(c) = color_value_to_color(&spec.value) {
                        target.set_fg(Some(c));
                    }
                }
                ColorAttribute::Bg => {
                    if let Some(c) = color_value_to_color(&spec.value) {
                        target.set_bg(Some(c));
                    }
                }
                ColorAttribute::Style => {
                    match spec.value {
                        ColorValue::Bold => {
                            target.set_bold(true);
                        }
                        ColorValue::Italic => {
                            target.set_italic(true);
                        }
                        ColorValue::Underline => {
                            target.set_underline(true);
                        }
                        _ => {}
                    }
                }
            }
        }
        colors
    }

    /// Create the default color specs with color enabled.
    ///
    /// The defaults are:
    /// - **path**: magenta
    /// - **line**: green
    /// - **column**: green
    /// - **match**: bold red
    pub fn default_with_color() -> ColorSpecs {
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

/// Convert a `ColorValue` to a termcolor `Color`, if applicable.
fn color_value_to_color(value: &ColorValue) -> Option<Color> {
    match value {
        ColorValue::Black => Some(Color::Black),
        ColorValue::Blue => Some(Color::Blue),
        ColorValue::Green => Some(Color::Green),
        ColorValue::Red => Some(Color::Red),
        ColorValue::Cyan => Some(Color::Cyan),
        ColorValue::Magenta => Some(Color::Magenta),
        ColorValue::Yellow => Some(Color::Yellow),
        ColorValue::White => Some(Color::White),
        ColorValue::Ansi256(n) => Some(Color::Ansi256(*n)),
        ColorValue::Rgb(r, g, b) => Some(Color::Rgb(*r, *g, *b)),
        // Style values are not colors.
        ColorValue::Bold | ColorValue::Italic | ColorValue::Underline => None,
    }
}

// ---------------------------------------------------------------------------
// Standard printer
// ---------------------------------------------------------------------------

/// Configuration for the [`Standard`] printer.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct StandardConfig {
    colors: ColorSpecs,
    heading: bool,
    path: bool,
    line_number: bool,
    column: bool,
    byte_offset: bool,
    separator_field_match: Vec<u8>,
    separator_field_context: Vec<u8>,
    separator_context: Option<Vec<u8>>,
    trim: bool,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    replacement: Option<Vec<u8>>,
    per_match: bool,
    only_matching: bool,
    path_terminator: Option<u8>,
    null_data: bool,
    hyperlink_format: Option<String>,
}

impl Default for StandardConfig {
    fn default() -> Self {
        StandardConfig {
            colors: ColorSpecs::default(),
            heading: false,
            path: true,
            line_number: true,
            column: false,
            byte_offset: false,
            separator_field_match: b":".to_vec(),
            separator_field_context: b"-".to_vec(),
            separator_context: Some(b"--".to_vec()),
            trim: false,
            max_columns: None,
            max_columns_preview: false,
            replacement: None,
            per_match: false,
            only_matching: false,
            path_terminator: None,
            null_data: false,
            hyperlink_format: None,
        }
    }
}

/// A builder for configuring the [`Standard`] printer.
///
/// Use [`StandardBuilder::new()`] to create a builder with default settings,
/// then chain configuration methods and call [`build()`](StandardBuilder::build)
/// to produce a [`Standard`] printer.
///
/// # Example
///
/// ```no_run
/// use grep_printer::StandardBuilder;
///
/// let mut builder = StandardBuilder::new();
/// builder.heading(true).line_number(true).column(true);
/// let stdout = termcolor::StandardStream::stdout(termcolor::ColorChoice::Auto);
/// let printer = builder.build(stdout);
/// ```
#[derive(Clone, Debug)]
pub struct StandardBuilder {
    config: StandardConfig,
}

impl Default for StandardBuilder {
    fn default() -> Self {
        StandardBuilder::new()
    }
}

impl StandardBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        StandardBuilder {
            config: StandardConfig::default(),
        }
    }

    /// Build a [`Standard`] printer that writes to the given writer.
    pub fn build<W: Write + WriteColor>(&self, wtr: W) -> Standard<W> {
        Standard {
            wtr,
            config: self.config.clone(),
        }
    }

    /// Enable or disable heading mode.
    ///
    /// In heading mode, the file path is printed once above the results for
    /// that file, rather than on each result line.
    pub fn heading(&mut self, yes: bool) -> &mut Self {
        self.config.heading = yes;
        self
    }

    /// Enable or disable path printing.
    pub fn path(&mut self, yes: bool) -> &mut Self {
        self.config.path = yes;
        self
    }

    /// Enable or disable line number printing.
    pub fn line_number(&mut self, yes: bool) -> &mut Self {
        self.config.line_number = yes;
        self
    }

    /// Enable or disable column number printing.
    pub fn column(&mut self, yes: bool) -> &mut Self {
        self.config.column = yes;
        self
    }

    /// Enable or disable byte offset printing.
    pub fn byte_offset(&mut self, yes: bool) -> &mut Self {
        self.config.byte_offset = yes;
        self
    }

    /// Set the field separator for match lines (default: `:`).
    pub fn separator_field_match(&mut self, sep: &[u8]) -> &mut Self {
        self.config.separator_field_match = sep.to_vec();
        self
    }

    /// Set the field separator for context lines (default: `-`).
    pub fn separator_field_context(&mut self, sep: &[u8]) -> &mut Self {
        self.config.separator_field_context = sep.to_vec();
        self
    }

    /// Set the context separator printed between context groups (default: `--`).
    ///
    /// Set to `None` to disable the context separator.
    pub fn separator_context(&mut self, sep: Option<&[u8]>) -> &mut Self {
        self.config.separator_context = sep.map(|s| s.to_vec());
        self
    }

    /// Enable or disable leading whitespace trimming.
    pub fn trim(&mut self, yes: bool) -> &mut Self {
        self.config.trim = yes;
        self
    }

    /// Set the maximum number of columns to print per line.
    ///
    /// Lines longer than this limit are either truncated or elided,
    /// depending on [`max_columns_preview`](StandardBuilder::max_columns_preview).
    pub fn max_columns(&mut self, limit: Option<u64>) -> &mut Self {
        self.config.max_columns = limit;
        self
    }

    /// If enabled, show a preview of truncated lines rather than replacing
    /// them with a message.
    pub fn max_columns_preview(&mut self, yes: bool) -> &mut Self {
        self.config.max_columns_preview = yes;
        self
    }

    /// Set the replacement text for matches.
    ///
    /// When set, matched text is replaced with this string in the output.
    pub fn replacement(&mut self, replacement: Option<&[u8]>) -> &mut Self {
        self.config.replacement = replacement.map(|r| r.to_vec());
        self
    }

    /// Enable per-match mode (one output line per match, as in `--vimgrep`).
    pub fn per_match(&mut self, yes: bool) -> &mut Self {
        self.config.per_match = yes;
        self
    }

    /// Enable only-matching mode (print only the matched portion of each line).
    pub fn only_matching(&mut self, yes: bool) -> &mut Self {
        self.config.only_matching = yes;
        self
    }

    /// Set the color specs for this printer.
    pub fn colors(&mut self, colors: ColorSpecs) -> &mut Self {
        self.config.colors = colors;
        self
    }

    /// Set the path terminator byte (e.g., `Some(b'\0')` for `-0`/`--null`).
    pub fn path_terminator(&mut self, term: Option<u8>) -> &mut Self {
        self.config.path_terminator = term;
        self
    }

    /// Set the hyperlink format.
    pub fn hyperlink_format(&mut self, format: Option<String>) -> &mut Self {
        self.config.hyperlink_format = format;
        self
    }
}

/// A standard grep-like printer.
///
/// This printer produces output in the traditional grep format:
///
/// ```text
/// path/to/file:42:matched line content
/// ```
///
/// With heading mode enabled:
///
/// ```text
/// path/to/file
/// 42:matched line content
/// ```
///
/// Use [`StandardBuilder`] to configure the printer, and then call
/// [`sink()`](Standard::sink) or [`sink_with_path()`](Standard::sink_with_path)
/// to create a [`Sink`] implementation that can be passed to a searcher.
pub struct Standard<W> {
    wtr: W,
    config: StandardConfig,
}

impl<W: Write + WriteColor> Standard<W> {
    /// Create a sink without a file path.
    ///
    /// This is useful when searching stdin or a single source where the
    /// file path is not meaningful.
    pub fn sink<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
    ) -> StandardSink<'p, 's, M, W> {
        StandardSink {
            matcher,
            standard: self,
            path: None,
            has_printed: false,
            needs_separator: false,
            match_count: 0,
            after_context_remaining: 0,
        }
    }

    /// Create a sink associated with a file path.
    ///
    /// The path will be printed as part of the output (subject to
    /// configuration).
    pub fn sink_with_path<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
        path: &Path,
    ) -> StandardSink<'p, 's, M, W> {
        StandardSink {
            matcher,
            standard: self,
            path: Some(path.to_path_buf()),
            has_printed: false,
            needs_separator: false,
            match_count: 0,
            after_context_remaining: 0,
        }
    }

    /// Get a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }
}

/// A [`Sink`] implementation for the [`Standard`] printer.
///
/// This is created by calling [`Standard::sink()`] or
/// [`Standard::sink_with_path()`]. It implements [`grep_searcher::Sink`]
/// to receive match and context events from a searcher.
#[allow(dead_code)]
pub struct StandardSink<'p, 's, M: Matcher, W: Write + WriteColor> {
    matcher: &'s M,
    standard: &'p mut Standard<W>,
    path: Option<PathBuf>,
    has_printed: bool,
    needs_separator: bool,
    match_count: u64,
    after_context_remaining: usize,
}

impl<'p, 's, M: Matcher, W: Write + WriteColor> StandardSink<'p, 's, M, W> {
    /// Returns the total number of matches found so far.
    pub fn match_count(&self) -> u64 {
        self.match_count
    }

    /// Returns whether any output has been printed.
    pub fn has_printed(&self) -> bool {
        self.has_printed
    }

    /// Write the path to the writer with coloring.
    fn write_path(&mut self) -> io::Result<()> {
        if let Some(ref path) = self.path {
            let path_bytes = path_to_bytes(path);
            self.standard
                .wtr
                .set_color(&self.standard.config.colors.path)?;
            self.standard.wtr.write_all(&path_bytes)?;
            self.standard.wtr.reset()?;
        }
        Ok(())
    }

    /// Write a separator byte after the path.
    fn write_path_separator(&mut self, is_match: bool) -> io::Result<()> {
        if self.standard.config.heading {
            // In heading mode, no separator after path on data lines
            return Ok(());
        }
        let sep = if is_match {
            &self.standard.config.separator_field_match
        } else {
            &self.standard.config.separator_field_context
        };
        self.standard.wtr.write_all(sep)?;
        Ok(())
    }

    /// Write a line number with coloring.
    fn write_line_number(
        &mut self,
        line_number: Option<u64>,
        is_match: bool,
    ) -> io::Result<()> {
        if !self.standard.config.line_number {
            return Ok(());
        }
        if let Some(n) = line_number {
            self.standard
                .wtr
                .set_color(&self.standard.config.colors.line)?;
            write!(self.standard.wtr, "{}", n)?;
            self.standard.wtr.reset()?;
            let sep = if is_match {
                &self.standard.config.separator_field_match
            } else {
                &self.standard.config.separator_field_context
            };
            self.standard.wtr.write_all(sep)?;
        }
        Ok(())
    }

    /// Write a column number with coloring.
    fn write_column(
        &mut self,
        column: Option<u64>,
        is_match: bool,
    ) -> io::Result<()> {
        if !self.standard.config.column {
            return Ok(());
        }
        if let Some(c) = column {
            self.standard
                .wtr
                .set_color(&self.standard.config.colors.column)?;
            write!(self.standard.wtr, "{}", c)?;
            self.standard.wtr.reset()?;
            let sep = if is_match {
                &self.standard.config.separator_field_match
            } else {
                &self.standard.config.separator_field_context
            };
            self.standard.wtr.write_all(sep)?;
        }
        Ok(())
    }

    /// Write a byte offset.
    fn write_byte_offset(
        &mut self,
        offset: u64,
        is_match: bool,
    ) -> io::Result<()> {
        if !self.standard.config.byte_offset {
            return Ok(());
        }
        write!(self.standard.wtr, "{}", offset)?;
        let sep = if is_match {
            &self.standard.config.separator_field_match
        } else {
            &self.standard.config.separator_field_context
        };
        self.standard.wtr.write_all(sep)?;
        Ok(())
    }

    /// Write the preamble fields (path, line number, column, byte offset)
    /// for a line of output.
    fn write_preamble(
        &mut self,
        line_number: Option<u64>,
        column: Option<u64>,
        byte_offset: u64,
        is_match: bool,
    ) -> io::Result<()> {
        if self.standard.config.path && !self.standard.config.heading {
            self.write_path()?;
            self.write_path_separator(is_match)?;
        }
        self.write_line_number(line_number, is_match)?;
        self.write_column(column, is_match)?;
        self.write_byte_offset(byte_offset, is_match)?;
        Ok(())
    }

    /// Trim leading whitespace from bytes if trimming is enabled.
    fn maybe_trim<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        if self.standard.config.trim {
            trim_leading_whitespace(bytes)
        } else {
            bytes
        }
    }

    /// Find all matches in the given line bytes and return them.
    fn find_matches(&self, line: &[u8]) -> Vec<Match> {
        let mut matches = Vec::new();
        // Strip trailing newline for matching purposes.
        let search_bytes = strip_line_terminator(line);
        let _ = self.matcher.find_iter(search_bytes, |m| {
            matches.push(m);
            true
        });
        matches
    }

    /// Write line content with match highlighting.
    fn write_highlighted_line(&mut self, line: &[u8]) -> io::Result<()> {
        let search_bytes = strip_line_terminator(line);
        let matches = self.find_matches(line);

        if matches.is_empty() {
            // No matches found (shouldn't happen for a match line, but be safe)
            self.standard.wtr.write_all(line)?;
            return Ok(());
        }

        let mut last_end = 0;
        for m in &matches {
            // Write text before the match
            if m.start() > last_end {
                self.standard
                    .wtr
                    .write_all(&search_bytes[last_end..m.start()])?;
            }
            // Write the match with color
            self.standard
                .wtr
                .set_color(&self.standard.config.colors.matched)?;
            self.standard
                .wtr
                .write_all(&search_bytes[m.start()..m.end()])?;
            self.standard.wtr.reset()?;
            last_end = m.end();
        }
        // Write remaining text after last match
        if last_end < search_bytes.len() {
            self.standard
                .wtr
                .write_all(&search_bytes[last_end..])?;
        }
        // Write the line terminator
        if line.len() > search_bytes.len() {
            self.standard
                .wtr
                .write_all(&line[search_bytes.len()..])?;
        }
        Ok(())
    }

    /// Write a single match line in per_match or only_matching mode.
    fn write_single_match(
        &mut self,
        line_number: Option<u64>,
        byte_offset: u64,
        m: &Match,
        line: &[u8],
    ) -> io::Result<()> {
        let search_bytes = strip_line_terminator(line);
        let col = Some(m.start() as u64 + 1);
        self.write_preamble(line_number, col, byte_offset, true)?;

        // Write just the matched text
        self.standard
            .wtr
            .set_color(&self.standard.config.colors.matched)?;
        self.standard
            .wtr
            .write_all(&search_bytes[m.start()..m.end()])?;
        self.standard.wtr.reset()?;
        self.standard.wtr.write_all(b"\n")?;
        Ok(())
    }

    /// Write a match line with replacement applied.
    fn write_replacement_line(
        &mut self,
        line_number: Option<u64>,
        byte_offset: u64,
        line: &[u8],
        replacement: &[u8],
    ) -> io::Result<()> {
        let search_bytes = strip_line_terminator(line);
        let matches = self.find_matches(line);
        let column = matches.first().map(|m| m.start() as u64 + 1);

        self.write_preamble(line_number, column, byte_offset, true)?;

        // Build the replaced line
        let mut last_end = 0;
        for m in &matches {
            if m.start() > last_end {
                self.standard
                    .wtr
                    .write_all(&search_bytes[last_end..m.start()])?;
            }
            self.standard
                .wtr
                .set_color(&self.standard.config.colors.matched)?;
            self.standard.wtr.write_all(replacement)?;
            self.standard.wtr.reset()?;
            last_end = m.end();
        }
        if last_end < search_bytes.len() {
            self.standard
                .wtr
                .write_all(&search_bytes[last_end..])?;
        }
        // Write line terminator
        if line.len() > search_bytes.len() {
            self.standard
                .wtr
                .write_all(&line[search_bytes.len()..])?;
        } else {
            self.standard.wtr.write_all(b"\n")?;
        }
        Ok(())
    }

    /// Write a line with max_columns handling.
    fn write_with_max_columns(
        &mut self,
        line_number: Option<u64>,
        column: Option<u64>,
        byte_offset: u64,
        line: &[u8],
        is_match: bool,
    ) -> io::Result<()> {
        let max = match self.standard.config.max_columns {
            Some(m) => m as usize,
            None => {
                // No limit, write normally
                self.write_preamble(line_number, column, byte_offset, is_match)?;
                if is_match {
                    self.write_highlighted_line(line)?;
                } else {
                    self.standard.wtr.write_all(line)?;
                }
                return Ok(());
            }
        };

        let trimmed_line = strip_line_terminator(line);
        let char_count = trimmed_line.len();

        if char_count <= max {
            // Line fits, write normally
            self.write_preamble(line_number, column, byte_offset, is_match)?;
            if is_match {
                self.write_highlighted_line(line)?;
            } else {
                self.standard.wtr.write_all(line)?;
            }
        } else if self.standard.config.max_columns_preview {
            // Show a truncated preview
            self.write_preamble(line_number, column, byte_offset, is_match)?;
            let preview = &trimmed_line[..max];
            if is_match {
                // Try to highlight the preview portion
                let mut matches = Vec::new();
                let _ = self.matcher.find_iter(preview, |m| {
                    matches.push(m);
                    true
                });
                let mut last_end = 0;
                for m in &matches {
                    if m.start() > last_end {
                        self.standard
                            .wtr
                            .write_all(&preview[last_end..m.start()])?;
                    }
                    self.standard
                        .wtr
                        .set_color(&self.standard.config.colors.matched)?;
                    self.standard
                        .wtr
                        .write_all(&preview[m.start()..m.end()])?;
                    self.standard.wtr.reset()?;
                    last_end = m.end();
                }
                if last_end < preview.len() {
                    self.standard
                        .wtr
                        .write_all(&preview[last_end..])?;
                }
            } else {
                self.standard.wtr.write_all(preview)?;
            }
            write!(
                self.standard.wtr,
                " [... {} more bytes]\n",
                char_count - max
            )?;
        } else {
            // Replace with a message
            self.write_preamble(line_number, column, byte_offset, is_match)?;
            write!(
                self.standard.wtr,
                "[Omitted long line with {} bytes]\n",
                char_count
            )?;
        }
        Ok(())
    }
}

impl<M: Matcher, W: Write + WriteColor> Sink for StandardSink<'_, '_, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        // Write context separator if needed (between result groups)
        if self.needs_separator {
            if let Some(ref sep) = self.standard.config.separator_context {
                self.standard.wtr.write_all(sep)?;
                self.standard.wtr.write_all(b"\n")?;
            }
            self.needs_separator = false;
        }

        let line = mat.bytes();
        let line_number = mat.line_number();
        let byte_offset = mat.absolute_byte_offset();

        self.match_count += 1;
        self.has_printed = true;

        // Handle replacement mode
        if let Some(ref replacement) = self.standard.config.replacement.clone() {
            self.write_replacement_line(
                line_number,
                byte_offset,
                line,
                replacement,
            )?;
            return Ok(true);
        }

        // Handle only-matching mode
        if self.standard.config.only_matching {
            let matches = self.find_matches(line);
            for m in &matches {
                self.write_single_match(line_number, byte_offset, m, line)?;
            }
            return Ok(true);
        }

        // Handle per-match mode (--vimgrep)
        if self.standard.config.per_match {
            let matches = self.find_matches(line);
            for m in &matches {
                self.write_single_match(line_number, byte_offset, m, line)?;
            }
            return Ok(true);
        }

        // Normal mode: find the first match column
        let matches = self.find_matches(line);
        let column = if self.standard.config.column {
            matches.first().map(|m| m.start() as u64 + 1)
        } else {
            None
        };

        let line = self.maybe_trim(line);

        // Handle max_columns
        self.write_with_max_columns(
            line_number,
            column,
            byte_offset,
            line,
            true,
        )?;

        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        // Write context separator if needed
        if self.needs_separator {
            if let Some(ref sep) = self.standard.config.separator_context {
                self.standard.wtr.write_all(sep)?;
                self.standard.wtr.write_all(b"\n")?;
            }
            self.needs_separator = false;
        }

        let line = ctx.bytes();
        let line_number = ctx.line_number();
        let byte_offset = ctx.absolute_byte_offset();

        let line = self.maybe_trim(line);

        self.has_printed = true;

        self.write_with_max_columns(
            line_number,
            None,
            byte_offset,
            line,
            false,
        )?;

        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        self.needs_separator = true;
        Ok(true)
    }

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        if self.standard.config.heading && self.standard.config.path {
            if self.path.is_some() {
                self.write_path()?;
                let term = self
                    .standard
                    .config
                    .path_terminator
                    .unwrap_or(b'\n');
                self.standard.wtr.write_all(&[term])?;
            }
        }
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        if let Some(offset) = finish.binary_byte_offset {
            // Print binary file message
            if self.standard.config.path {
                if let Some(ref path) = self.path {
                    write!(
                        self.standard.wtr,
                        "Binary file {} matches (found at byte offset {})\n",
                        path.display(),
                        offset
                    )?;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Summary printer
// ---------------------------------------------------------------------------

/// The kind of summary output to produce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SummaryKind {
    /// Print the count of matching lines per file.
    Count,
    /// Print the count of individual matches per file.
    CountMatches,
    /// Print filenames that have at least one match.
    FilesWithMatches,
    /// Print filenames that have no matches.
    FilesWithoutMatch,
    /// Produce no output; only track whether any match was found.
    Quiet,
}

/// Configuration for the [`Summary`] printer.
#[derive(Clone, Debug)]
struct SummaryConfig {
    kind: SummaryKind,
    colors: ColorSpecs,
    path: bool,
    separator_field: Vec<u8>,
    path_terminator: Option<u8>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        SummaryConfig {
            kind: SummaryKind::Count,
            colors: ColorSpecs::default(),
            path: true,
            separator_field: b":".to_vec(),
            path_terminator: None,
        }
    }
}

/// A builder for configuring the [`Summary`] printer.
///
/// # Example
///
/// ```no_run
/// use grep_printer::{SummaryBuilder, SummaryKind};
///
/// let mut builder = SummaryBuilder::new();
/// builder.kind(SummaryKind::FilesWithMatches);
/// let stdout = termcolor::StandardStream::stdout(termcolor::ColorChoice::Auto);
/// let printer = builder.build(stdout);
/// ```
#[derive(Clone, Debug)]
pub struct SummaryBuilder {
    config: SummaryConfig,
}

impl Default for SummaryBuilder {
    fn default() -> Self {
        SummaryBuilder::new()
    }
}

impl SummaryBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        SummaryBuilder {
            config: SummaryConfig::default(),
        }
    }

    /// Build a [`Summary`] printer that writes to the given writer.
    pub fn build<W: Write + WriteColor>(&self, wtr: W) -> Summary<W> {
        Summary {
            wtr,
            config: self.config.clone(),
        }
    }

    /// Set the kind of summary to produce.
    pub fn kind(&mut self, kind: SummaryKind) -> &mut Self {
        self.config.kind = kind;
        self
    }

    /// Set the color specs for this printer.
    pub fn colors(&mut self, colors: ColorSpecs) -> &mut Self {
        self.config.colors = colors;
        self
    }

    /// Enable or disable path printing.
    pub fn path(&mut self, yes: bool) -> &mut Self {
        self.config.path = yes;
        self
    }

    /// Set the field separator (default: `:`).
    pub fn separator_field(&mut self, sep: &[u8]) -> &mut Self {
        self.config.separator_field = sep.to_vec();
        self
    }

    /// Set the path terminator byte (e.g., `Some(b'\0')` for `-0`/`--null`).
    pub fn path_terminator(&mut self, term: Option<u8>) -> &mut Self {
        self.config.path_terminator = term;
        self
    }
}

/// A summary printer.
///
/// This printer produces summary output for search results:
/// - **Count**: prints `<path>:<count>` for matching lines.
/// - **CountMatches**: prints `<path>:<count>` for individual matches.
/// - **FilesWithMatches**: prints filenames that contain matches.
/// - **FilesWithoutMatch**: prints filenames that contain no matches.
/// - **Quiet**: produces no output, just tracks match state.
pub struct Summary<W> {
    wtr: W,
    config: SummaryConfig,
}

impl<W: Write + WriteColor> Summary<W> {
    /// Create a sink without a file path.
    pub fn sink<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
    ) -> SummarySink<'p, 's, M, W> {
        SummarySink {
            matcher,
            summary: self,
            path: None,
            match_count: 0,
            line_count: 0,
            binary_byte_offset: None,
        }
    }

    /// Create a sink associated with a file path.
    pub fn sink_with_path<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
        path: &Path,
    ) -> SummarySink<'p, 's, M, W> {
        SummarySink {
            matcher,
            summary: self,
            path: Some(path.to_path_buf()),
            match_count: 0,
            line_count: 0,
            binary_byte_offset: None,
        }
    }

    /// Get a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }
}

/// A [`Sink`] implementation for the [`Summary`] printer.
pub struct SummarySink<'p, 's, M: Matcher, W: Write + WriteColor> {
    matcher: &'s M,
    summary: &'p mut Summary<W>,
    path: Option<PathBuf>,
    match_count: u64,
    line_count: u64,
    binary_byte_offset: Option<u64>,
}

impl<M: Matcher, W: Write + WriteColor> SummarySink<'_, '_, M, W> {
    /// Returns the total number of individual matches found.
    pub fn match_count(&self) -> u64 {
        self.match_count
    }

    /// Returns the total number of matching lines found.
    pub fn line_count(&self) -> u64 {
        self.line_count
    }

    /// Write the path to the writer with coloring.
    fn write_path(&mut self) -> io::Result<()> {
        if let Some(ref path) = self.path {
            let path_bytes = path_to_bytes(path);
            self.summary
                .wtr
                .set_color(&self.summary.config.colors.path)?;
            self.summary.wtr.write_all(&path_bytes)?;
            self.summary.wtr.reset()?;
        }
        Ok(())
    }

    /// Write the path terminator.
    fn write_path_terminator(&mut self) -> io::Result<()> {
        let term = self
            .summary
            .config
            .path_terminator
            .unwrap_or(b'\n');
        self.summary.wtr.write_all(&[term])?;
        Ok(())
    }
}

impl<M: Matcher, W: Write + WriteColor> Sink for SummarySink<'_, '_, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.line_count += 1;

        // Count individual matches in this line
        let line = strip_line_terminator(mat.bytes());
        let mut count: u64 = 0;
        let _ = self.matcher.find_iter(line, |_m| {
            count += 1;
            true
        });
        // At minimum, we found one match (we got called)
        if count == 0 {
            count = 1;
        }
        self.match_count += count;

        // For FilesWithMatches and Quiet, we can stop after the first match
        match self.summary.config.kind {
            SummaryKind::FilesWithMatches => Ok(false),
            SummaryKind::Quiet => Ok(false),
            _ => Ok(true),
        }
    }

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        self.binary_byte_offset = finish.binary_byte_offset;

        match self.summary.config.kind {
            SummaryKind::Count => {
                if self.summary.config.path {
                    self.write_path()?;
                    self.summary.wtr.write_all(&self.summary.config.separator_field)?;
                }
                write!(self.summary.wtr, "{}", self.line_count)?;
                self.summary.wtr.write_all(b"\n")?;
            }
            SummaryKind::CountMatches => {
                if self.summary.config.path {
                    self.write_path()?;
                    self.summary.wtr.write_all(&self.summary.config.separator_field)?;
                }
                write!(self.summary.wtr, "{}", self.match_count)?;
                self.summary.wtr.write_all(b"\n")?;
            }
            SummaryKind::FilesWithMatches => {
                if self.match_count > 0 {
                    if self.summary.config.path {
                        self.write_path()?;
                        self.write_path_terminator()?;
                    }
                }
            }
            SummaryKind::FilesWithoutMatch => {
                if self.match_count == 0 {
                    if self.summary.config.path {
                        self.write_path()?;
                        self.write_path_terminator()?;
                    }
                }
            }
            SummaryKind::Quiet => {
                // No output
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// JSON printer
// ---------------------------------------------------------------------------

/// A builder for configuring the [`JSON`] printer.
///
/// # Example
///
/// ```no_run
/// use grep_printer::JSONBuilder;
///
/// let builder = JSONBuilder::new();
/// let stdout = std::io::stdout();
/// let printer = builder.build(stdout);
/// ```
#[derive(Clone, Debug, Default)]
pub struct JSONBuilder {}

impl JSONBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        JSONBuilder {}
    }

    /// Build a [`JSON`] printer that writes to the given writer.
    pub fn build<W: Write>(&self, wtr: W) -> JSON<W> {
        JSON { wtr }
    }
}

/// A JSON Lines printer.
///
/// This printer outputs one JSON object per line, following a schema with
/// message types `begin`, `match`, `context`, `end`, and `summary`.
///
/// This is useful for machine-readable output that can be parsed by other
/// tools.
pub struct JSON<W> {
    wtr: W,
}

impl<W: Write> JSON<W> {
    /// Create a sink without a file path.
    pub fn sink<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
    ) -> JSONSink<'p, 's, M, W> {
        JSONSink {
            matcher,
            json: self,
            path: None,
            match_count: 0,
            matched_line_count: 0,
            byte_count: 0,
            bytes_printed: 0,
        }
    }

    /// Create a sink associated with a file path.
    pub fn sink_with_path<'p, 's, M: Matcher>(
        &'p mut self,
        matcher: &'s M,
        path: &Path,
    ) -> JSONSink<'p, 's, M, W> {
        JSONSink {
            matcher,
            json: self,
            path: Some(path.to_path_buf()),
            match_count: 0,
            matched_line_count: 0,
            byte_count: 0,
            bytes_printed: 0,
        }
    }

    /// Get a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }
}

/// A [`Sink`] implementation for the [`JSON`] printer.
pub struct JSONSink<'p, 's, M: Matcher, W: Write> {
    matcher: &'s M,
    json: &'p mut JSON<W>,
    path: Option<PathBuf>,
    match_count: u64,
    matched_line_count: u64,
    byte_count: u64,
    bytes_printed: u64,
}

impl<M: Matcher, W: Write> JSONSink<'_, '_, M, W> {
    /// Returns the total number of individual matches found.
    pub fn match_count(&self) -> u64 {
        self.match_count
    }

    /// Write a JSON message and track bytes written.
    fn write_message(&mut self, msg: &JSONMessage<'_>) -> io::Result<()> {
        let serialized =
            serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.json.wtr.write_all(&serialized)?;
        self.json.wtr.write_all(b"\n")?;
        self.bytes_printed += serialized.len() as u64 + 1;
        Ok(())
    }

    /// Get the path as a JSON path value.
    fn path_data(&self) -> JSONPathData {
        match self.path {
            Some(ref p) => JSONPathData {
                text: p.to_string_lossy().into_owned(),
            },
            None => JSONPathData {
                text: String::new(),
            },
        }
    }

    /// Find submatches in a line.
    fn find_submatches(&self, line: &[u8]) -> Vec<JSONSubmatch> {
        let mut submatches = Vec::new();
        let search_bytes = strip_line_terminator(line);
        let _ = self.matcher.find_iter(search_bytes, |m| {
            let matched_bytes = &search_bytes[m.start()..m.end()];
            submatches.push(JSONSubmatch {
                match_val: JSONTextData {
                    text: String::from_utf8_lossy(matched_bytes).into_owned(),
                },
                start: m.start() as u64,
                end: m.end() as u64,
            });
            true
        });
        submatches
    }
}

// --- JSON serialization types ---

/// A JSON message (one line of output).
#[derive(Serialize)]
struct JSONMessage<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    data: JSONData<'a>,
}

/// The data payload of a JSON message.
#[derive(Serialize)]
#[serde(untagged)]
enum JSONData<'a> {
    /// Begin message data.
    Begin {
        /// The file path.
        path: &'a JSONPathData,
    },
    /// Match message data.
    Match {
        /// The file path.
        path: &'a JSONPathData,
        /// The full matching line.
        lines: JSONTextData,
        /// The line number, if available.
        line_number: Option<u64>,
        /// The absolute byte offset.
        absolute_offset: u64,
        /// The submatches within the line.
        submatches: Vec<JSONSubmatch>,
    },
    /// Context message data.
    Context {
        /// The file path.
        path: &'a JSONPathData,
        /// The context line.
        lines: JSONTextData,
        /// The line number, if available.
        line_number: Option<u64>,
        /// The absolute byte offset.
        absolute_offset: u64,
    },
    /// End message data.
    End {
        /// The file path.
        path: &'a JSONPathData,
        /// Search statistics for this file.
        stats: JSONEndStats,
    },
}

/// Path data in JSON output.
#[derive(Serialize)]
struct JSONPathData {
    /// The path as a UTF-8 string.
    text: String,
}

/// Text data in JSON output.
#[derive(Serialize)]
struct JSONTextData {
    /// The text content.
    text: String,
}

/// A submatch within a line.
#[derive(Serialize)]
struct JSONSubmatch {
    /// The matched text.
    #[serde(rename = "match")]
    match_val: JSONTextData,
    /// Start byte offset within the line.
    start: u64,
    /// End byte offset within the line.
    end: u64,
}

/// Statistics included in the `end` message.
#[derive(Serialize)]
struct JSONEndStats {
    /// Number of individual matches.
    matches: u64,
    /// Number of matching lines.
    matched_lines: u64,
    /// Bytes searched.
    bytes_searched: u64,
    /// Bytes printed.
    bytes_printed: u64,
}

impl<M: Matcher, W: Write> Sink for JSONSink<'_, '_, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.matched_line_count += 1;

        let line = mat.bytes();
        let submatches = self.find_submatches(line);
        self.match_count += submatches.len().max(1) as u64;

        let path_data = self.path_data();
        let line_text = String::from_utf8_lossy(strip_line_terminator(line)).into_owned();

        let msg = JSONMessage {
            msg_type: "match",
            data: JSONData::Match {
                path: &path_data,
                lines: JSONTextData { text: line_text },
                line_number: mat.line_number(),
                absolute_offset: mat.absolute_byte_offset(),
                submatches,
            },
        };
        self.write_message(&msg)?;
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        let line = ctx.bytes();
        let path_data = self.path_data();
        let line_text = String::from_utf8_lossy(strip_line_terminator(line)).into_owned();

        let msg = JSONMessage {
            msg_type: "context",
            data: JSONData::Context {
                path: &path_data,
                lines: JSONTextData { text: line_text },
                line_number: ctx.line_number(),
                absolute_offset: ctx.absolute_byte_offset(),
            },
        };
        self.write_message(&msg)?;
        Ok(true)
    }

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        let path_data = self.path_data();
        let msg = JSONMessage {
            msg_type: "begin",
            data: JSONData::Begin { path: &path_data },
        };
        self.write_message(&msg)?;
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        self.byte_count = finish.byte_count;

        let path_data = self.path_data();
        let msg = JSONMessage {
            msg_type: "end",
            data: JSONData::End {
                path: &path_data,
                stats: JSONEndStats {
                    matches: self.match_count,
                    matched_lines: self.matched_line_count,
                    bytes_searched: self.byte_count,
                    bytes_printed: self.bytes_printed,
                },
            },
        };
        self.write_message(&msg)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Convert a path to bytes for display.
///
/// On Unix, this is just the raw bytes. On other platforms, we use
/// `to_string_lossy()`.
fn path_to_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned().into_bytes()
    }
}

/// Strip a trailing line terminator (LF or CRLF) from a byte slice.
fn strip_line_terminator(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    &bytes[..end]
}

/// Trim leading ASCII whitespace from a byte slice.
fn trim_leading_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < bytes.len()
        && (bytes[start] == b' ' || bytes[start] == b'\t')
    {
        start += 1;
    }
    &bytes[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Match as MatcherMatch, NoCaptures, NoError};

    // A simple substring matcher for testing.
    struct SubstringMatcher {
        pattern: Vec<u8>,
    }

    impl SubstringMatcher {
        fn new(pattern: &[u8]) -> Self {
            SubstringMatcher {
                pattern: pattern.to_vec(),
            }
        }
    }

    impl Matcher for SubstringMatcher {
        type Captures = NoCaptures;
        type Error = NoError;

        fn find_at(
            &self,
            haystack: &[u8],
            at: usize,
        ) -> Result<Option<MatcherMatch>, NoError> {
            if at > haystack.len() {
                return Ok(None);
            }
            let haystack_slice = &haystack[at..];
            if self.pattern.is_empty() {
                return Ok(None);
            }
            if let Some(pos) = haystack_slice
                .windows(self.pattern.len())
                .position(|w| w == self.pattern.as_slice())
            {
                Ok(Some(MatcherMatch::new(at + pos, at + pos + self.pattern.len())))
            } else {
                Ok(None)
            }
        }

        fn new_captures(&self) -> Result<NoCaptures, NoError> {
            Ok(NoCaptures::new())
        }
    }

    // Helper: create a NoColor buffer writer for testing.
    fn no_color_buffer() -> termcolor::NoColor<Vec<u8>> {
        termcolor::NoColor::new(Vec::new())
    }

    #[test]
    fn test_strip_line_terminator() {
        assert_eq!(strip_line_terminator(b"hello\n"), b"hello");
        assert_eq!(strip_line_terminator(b"hello\r\n"), b"hello");
        assert_eq!(strip_line_terminator(b"hello"), b"hello");
        assert_eq!(strip_line_terminator(b""), b"");
        assert_eq!(strip_line_terminator(b"\n"), b"");
    }

    #[test]
    fn test_trim_leading_whitespace() {
        assert_eq!(trim_leading_whitespace(b"  hello"), b"hello");
        assert_eq!(trim_leading_whitespace(b"\thello"), b"hello");
        assert_eq!(trim_leading_whitespace(b"hello"), b"hello");
        assert_eq!(trim_leading_whitespace(b""), b"");
    }

    #[test]
    fn test_color_specs_default() {
        let specs = ColorSpecs::default();
        assert_eq!(specs.path.fg(), Some(&Color::Magenta));
        assert_eq!(specs.line.fg(), Some(&Color::Green));
        assert_eq!(specs.column.fg(), Some(&Color::Green));
        assert_eq!(specs.matched.fg(), Some(&Color::Red));
        assert!(specs.matched.bold());
    }

    #[test]
    fn test_color_specs_custom() {
        let user_specs = vec![
            UserColorSpec {
                ty: ColorType::Path,
                attr: ColorAttribute::Fg,
                value: ColorValue::Blue,
            },
            UserColorSpec {
                ty: ColorType::Match,
                attr: ColorAttribute::Style,
                value: ColorValue::Underline,
            },
        ];
        let specs = ColorSpecs::new(&user_specs);
        assert_eq!(specs.path.fg(), Some(&Color::Blue));
        assert!(specs.matched.underline());
    }

    #[test]
    fn test_standard_basic_match() {
        let wtr = no_color_buffer();
        let mut printer = StandardBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"world");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("test.txt"), "output was: {:?}", output);
        assert!(output.contains("1"), "output was: {:?}", output);
        assert!(output.contains("world"), "output was: {:?}", output);
    }

    #[test]
    fn test_standard_heading_mode() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.heading(true);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"test");
        let input = b"this is a test\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("myfile.rs"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // In heading mode, the path should appear on its own line
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 2, "lines: {:?}", lines);
        assert_eq!(lines[0], "myfile.rs");
        assert!(lines[1].contains("test"), "lines: {:?}", lines);
    }

    #[test]
    fn test_standard_no_path() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.path(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("hello"), "output was: {:?}", output);
        // Should contain line number and match, but no path
        assert!(
            output.starts_with("1:") || output.starts_with("1-"),
            "output was: {:?}",
            output
        );
    }

    #[test]
    fn test_standard_multiple_matches() {
        let wtr = no_color_buffer();
        let mut printer = StandardBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\nhello again\ngoodbye\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("multi.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("hello world"), "output was: {:?}", output);
        assert!(output.contains("hello again"), "output was: {:?}", output);
        assert!(!output.contains("goodbye"), "output was: {:?}", output);
    }

    #[test]
    fn test_summary_count() {
        let wtr = no_color_buffer();
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::Count);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\nhello again\ngoodbye\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("test.txt"), "output was: {:?}", output);
        assert!(output.contains("2"), "output was: {:?}", output);
    }

    #[test]
    fn test_summary_files_with_matches() {
        let wtr = no_color_buffer();
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::FilesWithMatches);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("found.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("found.txt"), "output was: {:?}", output);
    }

    #[test]
    fn test_summary_files_without_match() {
        let wtr = no_color_buffer();
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::FilesWithoutMatch);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"no match here\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("empty.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("empty.txt"), "output was: {:?}", output);
    }

    #[test]
    fn test_json_basic() {
        let wtr = Vec::new();
        let mut printer = JSONBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"world");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3, "expected 3 lines (begin, match, end), got: {:?}", lines);

        // Verify begin message
        let begin: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(begin["type"], "begin");
        assert_eq!(begin["data"]["path"]["text"], "test.txt");

        // Verify match message
        let match_msg: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(match_msg["type"], "match");
        assert!(
            match_msg["data"]["lines"]["text"]
                .as_str()
                .unwrap()
                .contains("hello world"),
            "match data was: {:?}",
            match_msg
        );
        assert!(match_msg["data"]["submatches"][0]["match"]["text"]
            .as_str()
            .unwrap()
            .contains("world"));

        // Verify end message
        let end: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(end["type"], "end");
        assert_eq!(end["data"]["stats"]["matched_lines"], 1);
    }

    #[test]
    fn test_standard_only_matching() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.only_matching(true).path(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"say hello now\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        let trimmed = output.trim();
        assert!(trimmed.contains("hello"), "output was: {:?}", trimmed);
    }

    #[test]
    fn test_stats_default() {
        let stats = Stats::default();
        assert_eq!(stats.matches, 0);
        assert_eq!(stats.matched_lines, 0);
        assert_eq!(stats.bytes_searched, 0);
    }

    #[test]
    fn test_stats_add() {
        let mut a = Stats::default();
        a.matches = 5;
        a.matched_lines = 3;
        a.files_searched = 1;

        let mut b = Stats::default();
        b.matches = 10;
        b.matched_lines = 7;
        b.files_searched = 2;

        a.add(&b);
        assert_eq!(a.matches, 15);
        assert_eq!(a.matched_lines, 10);
        assert_eq!(a.files_searched, 3);
    }

    #[test]
    fn test_standard_column_number() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.column(true).path(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"world");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // Column should be 7 (1-indexed position of "world" in "hello world")
        assert!(output.contains("7"), "output was: {:?}", output);
    }

    #[test]
    fn test_standard_trim() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.trim(true).path(false).line_number(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"   hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(false)
            .build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // Output should not have leading spaces
        assert!(
            output.starts_with("hello"),
            "output was: {:?}",
            output
        );
    }

    #[test]
    fn test_standard_replacement() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder
            .replacement(Some(b"REPLACED"))
            .path(false)
            .line_number(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"world");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(false)
            .build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.contains("REPLACED"), "output was: {:?}", output);
        assert!(output.contains("hello"), "output was: {:?}", output);
        // Original "world" should be replaced
        assert!(!output.contains("world"), "output was: {:?}", output);
    }

    #[test]
    fn test_summary_quiet() {
        let wtr = no_color_buffer();
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::Quiet);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(output.is_empty(), "quiet mode should produce no output, got: {:?}", output);
    }

    #[test]
    fn test_json_multiple_matches() {
        let wtr = Vec::new();
        let mut printer = JSONBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"line");
        let input = b"first line\nsecond line\nno match\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("multi.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        // begin + 2 matches + end = 4
        assert_eq!(lines.len(), 4, "lines: {:?}", lines);

        let m1: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(m1["type"], "match");

        let m2: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(m2["type"], "match");

        let end: serde_json::Value = serde_json::from_str(lines[3]).unwrap();
        assert_eq!(end["type"], "end");
        assert_eq!(end["data"]["stats"]["matched_lines"], 2);
    }

    #[test]
    fn test_standard_max_columns() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.max_columns(Some(10)).path(false).line_number(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"x");
        let input = b"this is a very long line with x in it somewhere\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(false)
            .build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // Should contain "Omitted" since max_columns_preview is false by default
        assert!(
            output.contains("Omitted"),
            "output was: {:?}",
            output
        );
    }

    #[test]
    fn test_standard_max_columns_preview() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder
            .max_columns(Some(10))
            .max_columns_preview(true)
            .path(false)
            .line_number(false);
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"this");
        let input = b"this is a very long line\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(false)
            .build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        assert!(
            output.contains("more bytes"),
            "output was: {:?}",
            output
        );
    }

    #[test]
    fn test_standard_context_with_searcher() {
        let wtr = no_color_buffer();
        let mut printer = StandardBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"match");
        // Lines: before, MATCH, after, separator, before2, MATCH2, after2
        let input = b"before line\nthe match line\nafter line\nfiller1\nfiller2\nanother match\ntrailing\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .before_context(1)
            .after_context(1)
            .build();
        let sink = printer.sink_with_path(&matcher, Path::new("ctx.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // Should contain match lines
        assert!(output.contains("match"), "output was: {:?}", output);
        // Should contain context lines with '-' separator
        assert!(output.contains("-"), "output was: {:?}", output);
        // Should contain ':' separator for matches
        assert!(output.contains(":"), "output was: {:?}", output);
    }

    #[test]
    fn test_standard_heading_with_path_terminator() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.heading(true).path_terminator(Some(b'\0'));
        let mut printer = builder.build(wtr);
        let matcher = SubstringMatcher::new(b"hello");
        let input = b"hello world\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = printer.wtr.into_inner();
        // Path should be followed by null byte
        assert!(output.contains(&0u8), "output was: {:?}", output);
    }

    #[test]
    fn test_summary_count_matches() {
        let wtr = no_color_buffer();
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::CountMatches);
        let mut printer = builder.build(wtr);
        // Pattern "o" appears multiple times
        let matcher = SubstringMatcher::new(b"o");
        let input = b"hello world or foo\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("multi.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // "o" appears in: hell[o] w[o]rld [o]r f[o][o] = 5 matches
        assert!(
            output.contains("5"),
            "expected 5 matches, output was: {:?}",
            output
        );
    }

    #[test]
    fn test_json_no_matches() {
        let wtr = Vec::new();
        let mut printer = JSONBuilder::new().build(wtr);
        let matcher = SubstringMatcher::new(b"xyz");
        let input = b"no match here\n";

        let mut searcher = grep_searcher::SearcherBuilder::new().build();
        let sink = printer.sink_with_path(&matcher, Path::new("empty.txt"));
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        // begin + end = 2
        assert_eq!(lines.len(), 2, "lines: {:?}", lines);

        let end: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(end["type"], "end");
        assert_eq!(end["data"]["stats"]["matches"], 0);
        assert_eq!(end["data"]["stats"]["matched_lines"], 0);
    }

    #[test]
    fn test_standard_per_match() {
        let wtr = no_color_buffer();
        let mut builder = StandardBuilder::new();
        builder.per_match(true).path(false).line_number(false);
        let mut printer = builder.build(wtr);
        // "o" appears 3 times in "foo boo"
        let matcher = SubstringMatcher::new(b"o");
        let input = b"foo boo\n";

        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(false)
            .build();
        let sink = printer.sink(&matcher);
        searcher.search_slice(&matcher, input, sink).unwrap();

        let output = String::from_utf8(printer.wtr.into_inner()).unwrap();
        // Each "o" match should produce a separate output line
        let lines: Vec<&str> = output.lines().collect();
        // "foo boo" has o at positions 1,2,5,6 = 4 matches
        assert!(lines.len() >= 2, "per_match should produce multiple lines, got: {:?}", lines);
    }
}

