/*!
The `grep-regex` crate provides the default regex engine for ripgrep. It
implements the [`Matcher`] trait from `grep-matcher` using the `regex` crate's
byte-oriented regex engine (`regex::bytes::Regex`).

The primary types are:

- [`RegexMatcher`]: A compiled regex that implements [`grep_matcher::Matcher`].
- [`RegexMatcherBuilder`]: A builder for configuring and constructing a
  [`RegexMatcher`].
- [`RegexCaptures`]: Capture group storage implementing
  [`grep_matcher::Captures`].
- [`Error`]: The error type for regex compilation failures.
*/

use std::collections::HashSet;
use std::fmt;

use grep_matcher::{
    ByteSet, Captures, LineMatchKind, LineTerminator, Match, Matcher,
};
use regex::bytes::{Regex, RegexBuilder};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// An error that can occur when building or using a `RegexMatcher`.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    /// An error from the regex engine.
    Regex(String),
    /// A generic error message.
    Generic(String),
}

impl Error {
    /// Create a new error from a regex compilation error.
    fn regex(err: regex::Error) -> Error {
        Error {
            kind: ErrorKind::Regex(err.to_string()),
        }
    }

    /// Create a new error with a generic message.
    fn generic(msg: impl Into<String>) -> Error {
        Error {
            kind: ErrorKind::Generic(msg.into()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::Regex(ref msg) => write!(f, "regex error: {}", msg),
            ErrorKind::Generic(ref msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, err)
    }
}

// ---------------------------------------------------------------------------
// RegexCaptures
// ---------------------------------------------------------------------------

/// `RegexCaptures` stores the results of capture groups from a regex match.
///
/// It implements [`grep_matcher::Captures`], providing access to individual
/// capture group matches by index.
#[derive(Clone, Debug)]
pub struct RegexCaptures {
    /// Stored capture group matches (overall match at index 0).
    slots: Vec<Option<Match>>,
    /// The number of capture groups (including group 0).
    group_count: usize,
}

impl RegexCaptures {
    /// Create a new empty set of captures with the given number of groups.
    fn new(group_count: usize) -> RegexCaptures {
        RegexCaptures {
            slots: vec![None; group_count],
            group_count,
        }
    }

    /// Clear all capture group matches.
    fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
    }
}

impl Captures for RegexCaptures {
    fn len(&self) -> usize {
        self.group_count
    }

    fn get(&self, i: usize) -> Option<Match> {
        self.slots.get(i).copied().flatten()
    }
}

// ---------------------------------------------------------------------------
// RegexMatcher
// ---------------------------------------------------------------------------

/// A `RegexMatcher` implements [`grep_matcher::Matcher`] using the `regex`
/// crate's byte-oriented regex engine.
///
/// It supports capture groups, configurable line terminators, and various
/// regex compilation options through [`RegexMatcherBuilder`].
#[derive(Clone, Debug)]
pub struct RegexMatcher {
    /// The compiled regex.
    regex: Regex,
    /// Capture locations for efficient capture group extraction.
    /// We store the number of capture groups here.
    capture_group_count: usize,
    /// Optional line terminator configuration.
    line_terminator: Option<LineTerminator>,
    /// A set of non-matching bytes (bytes that can never appear in a match).
    non_matching_bytes: Option<ByteSet>,
}

impl RegexMatcher {
    /// Create a new `RegexMatcher` from the given pattern with default
    /// settings.
    ///
    /// For more control, use [`RegexMatcherBuilder`].
    pub fn new(pattern: &str) -> Result<RegexMatcher, Error> {
        RegexMatcherBuilder::new().build(pattern)
    }

    /// Return true if the compiled regex is known to never produce a match.
    ///
    /// This is a heuristic check. Currently, regex compilation generally
    /// succeeds only for patterns that can potentially match something, so
    /// this always returns `false`.
    pub fn is_match_impossible(&self) -> bool {
        false
    }
}

impl Matcher for RegexMatcher {
    type Captures = RegexCaptures;
    type Error = Error;

    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Error> {
        Ok(self
            .regex
            .find_at(haystack, at)
            .map(|m| Match::new(m.start(), m.end())))
    }

    fn new_captures(&self) -> Result<RegexCaptures, Error> {
        Ok(RegexCaptures::new(self.capture_group_count))
    }

    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        caps: &mut RegexCaptures,
    ) -> Result<bool, Error> {
        caps.clear();
        let locs = {
            let mut locs = self.regex.capture_locations();
            if self.regex.captures_read_at(&mut locs, haystack, at).is_none()
            {
                return Ok(false);
            }
            locs
        };
        for i in 0..self.capture_group_count {
            if let Some((start, end)) = locs.get(i) {
                caps.slots[i] = Some(Match::new(start, end));
            }
        }
        Ok(true)
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        self.line_terminator
    }

