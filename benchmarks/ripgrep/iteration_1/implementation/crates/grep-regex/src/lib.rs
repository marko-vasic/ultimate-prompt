/*!
Default regex engine implementation for grep-like programs.

This crate provides [`RegexMatcher`], which implements the
[`grep_matcher::Matcher`] trait using Rust's `regex-automata` crate as the
underlying regex engine. A [`RegexMatcherBuilder`] is provided for configuring
and constructing matchers with features like smart-case, word boundaries,
whole-line matching, and fixed (literal) string matching.

# Example

```
use grep_regex::RegexMatcherBuilder;
use grep_matcher::Matcher;

let matcher = RegexMatcherBuilder::new()
    .case_insensitive(true)
    .build("hello")
    .unwrap();
assert!(matcher.is_match(b"Hello World").unwrap());
```
*/

use std::collections::BTreeSet;
use std::fmt;

use grep_matcher::{ByteSet, LineTerminator, Match, Matcher, Captures};
use regex_automata::{
    meta::Regex,
    Input, MatchKind, PatternID,
    util::syntax::Config as SyntaxConfig,
    meta::Config as MetaConfig,
};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// An error that can occur when building or using a [`RegexMatcher`].
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
#[allow(dead_code)]
enum ErrorKind {
    /// An error from building / compiling a regex.
    Regex(String),
    /// An error from parsing regex syntax.
    Syntax(String),
}

impl Error {
    fn regex(err: regex_automata::meta::BuildError) -> Error {
        Error {
            kind: ErrorKind::Regex(format!("{err:?}")),
        }
    }