    fn non_matching_bytes(&self) -> Option<&ByteSet> {
        self.non_matching_bytes.as_ref()
    }

    fn find_candidate_line(
        &self,
        haystack: &[u8],
    ) -> Result<Option<LineMatchKind>, Error> {
        Ok(self
            .find(haystack)?
            .map(|m| LineMatchKind::Confirmed(m.start())))
    }
}

// ---------------------------------------------------------------------------
// RegexMatcherBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a [`RegexMatcher`].
///
/// The builder provides a fluent API for configuring regex compilation
/// options such as case sensitivity, smart-case, fixed-string (literal)
/// matching, word/line boundaries, and more.
///
/// # Example
///
/// ```
/// use grep_regex::RegexMatcherBuilder;
///
/// let matcher = RegexMatcherBuilder::new()
///     .case_insensitive(true)
///     .build("hello")
///     .unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct RegexMatcherBuilder {
    case_insensitive: Option<bool>,
    case_smart: bool,
    fixed_strings: bool,
    word: bool,
    line: bool,
    dot_matches_new_line: bool,
    multi_line: bool,
    unicode: bool,
    octal: bool,
    crlf: bool,
    line_terminator: Option<LineTerminator>,
    size_limit: usize,
    dfa_size_limit: usize,
    nest_limit: u32,
}

impl Default for RegexMatcherBuilder {
    fn default() -> RegexMatcherBuilder {
        RegexMatcherBuilder::new()
    }
}

impl RegexMatcherBuilder {
    /// Create a new builder with default settings.
    ///
    /// By default, smart-case is enabled, unicode is enabled, and other
    /// options are disabled.
    pub fn new() -> RegexMatcherBuilder {
        RegexMatcherBuilder {
            case_insensitive: None,
            case_smart: true,
            fixed_strings: false,
            word: false,
            line: false,
            dot_matches_new_line: false,
            multi_line: false,
            unicode: true,
            octal: false,
            crlf: false,
            line_terminator: None,
            size_limit: 10 * (1 << 20),  // 10 MB
            dfa_size_limit: 10 * (1 << 20),  // 10 MB
            nest_limit: 250,
        }
    }

    /// Build a `RegexMatcher` from a single pattern string.
    pub fn build(&self, pattern: &str) -> Result<RegexMatcher, Error> {
        let patterns = [pattern.to_string()];
        self.build_many(&patterns)
    }

    /// Set a single pattern. This is a convenience that calls `build`.
    pub fn pattern(&mut self, pattern: &str) -> &mut RegexMatcherBuilder {
        // This is a no-op setter; the pattern is passed to `build()`.
        // Included for API compatibility.
        let _ = pattern;
        self
    }

    /// Set multiple patterns. This is a convenience that deduplicates and
    /// then passes the patterns to `build_many`.
    pub fn patterns(
        &mut self,
        _patterns: &[String],
    ) -> &mut RegexMatcherBuilder {
        // This is a no-op setter; patterns are passed to `build_many()`.
        self
    }

    /// Build a `RegexMatcher` from multiple pattern strings.
    ///
    /// The patterns are deduplicated, optionally escaped/wrapped, and then
    /// joined with `|` to form a single alternation pattern.
    pub fn build_many(
        &self,
        patterns: &[String],
    ) -> Result<RegexMatcher, Error> {
        if patterns.is_empty() {
            return Err(Error::generic("no patterns provided"));
        }

        // Deduplicate patterns while preserving order.
        let mut seen = HashSet::new();
        let mut unique_patterns: Vec<&str> = Vec::new();
        for p in patterns {
            if seen.insert(p.as_str()) {
                unique_patterns.push(p.as_str());
            }
        }

        // Apply transformations to each pattern.
        let transformed: Vec<String> = unique_patterns
            .iter()
            .map(|p| self.transform_pattern(p))
            .collect();

        // Join patterns into a single alternation.
        let combined = if transformed.len() == 1 {
            transformed.into_iter().next().unwrap()
        } else {
            let inner = transformed.join("|");
            format!("(?:{})", inner)
        };

        // Determine case insensitivity.
        let case_insensitive = self.resolve_case_insensitive(&combined);

        // Build the regex.
        let regex = RegexBuilder::new(&combined)
            .case_insensitive(case_insensitive)
            .multi_line(self.multi_line)
            .dot_matches_new_line(self.dot_matches_new_line)
            .unicode(self.unicode)
            .octal(self.octal)
            .size_limit(self.size_limit)
            .dfa_size_limit(self.dfa_size_limit)
            .nest_limit(self.nest_limit)
            .crlf(self.crlf)
            .build()
            .map_err(Error::regex)?;

        let capture_group_count = regex.captures_len();

        let line_terminator = if self.crlf {
            Some(LineTerminator::CRLF)
        } else {
            self.line_terminator
        };

        // Build non-matching bytes set if a line terminator is configured
        // and the regex does not match newlines (i.e., dot_matches_new_line
        // is false). This is an optimization hint.
        let non_matching_bytes = None;

        Ok(RegexMatcher {
            regex,
            capture_group_count,
            line_terminator,
            non_matching_bytes,
        })
    }

    /// Force case-insensitive matching.
    ///
    /// When set to `true`, the search is always case-insensitive regardless
    /// of smart-case. When set to `false`, case-insensitive is disabled
    /// (but smart-case may still enable it).
    pub fn case_insensitive(
        &mut self,
        yes: bool,
    ) -> &mut RegexMatcherBuilder {
        self.case_insensitive = Some(yes);
        self
    }

    /// Enable smart-case mode (the default).
    ///
    /// When enabled, if the pattern contains no uppercase ASCII letters,
    /// the search is case-insensitive. If it contains any uppercase ASCII
    /// letter, the search is case-sensitive.
    ///
    /// This is overridden by an explicit call to `case_insensitive`.
    pub fn case_smart(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.case_smart = yes;
        self
    }

    /// Treat all patterns as literal (fixed) strings.
    ///
    /// When enabled, regex metacharacters in patterns are escaped so they
    /// match literally.
    pub fn fixed_strings(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.fixed_strings = yes;
        self
    }

    /// Require matches to be surrounded by word boundaries (`\b`).
    pub fn word(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.word = yes;
        self
    }

    /// Require matches to span entire lines.
    ///
    /// Wraps each pattern in `(?m:^...$)` for full-line matching.
    pub fn line(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.line = yes;
        self
    }

    /// Allow `.` to match newlines.
    pub fn dot_matches_new_line(
        &mut self,
        yes: bool,
    ) -> &mut RegexMatcherBuilder {
        self.dot_matches_new_line = yes;
        self
    }

    /// Enable multi-line mode (`(?m)` and `(?s)` flags).
    pub fn multi_line(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.multi_line = yes;
        self
    }

    /// Enable or disable Unicode mode.
    ///
    /// When disabled, `.` will match any byte, and character classes like
    /// `\w` will only match ASCII.
    pub fn unicode(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.unicode = yes;
        self
    }

    /// Enable octal escape sequences (e.g., `\123`).
    pub fn octal(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.octal = yes;
        self
    }

    /// Enable CRLF-aware matching.
    ///
    /// When enabled, `$` in multiline mode matches before `\r\n`, and the
    /// line terminator is set to CRLF.
    pub fn crlf(&mut self, yes: bool) -> &mut RegexMatcherBuilder {
        self.crlf = yes;
        self
    }

    /// Set the line terminator.
    ///
    /// The line terminator is reported by the `Matcher::line_terminator`
    /// method and is used by higher-level consumers for line segmentation.
    pub fn line_terminator(
        &mut self,
        line_terminator: Option<LineTerminator>,
    ) -> &mut RegexMatcherBuilder {
        self.line_terminator = line_terminator;
        self
    }

    /// Set the approximate size limit (in bytes) of the compiled regex.
    pub fn size_limit(&mut self, limit: usize) -> &mut RegexMatcherBuilder {
        self.size_limit = limit;
        self
    }

    /// Set the approximate size limit (in bytes) of the regex DFA cache.
    pub fn dfa_size_limit(
        &mut self,
        limit: usize,
    ) -> &mut RegexMatcherBuilder {
        self.dfa_size_limit = limit;
        self
    }

    /// Set the nesting limit for the regex parser.
    pub fn nest_limit(&mut self, limit: u32) -> &mut RegexMatcherBuilder {
        self.nest_limit = limit;
        self
    }

    // ----- Internal helpers -----

    /// Transform a single pattern according to builder configuration.
    ///
    /// Applies, in order: fixed-string escaping, word boundary wrapping,
    /// and line boundary wrapping.
    fn transform_pattern(&self, pattern: &str) -> String {
        let mut p = if self.fixed_strings {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };

        if self.word {
            p = format!(r"\b(?:{})\b", p);
        }

        if self.line {
            p = format!("(?m:^(?:{})$)", p);
        }

        p
    }