    #[allow(dead_code)]
    fn syntax(err: regex_syntax::Error) -> Error {
        Error {
            kind: ErrorKind::Syntax(err.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Regex(msg) => write!(f, "regex build error: {msg}"),
            ErrorKind::Syntax(msg) => write!(f, "regex syntax error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

// ---------------------------------------------------------------------------
// RegexCaptures
// ---------------------------------------------------------------------------

/// Capture groups for a [`RegexMatcher`].
///
/// This stores a single overall match span plus any sub-capture group spans
/// produced by the regex engine.
#[derive(Clone, Debug)]
pub struct RegexCaptures {
    /// The spans of each capture group. Index 0 is the overall match.
    /// `None` means the group did not participate in the match.
    slots: Vec<Option<Match>>,
    /// Total number of groups (including group 0).
    group_count: usize,
}

impl RegexCaptures {
    /// Create a new empty `RegexCaptures` with the given number of groups.
    fn new(group_count: usize) -> RegexCaptures {
        RegexCaptures {
            slots: vec![None; group_count],
            group_count,
        }
    }

    /// Clear all stored capture spans.
    fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
    }
}

impl Captures for RegexCaptures {
    fn group_len(&self) -> usize {
        self.group_count
    }

    fn group(&self, index: usize) -> Option<Match> {
        self.slots.get(index).copied().flatten()
    }
}

// ---------------------------------------------------------------------------
// RegexMatcherBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a [`RegexMatcher`].
///
/// The builder provides a fluent API for configuring regex compilation options
/// such as case sensitivity, unicode support, multiline mode, and more.
///
/// # Example
///
/// ```
/// use grep_regex::RegexMatcherBuilder;
/// use grep_matcher::Matcher;
///
/// let matcher = RegexMatcherBuilder::new()
///     .case_smart(true)
///     .build("hello")
///     .unwrap();
/// // "hello" has no uppercase → smart-case enables case-insensitive
/// assert!(matcher.is_match(b"HELLO world").unwrap());
/// ```
#[derive(Clone, Debug)]
pub struct RegexMatcherBuilder {
    case_insensitive: Option<bool>,
    case_smart: bool,
    multi_line: bool,
    dot_all: bool,
    unicode: bool,
    crlf: bool,
    word: bool,
    line_terminator: Option<LineTerminator>,
    fixed_strings: bool,
    whole_line: bool,
    regex_size_limit: Option<usize>,
    dfa_size_limit: Option<usize>,
}

impl RegexMatcherBuilder {
    /// Create a new builder with default settings.
    ///
    /// Defaults:
    /// - `case_smart`: `true`
    /// - `unicode`: `true`
    /// - All other boolean flags: `false`
    pub fn new() -> RegexMatcherBuilder {
        RegexMatcherBuilder {
            case_insensitive: None,
            case_smart: true,
            multi_line: false,
            dot_all: false,
            unicode: true,
            crlf: false,
            word: false,
            line_terminator: None,
            fixed_strings: false,
            whole_line: false,
            regex_size_limit: None,
            dfa_size_limit: None,
        }
    }

    /// Force case-insensitive matching.
    ///
    /// When set to `true`, this overrides smart-case logic.
    pub fn case_insensitive(&mut self, yes: bool) -> &mut Self {
        self.case_insensitive = Some(yes);
        self
    }

    /// Enable smart-case mode.
    ///
    /// When enabled (and `case_insensitive` is not explicitly set), the
    /// pattern is matched case-insensitively if it contains no uppercase
    /// letters, and case-sensitively otherwise.
    pub fn case_smart(&mut self, yes: bool) -> &mut Self {
        self.case_smart = yes;
        self
    }

    /// Enable multiline matching mode.
    ///
    /// When enabled, `^` and `$` match at line boundaries within the input
    /// rather than only at the start and end of the entire haystack.
    pub fn multi_line(&mut self, yes: bool) -> &mut Self {
        self.multi_line = yes;
        self
    }

    /// Make `.` match `\n` when multiline mode is enabled.
    pub fn dot_all(&mut self, yes: bool) -> &mut Self {
        self.dot_all = yes;
        self
    }

    /// Enable or disable Unicode mode.
    ///
    /// When enabled (the default), character classes like `\w`, `\d`, etc.
    /// match Unicode characters rather than just ASCII.
    pub fn unicode(&mut self, yes: bool) -> &mut Self {
        self.unicode = yes;
        self
    }

    /// Enable CRLF line terminator mode.
    ///
    /// When enabled, the line terminator is set to CRLF (`\r\n`).
    pub fn crlf(&mut self, yes: bool) -> &mut Self {
        self.crlf = yes;
        self
    }

    /// Wrap the pattern with word boundary assertions (`\b`).
    pub fn word(&mut self, yes: bool) -> &mut Self {
        self.word = yes;
        self
    }

    /// Set the line terminator.
    pub fn line_terminator(&mut self, lt: Option<LineTerminator>) -> &mut Self {
        self.line_terminator = lt;
        self
    }

    /// Treat the pattern as a fixed (literal) string.
    ///
    /// When enabled, the pattern is escaped so that all regex meta-characters
    /// are treated literally.
    pub fn fixed_strings(&mut self, yes: bool) -> &mut Self {
        self.fixed_strings = yes;
        self
    }

    /// Match the pattern against the entire line.
    ///
    /// When enabled, the pattern is wrapped with `^` and `$` anchors
    /// (in multiline mode).
    pub fn whole_line(&mut self, yes: bool) -> &mut Self {
        self.whole_line = yes;
        self
    }

    /// Set the approximate size limit of the compiled regex (in bytes).
    pub fn regex_size_limit(&mut self, limit: usize) -> &mut Self {
        self.regex_size_limit = Some(limit);
        self
    }

    /// Set the DFA (lazy or full) size limit in bytes.
    pub fn dfa_size_limit(&mut self, limit: usize) -> &mut Self {
        self.dfa_size_limit = Some(limit);
        self
    }

    /// Build a [`RegexMatcher`] from a single pattern.
    pub fn build(&self, pattern: &str) -> Result<RegexMatcher, Error> {
        self.build_many(&[pattern])
    }

    /// Build a [`RegexMatcher`] from multiple patterns.
    ///
    /// The patterns are joined into an alternation so that a match on any
    /// single pattern is reported.
    pub fn build_many(&self, patterns: &[&str]) -> Result<RegexMatcher, Error> {
        let processed = self.process_patterns(patterns)?;
        let case_insensitive = self.resolve_case_insensitivity(&processed);
        let regex = self.compile(&processed, case_insensitive)?;
        let line_terminator = self.resolve_line_terminator();
        let non_matching = build_non_matching_bytes(&line_terminator);

        Ok(RegexMatcher {
            regex,
            line_terminator,
            non_matching,
        })
    }

    /// Process the raw patterns: escape fixed strings, wrap with word /
    /// whole-line anchors, deduplicate.
    fn process_patterns(&self, patterns: &[&str]) -> Result<Vec<String>, Error> {
        let mut seen = BTreeSet::new();
        let mut result = Vec::with_capacity(patterns.len());

        for &pat in patterns {
            let mut p = if self.fixed_strings {
                regex_syntax::escape(pat)
            } else {
                pat.to_string()
            };

            if self.word {
                p = format!(r"(?-u:\b)(?:{p})(?-u:\b)");
            }

            if self.whole_line {
                p = format!(r"(?m:^)(?:{p})(?m:$)");
            }

            if seen.insert(p.clone()) {
                result.push(p);
            }
        }

        Ok(result)
    }

    /// Determine whether the match should be case-insensitive.
    ///
    /// If the user explicitly set `case_insensitive`, that value wins.
    /// Otherwise, if `case_smart` is enabled, we check whether the
    /// (processed) patterns contain any uppercase characters, skipping
    /// regex metacharacters and escape sequences.
    fn resolve_case_insensitivity(&self, patterns: &[String]) -> bool {
        if let Some(ci) = self.case_insensitive {
            return ci;
        }
        if self.case_smart {
            return !has_uppercase_literal(patterns);
        }
        false
    }

    /// Compile the processed patterns into a `regex_automata::meta::Regex`.
    fn compile(
        &self,
        patterns: &[String],
        case_insensitive: bool,
    ) -> Result<Regex, Error> {
        let syntax = SyntaxConfig::new()
            .case_insensitive(case_insensitive)
            .unicode(self.unicode)
            .utf8(self.unicode)
            .multi_line(self.multi_line)
            .dot_matches_new_line(self.dot_all);

        let mut meta_config = MetaConfig::new()
            .match_kind(MatchKind::LeftmostFirst);

        if let Some(limit) = self.dfa_size_limit {
            meta_config = meta_config.hybrid_cache_capacity(limit);
        }
        if let Some(limit) = self.regex_size_limit {
            meta_config = meta_config.nfa_size_limit(Some(limit));
        }

        let mut builder = Regex::builder();
        builder.syntax(syntax).configure(meta_config);

        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
        builder.build_many(&pattern_refs).map_err(Error::regex)
    }

    /// Determine the line terminator to use.
    fn resolve_line_terminator(&self) -> Option<LineTerminator> {
        if self.crlf {
            Some(LineTerminator::CRLF)
        } else {
            self.line_terminator
        }
    }
}

impl Default for RegexMatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Smart-case helpers
// ---------------------------------------------------------------------------

/// Returns `true` if any of the patterns contain an uppercase literal
/// character (i.e. a character that is not a regex metacharacter and is
/// uppercase according to Unicode).
fn has_uppercase_literal(patterns: &[String]) -> bool {
    for pattern in patterns {
        let mut chars = pattern.chars().peekable();
        while let Some(ch) = chars.next() {
            // Skip escape sequences: if we see a backslash, skip the next
            // character because it is part of the escape, not a literal.
            if ch == '\\' {
                // Consume the escaped character.
                let _ = chars.next();
                continue;
            }
            // Skip regex metacharacters — they are structural, not literal.
            if regex_syntax::is_meta_character(ch) {
                continue;
            }
            if ch.is_uppercase() {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Non-matching bytes
// ---------------------------------------------------------------------------

/// Build a `ByteSet` of bytes that should never appear in a match.
///
/// Currently this just marks the line terminator byte as non-matching when
/// a line terminator is set and dot-all is not enabled — the regex should
/// never match across line boundaries in normal grep usage.
fn build_non_matching_bytes(lt: &Option<LineTerminator>) -> ByteSet {
    let mut set = ByteSet::empty();
    if let Some(lt) = lt {
        set.add(lt.as_byte());
        if lt.is_crlf() {
            set.add(b'\r');
        }
    }
    set
}

// ---------------------------------------------------------------------------
// RegexMatcher
// ---------------------------------------------------------------------------

/// A matcher backed by `regex_automata::meta::Regex`.
///
/// Implements the [`grep_matcher::Matcher`] trait so that it can be plugged
/// into any grep pipeline component.
#[derive(Clone, Debug)]
pub struct RegexMatcher {
    regex: Regex,
    line_terminator: Option<LineTerminator>,
    non_matching: ByteSet,
}

impl RegexMatcher {
    /// Convenience: build a matcher for a single pattern with default
    /// settings.
    ///
    /// Equivalent to `RegexMatcherBuilder::new().build(pattern)`.
    pub fn new(pattern: &str) -> Result<RegexMatcher, Error> {
        RegexMatcherBuilder::new().build(pattern)
    }
}

impl Matcher for RegexMatcher {
    type Error = Error;
    type Captures = RegexCaptures;

    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Error> {
        let input = Input::new(haystack).span(at..haystack.len());
        Ok(self.regex.find(input).map(|m| Match::new(m.start(), m.end())))
    }

    fn new_captures(&self) -> Result<RegexCaptures, Error> {
        let count = self
            .regex
            .group_info()
            .group_len(PatternID::ZERO);
        Ok(RegexCaptures::new(count))
    }

    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        caps: &mut RegexCaptures,
    ) -> Result<bool, Error> {
        caps.clear();
        let input = Input::new(haystack).span(at..haystack.len());
        let mut auto_caps = self.regex.create_captures();
        self.regex.captures(input, &mut auto_caps);

        let overall = match auto_caps.get_match() {
            Some(m) => m,
            None => return Ok(false),
        };

        // Store group 0 (the overall match).
        if caps.group_count > 0 {
            caps.slots[0] = Some(Match::new(overall.start(), overall.end()));
        }

        // Store remaining capture groups.
        let pid = overall.pattern();
        let ngroups = self.regex.group_info().group_len(pid);
        for i in 1..ngroups {
            if i >= caps.group_count {
                break;
            }
            if let Some(span) = auto_caps.get_group(i) {
                caps.slots[i] = Some(Match::new(span.start, span.end));
            }
        }

        Ok(true)
    }

    fn capture_count(&self) -> usize {
        self.regex.group_info().group_len(PatternID::ZERO)
    }

    fn capture_index(&self, name: &str) -> Option<usize> {
        self.regex
            .group_info()
            .to_index(PatternID::ZERO, name)
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        self.line_terminator
    }

    fn non_matching_bytes(&self) -> &ByteSet {
        &self.non_matching
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::Matcher;

    #[test]
    fn test_simple_match() {
        let m = RegexMatcher::new("foo").unwrap();
        let hay = b"hello foo bar";
        let result = m.find(hay).unwrap().unwrap();
        assert_eq!(result.start(), 6);
        assert_eq!(result.end(), 9);
    }

    #[test]
    fn test_no_match() {
        let m = RegexMatcher::new("xyz").unwrap();
        assert!(m.find(b"hello world").unwrap().is_none());
    }

    #[test]
    fn test_find_at() {
        let m = RegexMatcher::new("foo").unwrap();
        let hay = b"foo bar foo baz";
        let second = m.find_at(hay, 4).unwrap().unwrap();
        assert_eq!(second.start(), 8);
        assert_eq!(second.end(), 11);
    }

    #[test]
    fn test_is_match() {
        let m = RegexMatcher::new("bar").unwrap();
        assert!(m.is_match(b"foo bar baz").unwrap());
        assert!(!m.is_match(b"no match").unwrap());
    }

    #[test]
    fn test_case_insensitive() {
        let m = RegexMatcherBuilder::new()
            .case_insensitive(true)
            .build("hello")
            .unwrap();
        assert!(m.is_match(b"HELLO").unwrap());
    }

    #[test]
    fn test_smart_case_lower() {
        // All lowercase → case-insensitive
        let m = RegexMatcherBuilder::new()
            .case_smart(true)
            .build("hello")
            .unwrap();
        assert!(m.is_match(b"HELLO").unwrap());
    }

    #[test]
    fn test_smart_case_upper() {
        // Contains uppercase → case-sensitive
        let m = RegexMatcherBuilder::new()
            .case_smart(true)
            .build("Hello")
            .unwrap();
        assert!(m.is_match(b"Hello").unwrap());
        assert!(!m.is_match(b"hello").unwrap());
    }

    #[test]
    fn test_smart_case_meta_chars() {
        // Metacharacters like \d shouldn't trigger case-sensitive matching
        let m = RegexMatcherBuilder::new()
            .case_smart(true)
            .build(r"\d+foo")
            .unwrap();
        assert!(m.is_match(b"123FOO").unwrap());
    }

    #[test]
    fn test_fixed_strings() {
        // "foo.bar" should match literally, not as a regex
        let m = RegexMatcherBuilder::new()
            .fixed_strings(true)
            .case_smart(false)
            .build("foo.bar")
            .unwrap();
        assert!(m.is_match(b"foo.bar").unwrap());
        assert!(!m.is_match(b"fooXbar").unwrap());
    }

    #[test]
    fn test_word_boundary() {
        let m = RegexMatcherBuilder::new()
            .word(true)
            .case_smart(false)
            .build("foo")
            .unwrap();
        assert!(m.is_match(b"hello foo bar").unwrap());
        assert!(!m.is_match(b"foobar").unwrap());
    }

    #[test]
    fn test_whole_line() {
        let m = RegexMatcherBuilder::new()
            .whole_line(true)
            .multi_line(true)
            .case_smart(false)
            .build("foo")
            .unwrap();
        assert!(m.is_match(b"foo").unwrap());
        assert!(!m.is_match(b"foo bar").unwrap());
    }

    #[test]
    fn test_build_many() {
        let m = RegexMatcherBuilder::new()
            .case_smart(false)
            .build_many(&["foo", "bar"])
            .unwrap();
        assert!(m.is_match(b"foo").unwrap());
        assert!(m.is_match(b"bar").unwrap());
        assert!(!m.is_match(b"baz").unwrap());
    }

    #[test]
    fn test_build_many_dedup() {
        // Duplicate patterns should be deduplicated
        let m = RegexMatcherBuilder::new()
            .case_smart(false)
            .build_many(&["foo", "foo", "bar"])
            .unwrap();
        assert!(m.is_match(b"foo").unwrap());
        assert!(m.is_match(b"bar").unwrap());
    }

    #[test]
    fn test_capture_count() {
        // Pattern with no groups
        let m = RegexMatcher::new("foo").unwrap();
        assert_eq!(m.capture_count(), 1); // group 0 always exists

        // Pattern with one explicit group
        let m2 = RegexMatcher::new("(foo)(bar)").unwrap();
        assert_eq!(m2.capture_count(), 3); // group 0 + 2 explicit
    }

    #[test]
    fn test_captures_at() {
        let m = RegexMatcher::new("(foo)(bar)").unwrap();
        let mut caps = m.new_captures().unwrap();
        let hay = b"foobar";
        let found = m.captures_at(hay, 0, &mut caps).unwrap();
        assert!(found);

        // Group 0: overall match
        assert_eq!(caps.group(0), Some(Match::new(0, 6)));
        // Group 1: "foo"
        assert_eq!(caps.group(1), Some(Match::new(0, 3)));
        // Group 2: "bar"
        assert_eq!(caps.group(2), Some(Match::new(3, 6)));
    }

    #[test]
    fn test_capture_index_by_name() {
        let m = RegexMatcher::new("(?P<word>\\w+)").unwrap();
        assert_eq!(m.capture_index("word"), Some(1));
        assert_eq!(m.capture_index("nonexistent"), None);
    }

    #[test]
    fn test_line_terminator_default() {
        let m = RegexMatcher::new("foo").unwrap();
        assert_eq!(m.line_terminator(), None);
    }

    #[test]
    fn test_line_terminator_set() {
        let m = RegexMatcherBuilder::new()
            .line_terminator(Some(LineTerminator::Byte(b'\n')))
            .build("foo")
            .unwrap();
        assert_eq!(m.line_terminator(), Some(LineTerminator::Byte(b'\n')));
    }

    #[test]
    fn test_crlf_line_terminator() {
        let m = RegexMatcherBuilder::new()
            .crlf(true)
            .build("foo")
            .unwrap();
        assert_eq!(m.line_terminator(), Some(LineTerminator::CRLF));
    }

    #[test]
    fn test_non_matching_bytes() {
        let m = RegexMatcherBuilder::new()
            .line_terminator(Some(LineTerminator::Byte(b'\n')))
            .build("foo")
            .unwrap();
        assert!(m.non_matching_bytes().contains(b'\n'));
        assert!(!m.non_matching_bytes().contains(b'a'));
    }

    #[test]
    fn test_non_matching_bytes_crlf() {
        let m = RegexMatcherBuilder::new()
            .crlf(true)
            .build("foo")
            .unwrap();
        assert!(m.non_matching_bytes().contains(b'\n'));
        assert!(m.non_matching_bytes().contains(b'\r'));
    }

    #[test]
    fn test_error_display() {
        let err = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("[invalid");
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("error"), "error message was: {msg}");
    }

    #[test]
    fn test_find_iter() {
        let m = RegexMatcher::new("ab").unwrap();
        let hay = b"ab cd ab ef ab";
        let mut matches = vec![];
        m.find_iter(hay, |mat| {
            matches.push(mat);
            true
        })
        .unwrap();
        assert_eq!(
            matches,
            vec![Match::new(0, 2), Match::new(6, 8), Match::new(12, 14)]
        );
    }

    #[test]
    fn test_dot_all() {
        let m = RegexMatcherBuilder::new()
            .dot_all(true)
            .case_smart(false)
            .build("foo.bar")
            .unwrap();
        assert!(m.is_match(b"foo\nbar").unwrap());
    }

    #[test]
    fn test_unicode_default() {
        // \w should match Unicode word chars by default
        let m = RegexMatcher::new(r"\w+").unwrap();
        assert!(m.is_match("café".as_bytes()).unwrap());
    }
}