    /// Determine whether to enable case-insensitive matching.
    ///
    /// Priority:
    /// 1. Explicit `case_insensitive` setting.
    /// 2. Smart-case heuristic (if enabled).
    /// 3. Default: case-sensitive.
    fn resolve_case_insensitive(&self, pattern: &str) -> bool {
        // Explicit setting takes precedence.
        if let Some(ci) = self.case_insensitive {
            return ci;
        }

        // Smart-case: if the pattern has no uppercase ASCII letters,
        // enable case-insensitive matching.
        if self.case_smart {
            return !has_uppercase_ascii(pattern);
        }

        // Default: case-sensitive.
        false
    }
}

/// Return true if the given string contains any ASCII uppercase letter.
///
/// This is used for smart-case detection. We only look at raw ASCII
/// letters since the pattern may contain regex syntax (like `\b`, `(?:`,
/// etc.) that should not trigger case-sensitivity.
fn has_uppercase_ascii(pattern: &str) -> bool {
    // We need to be smart about this: only look at literal characters,
    // not escape sequences or flags. For simplicity, we scan for any
    // uppercase letter that is NOT part of a common regex escape or
    // inline flag group.
    //
    // A robust approach: iterate the bytes and check for uppercase letters,
    // skipping characters preceded by `\` (escape sequences).
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Skip the escape character and the next character.
            i += 2;
            continue;
        }
        // Skip inline flag groups like (?i), (?m:...), etc.
        if bytes[i] == b'(' && i + 1 < bytes.len() && bytes[i + 1] == b'?' {
            // Skip until we find `)` or `:`.
            i += 2;
            while i < bytes.len()
                && bytes[i] != b')'
                && bytes[i] != b':'
            {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if bytes[i].is_ascii_uppercase() {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::Matcher;

    #[test]
    fn test_simple_match() {
        let matcher = RegexMatcher::new("hello").unwrap();
        let haystack = b"say hello world";
        let m = matcher.find(haystack).unwrap().unwrap();
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 9);
    }

    #[test]
    fn test_no_match() {
        let matcher = RegexMatcher::new("xyz").unwrap();
        let haystack = b"hello world";
        assert!(matcher.find(haystack).unwrap().is_none());
    }

    #[test]
    fn test_find_at() {
        let matcher = RegexMatcher::new("o").unwrap();
        let haystack = b"hello world";
        // First 'o' at index 4
        let m = matcher.find_at(haystack, 0).unwrap().unwrap();
        assert_eq!(m.start(), 4);
        // Next 'o' at index 7
        let m = matcher.find_at(haystack, 5).unwrap().unwrap();
        assert_eq!(m.start(), 7);
    }

    #[test]
    fn test_is_match() {
        let matcher = RegexMatcher::new("world").unwrap();
        assert!(matcher.is_match(b"hello world").unwrap());
        assert!(!matcher.is_match(b"hello earth").unwrap());
    }

    #[test]
    fn test_captures() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("(hel)(lo)")
            .unwrap();
        let haystack = b"say hello";
        let mut caps = matcher.new_captures().unwrap();
        assert!(matcher.captures(haystack, &mut caps).unwrap());
        // Group 0: overall match "hello"
        let m0 = caps.get(0).unwrap();
        assert_eq!(m0.start(), 4);
        assert_eq!(m0.end(), 9);
        // Group 1: "hel"
        let m1 = caps.get(1).unwrap();
        assert_eq!(m1.start(), 4);
        assert_eq!(m1.end(), 7);
        // Group 2: "lo"
        let m2 = caps.get(2).unwrap();
        assert_eq!(m2.start(), 7);
        assert_eq!(m2.end(), 9);
    }

    #[test]
    fn test_case_insensitive() {
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(true)
            .build("hello")
            .unwrap();
        assert!(matcher.is_match(b"HELLO").unwrap());
        assert!(matcher.is_match(b"Hello").unwrap());
    }

    #[test]
    fn test_smart_case_lowercase() {
        // All lowercase pattern → case-insensitive search (smart-case default).
        let matcher = RegexMatcherBuilder::new()
            .build("hello")
            .unwrap();
        assert!(matcher.is_match(b"HELLO").unwrap());
        assert!(matcher.is_match(b"hello").unwrap());
    }

    #[test]
    fn test_smart_case_uppercase() {
        // Pattern with uppercase → case-sensitive search.
        let matcher = RegexMatcherBuilder::new()
            .build("Hello")
            .unwrap();
        assert!(matcher.is_match(b"Hello").unwrap());
        assert!(!matcher.is_match(b"hello").unwrap());
        assert!(!matcher.is_match(b"HELLO").unwrap());
    }

    #[test]
    fn test_smart_case_disabled() {
        // With smart-case disabled, all-lowercase is case-sensitive.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("hello")
            .unwrap();
        assert!(matcher.is_match(b"hello").unwrap());
        assert!(!matcher.is_match(b"HELLO").unwrap());
    }

    #[test]
    fn test_fixed_strings() {
        let matcher = RegexMatcherBuilder::new()
            .fixed_strings(true)
            .case_smart(false)
            .build("he.lo")
            .unwrap();
        // Should match literal "he.lo", not "hello"
        assert!(matcher.is_match(b"he.lo").unwrap());
        assert!(!matcher.is_match(b"hello").unwrap());
    }

    #[test]
    fn test_word_boundary() {
        let matcher = RegexMatcherBuilder::new()
            .word(true)
            .case_smart(false)
            .build("cat")
            .unwrap();
        assert!(matcher.is_match(b"the cat sat").unwrap());
        assert!(!matcher.is_match(b"concatenate").unwrap());
    }

    #[test]
    fn test_line_matching() {
        let matcher = RegexMatcherBuilder::new()
            .line(true)
            .multi_line(true)
            .case_smart(false)
            .build("hello")
            .unwrap();
        assert!(matcher.is_match(b"hello").unwrap());
        assert!(!matcher.is_match(b"say hello").unwrap());
        assert!(matcher.is_match(b"say\nhello\nworld").unwrap());
    }

    #[test]
    fn test_multiple_patterns() {
        let patterns: Vec<String> =
            vec!["foo".to_string(), "bar".to_string(), "foo".to_string()];
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build_many(&patterns)
            .unwrap();
        assert!(matcher.is_match(b"foo").unwrap());
        assert!(matcher.is_match(b"bar").unwrap());
        assert!(!matcher.is_match(b"baz").unwrap());
    }

    #[test]
    fn test_line_terminator() {
        let matcher = RegexMatcherBuilder::new()
            .line_terminator(Some(LineTerminator::Byte(b'\n')))
            .build("test")
            .unwrap();
        assert_eq!(
            matcher.line_terminator(),
            Some(LineTerminator::Byte(b'\n'))
        );
    }

    #[test]
    fn test_crlf_mode() {
        let matcher = RegexMatcherBuilder::new()
            .crlf(true)
            .build("test")
            .unwrap();
        assert_eq!(matcher.line_terminator(), Some(LineTerminator::CRLF));
    }

    #[test]
    fn test_binary_haystack() {
        // Ensure we can match on non-UTF-8 bytes.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("abc")
            .unwrap();
        let haystack: &[u8] = &[0xFF, 0xFE, b'a', b'b', b'c', 0xFF];
        let m = matcher.find(haystack).unwrap().unwrap();
        assert_eq!(m.start(), 2);
        assert_eq!(m.end(), 5);
    }

    #[test]
    fn test_impossible_match() {
        let matcher = RegexMatcher::new("hello").unwrap();
        assert!(!matcher.is_match_impossible());
    }

    #[test]
    fn test_find_candidate_line() {
        let matcher = RegexMatcher::new("hello").unwrap();
        let haystack = b"say hello world";
        let result = matcher.find_candidate_line(haystack).unwrap().unwrap();
        match result {
            LineMatchKind::Confirmed(offset) => assert_eq!(offset, 4),
            LineMatchKind::Candidate(_) => panic!("expected confirmed"),
        }
    }

    #[test]
    fn test_error_display() {
        let err = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("[invalid")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("regex error"), "got: {}", msg);
    }

    #[test]
    fn test_has_uppercase_ascii() {
        assert!(!has_uppercase_ascii("hello"));
        assert!(has_uppercase_ascii("Hello"));
        assert!(!has_uppercase_ascii("hello123"));
        assert!(!has_uppercase_ascii(r"\bword\b"));
        // Escaped uppercase should NOT count.
        assert!(!has_uppercase_ascii(r"\Bword"));
        // Inline flags should not count.
        assert!(!has_uppercase_ascii("(?i:hello)"));
        assert!(!has_uppercase_ascii("(?ms)hello"));
    }

    #[test]
    fn test_captures_len() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("(a)(b)(c)")
            .unwrap();
        let caps = matcher.new_captures().unwrap();
        assert_eq!(caps.len(), 4); // group 0 + 3 capture groups
    }

    #[test]
    fn test_replace() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("world")
            .unwrap();
        let haystack = b"hello world, world!";
        let mut dst = Vec::new();
        matcher
            .replace(haystack, &mut dst, |_m, dst| {
                dst.extend_from_slice(b"earth");
                true
            })
            .unwrap();
        assert_eq!(dst, b"hello earth, earth!");
    }
}
