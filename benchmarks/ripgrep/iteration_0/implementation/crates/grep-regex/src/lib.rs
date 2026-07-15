//! # grep-regex
//!
//! This crate provides a concrete regex implementation for the grep pipeline,
//! backed by [`regex_automata::meta::Regex`]. It implements the
//! [`grep_matcher::Matcher`] trait, allowing it to plug directly into
//! the grep search and printing infrastructure.
//!
//! The primary types are:
//!
//! - [`RegexMatcherBuilder`]: A builder for configuring and constructing a
//!   [`RegexMatcher`].
//! - [`RegexMatcher`]: A compiled regex matcher that implements
//!   [`grep_matcher::Matcher`].
//! - [`RegexCaptures`]: Capture group information produced by a match,
//!   implementing [`grep_matcher::Captures`].
//! - [`Error`]: The error type used throughout this crate.
//!
//! # Features
//!
//! - **Smart case**: When enabled, patterns that contain no uppercase
//!   characters are matched case-insensitively.
//! - **Fixed strings**: Treat the pattern as a literal string (no regex
//!   metacharacters).
//! - **Word matching**: Wrap the pattern in word boundary assertions.
//! - **Line matching**: Wrap the pattern in line anchor assertions.
//! - **Whole-line matching**: Match against the entire line.
//! - **Multi-line & dot-all**: Control whether `.` matches newlines.
//! - **CRLF support**: Proper handling of `\r\n` line endings.
//! - **Multiple patterns**: Build a matcher from multiple patterns (union).

#![deny(missing_docs)]

use std::fmt;

use grep_matcher::{ByteSet, LineTerminator, Match};
use regex_automata::util::captures::Captures;
use regex_automata::{meta, Input};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// An error that can occur when building or using a regex matcher.
///
/// This type wraps errors from the underlying regex engine
/// ([`regex_automata`]) and from pattern parsing ([`regex_syntax`]).
pub struct Error {
    message: String,
}

impl Error {
    /// Create a new error with the given message.
    fn new(message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("message", &self.message)
            .finish()
    }
}

impl std::error::Error for Error {}

impl From<regex_automata::meta::BuildError> for Error {
    fn from(err: regex_automata::meta::BuildError) -> Error {
        Error::new(err.to_string())
    }
}

impl From<regex_syntax::Error> for Error {
    fn from(err: regex_syntax::Error) -> Error {
        Error::new(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// RegexMatcherBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a [`RegexMatcher`].
///
/// The builder provides a fluent API for configuring all aspects of how the
/// regex is compiled and how matching behaves. Use [`RegexMatcherBuilder::new`]
/// to create a builder with sensible defaults, then chain configuration
/// methods, and finally call [`RegexMatcherBuilder::build`] to produce a
/// compiled [`RegexMatcher`].
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
pub struct RegexMatcherBuilder {
    case_insensitive: bool,
    case_smart: bool,
    multi_line: bool,
    dot_all: bool,
    unicode: bool,
    octal: bool,
    word: bool,
    line: bool,
    fixed_strings: bool,
    whole_line: bool,
    crlf: bool,
    line_terminator: Option<u8>,
    size_limit: usize,
    dfa_size_limit: usize,
}

impl RegexMatcherBuilder {
    /// Create a new `RegexMatcherBuilder` with sensible defaults.
    ///
    /// The defaults are:
    /// - `unicode`: `true`
    /// - `case_smart`: `true`
    /// - `size_limit`: 10 MB
    /// - `dfa_size_limit`: 10 MB
    /// - All other boolean options: `false`
    /// - `line_terminator`: `None`
    pub fn new() -> RegexMatcherBuilder {
        RegexMatcherBuilder {
            case_insensitive: false,
            case_smart: true,
            multi_line: false,
            dot_all: false,
            unicode: true,
            octal: false,
            word: false,
            line: false,
            fixed_strings: false,
            whole_line: false,
            crlf: false,
            line_terminator: None,
            size_limit: 10 * (1 << 20),      // 10 MB
            dfa_size_limit: 10 * (1 << 20),   // 10 MB
        }
    }

    /// Set whether to enable case-insensitive matching.
    ///
    /// When enabled, patterns match regardless of case. This can be
    /// overridden by smart case if `case_smart` is also enabled.
    pub fn case_insensitive(&mut self, yes: bool) -> &mut Self {
        self.case_insensitive = yes;
        self
    }

    /// Set whether to enable smart case matching.
    ///
    /// When enabled, if the pattern contains no uppercase characters, then
    /// matching is done case-insensitively. If the pattern contains at least
    /// one uppercase character, matching is case-sensitive (unless
    /// `case_insensitive` is explicitly set).
    pub fn case_smart(&mut self, yes: bool) -> &mut Self {
        self.case_smart = yes;
        self
    }

    /// Set whether to enable multi-line mode.
    ///
    /// When enabled, `^` and `$` match at the beginning and end of each
    /// line, not just the beginning and end of the entire haystack.
    pub fn multi_line(&mut self, yes: bool) -> &mut Self {
        self.multi_line = yes;
        self
    }

    /// Set whether `.` matches newline characters in multi-line mode.
    pub fn dot_all(&mut self, yes: bool) -> &mut Self {
        self.dot_all = yes;
        self
    }

    /// Set whether to enable Unicode mode.
    ///
    /// When enabled, character classes like `\w`, `\d`, etc. match Unicode
    /// characters. When disabled, they match only ASCII characters.
    pub fn unicode(&mut self, yes: bool) -> &mut Self {
        self.unicode = yes;
        self
    }

    /// Set whether to allow octal escapes in patterns.
    pub fn octal(&mut self, yes: bool) -> &mut Self {
        self.octal = yes;
        self
    }

    /// Set whether to wrap the pattern in word boundary assertions.
    ///
    /// When enabled, the pattern is wrapped in `\b...\b` (Unicode mode) or
    /// `(?-u:\b)...(?-u:\b)` (non-Unicode mode).
    pub fn word(&mut self, yes: bool) -> &mut Self {
        self.word = yes;
        self
    }

    /// Set whether to wrap the pattern in line anchor assertions.
    ///
    /// When enabled, the pattern is wrapped in `(?m:^)...(?m:$)`.
    pub fn line(&mut self, yes: bool) -> &mut Self {
        self.line = yes;
        self
    }

    /// Set whether to treat the pattern as a fixed (literal) string.
    ///
    /// When enabled, all regex metacharacters are escaped so that the
    /// pattern is matched literally.
    pub fn fixed_strings(&mut self, yes: bool) -> &mut Self {
        self.fixed_strings = yes;
        self
    }

    /// Set whether to match the entire line.
    ///
    /// When enabled, the pattern is wrapped so that it must match the
    /// complete contents of a line (excluding the line terminator).
    pub fn whole_line(&mut self, yes: bool) -> &mut Self {
        self.whole_line = yes;
        self
    }

    /// Set whether to use CRLF (`\r\n`) as the line terminator.
    ///
    /// This affects line anchors (`^`, `$`) and line-wrapping behavior.
    pub fn crlf(&mut self, yes: bool) -> &mut Self {
        self.crlf = yes;
        self
    }

    /// Set the line terminator byte.
    ///
    /// This is used to configure the regex engine and the matcher's line
    /// terminator for line-oriented searching. Setting this to `Some(b'\n')`
    /// is the most common configuration.
    pub fn line_terminator(&mut self, byte: Option<u8>) -> &mut Self {
        self.line_terminator = byte;
        self
    }

    /// Set the size limit (in bytes) for the compiled regex.
    ///
    /// If the compiled regex would exceed this size, building fails with an
    /// error.
    pub fn size_limit(&mut self, limit: usize) -> &mut Self {
        self.size_limit = limit;
        self
    }

    /// Set the size limit (in bytes) for the DFA cache used by the regex
    /// engine.
    pub fn dfa_size_limit(&mut self, limit: usize) -> &mut Self {
        self.dfa_size_limit = limit;
        self
    }

    /// Build a [`RegexMatcher`] from a single pattern string.
    ///
    /// This applies all configured options (smart case, word boundaries,
    /// line anchors, fixed strings, etc.) before compiling the pattern.
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is invalid or if the compiled regex
    /// exceeds the configured size limits.
    pub fn build(&self, pattern: &str) -> Result<RegexMatcher, Error> {
        self.build_many(&[pattern])
    }

    /// Build a [`RegexMatcher`] from multiple pattern strings.
    ///
    /// The patterns are deduplicated and combined into a single regex
    /// (effectively a union / alternation). All configured options apply
    /// to each pattern.
    ///
    /// # Errors
    ///
    /// Returns an error if any pattern is invalid or if the compiled regex
    /// exceeds the configured size limits.
    pub fn build_many(&self, patterns: &[&str]) -> Result<RegexMatcher, Error> {
        // Step 1: Process each pattern (escape, wrap, etc.)
        let mut processed: Vec<String> = Vec::with_capacity(patterns.len());
        for &pat in patterns {
            processed.push(self.process_pattern(pat));
        }

        // Deduplicate patterns while preserving order.
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<String> = processed
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect();

        // Step 2: Determine case sensitivity via smart case.
        let case_insensitive = if self.case_insensitive {
            true
        } else if self.case_smart {
            // Smart case: if ALL patterns are "all lowercase", then
            // use case-insensitive matching.
            deduped.iter().all(|p| is_all_lowercase(p))
        } else {
            false
        };

        // Step 3: Build the regex-automata syntax config.
        let syntax_config = regex_automata::util::syntax::Config::new()
            .case_insensitive(case_insensitive)
            .multi_line(self.multi_line || self.line || self.whole_line)
            .dot_matches_new_line(self.dot_all)
            .unicode(self.unicode)
            .utf8(!self.unicode)
            .crlf(self.crlf)
            .octal(self.octal);

        // Step 4: Build the regex-automata meta config.
        let meta_config = meta::Config::new()
            .nfa_size_limit(Some(self.size_limit))
            .dfa_size_limit(Some(self.dfa_size_limit))
            .utf8_empty(!self.unicode);

        // Step 5: Compile the regex.
        let pattern_strs: Vec<&str> = deduped.iter().map(|s| s.as_str()).collect();
        let regex = meta::Regex::builder()
            .configure(meta_config)
            .syntax(syntax_config)
            .build_many(&pattern_strs)?;

        // Step 6: Compute metadata.
        let caps_len = regex.captures_len();

        // Determine line terminator.
        let line_terminator = if self.crlf {
            Some(LineTerminator::crlf())
        } else {
            self.line_terminator.map(LineTerminator::byte)
        };

        // Build non-matching bytes set.
        // If a line terminator is set, that byte can never appear in a match.
        let non_matching_bytes = line_terminator.map(|lt| {
            let mut set = ByteSet::empty();
            for &b in lt.as_bytes() {
                set.add(b);
            }
            set
        });

        Ok(RegexMatcher {
            regex,
            non_matching_bytes,
            line_terminator,
            caps_len,
        })
    }

    /// Process a single pattern by applying escaping, word boundaries,
    /// and line anchoring.
    fn process_pattern(&self, pattern: &str) -> String {
        let mut pat = if self.fixed_strings {
            regex_syntax::escape(pattern)
        } else {
            pattern.to_string()
        };

        if self.word {
            if self.unicode {
                pat = format!(r"\b(?:{})\b", pat);
            } else {
                pat = format!(r"(?-u:\b)(?:{})(?-u:\b)", pat);
            }
        }

        if self.whole_line {
            if self.crlf {
                pat = format!(r"(?m:^)(?:{})(?m:$)", pat);
            } else {
                pat = format!(r"(?m:^)(?:{})(?m:$)", pat);
            }
        } else if self.line {
            pat = format!(r"(?m:^)(?:{})(?m:$)", pat);
        }

        pat
    }
}

impl Default for RegexMatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RegexMatcher
// ---------------------------------------------------------------------------

/// A compiled regex matcher backed by [`regex_automata::meta::Regex`].
///
/// This type implements [`grep_matcher::Matcher`], making it suitable for
/// use throughout the grep pipeline. It is constructed via
/// [`RegexMatcherBuilder`].
///
/// # Example
///
/// ```
/// use grep_matcher::Matcher;
/// use grep_regex::RegexMatcherBuilder;
///
/// let matcher = RegexMatcherBuilder::new()
///     .case_smart(false)
///     .build("foo")
///     .unwrap();
/// let m = matcher.find(b"hello foo bar").unwrap().unwrap();
/// assert_eq!(m.start(), 6);
/// assert_eq!(m.end(), 9);
/// ```
pub struct RegexMatcher {
    regex: meta::Regex,
    non_matching_bytes: Option<ByteSet>,
    line_terminator: Option<LineTerminator>,
    caps_len: usize,
}

impl grep_matcher::Matcher for RegexMatcher {
    type Captures = RegexCaptures;
    type Error = Error;

    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Error> {
        let input = Input::new(haystack).range(at..);
        Ok(self.regex.find(input).map(|m| Match::new(m.start(), m.end())))
    }

    fn new_captures(&self) -> Result<RegexCaptures, Error> {
        Ok(RegexCaptures {
            caps: self.regex.create_captures(),
        })
    }

    fn capture_count(&self) -> usize {
        self.caps_len
    }

    fn capture_index(&self, name: &str) -> Option<usize> {
        // For a single-pattern regex, group names are scoped to pattern 0.
        let pid = regex_automata::PatternID::must(0);
        self.regex.group_info().to_index(pid, name)
    }

    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        caps: &mut RegexCaptures,
    ) -> Result<bool, Error> {
        let input = Input::new(haystack).range(at..);
        self.regex.search_captures(&input, &mut caps.caps);
        Ok(caps.caps.is_match())
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        self.line_terminator
    }

    fn non_matching_bytes(&self) -> Option<&ByteSet> {
        self.non_matching_bytes.as_ref()
    }

    fn find_candidate_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, Error> {
        self.find_at(haystack, at)
    }
}

// ---------------------------------------------------------------------------
// RegexCaptures
// ---------------------------------------------------------------------------

/// Capture group information produced by a [`RegexMatcher`].
///
/// This type wraps [`regex_automata::util::captures::Captures`] and implements
/// [`grep_matcher::Captures`].
pub struct RegexCaptures {
    caps: Captures,
}

impl grep_matcher::Captures for RegexCaptures {
    fn len(&self) -> usize {
        self.caps.group_len()
    }

    fn get(&self, i: usize) -> Option<Match> {
        self.caps.get_group(i).map(|sp| Match::new(sp.start, sp.end))
    }
}

// ---------------------------------------------------------------------------
// Smart case helper
// ---------------------------------------------------------------------------

/// Returns `true` if the pattern string contains no uppercase literal
/// characters.
///
/// This is used for "smart case" matching: if a pattern is "all lowercase",
/// then matching is done case-insensitively. We parse the pattern using
/// `regex_syntax::ast::parse::Parser` and walk the AST, checking only
/// literal characters for uppercase. Character classes, escapes, and
/// other constructs are ignored — only explicit literal characters are
/// checked.
///
/// If the pattern fails to parse, we fall back to treating it as not
/// all-lowercase (i.e. case-sensitive matching), which is the safer default.
fn is_all_lowercase(pattern: &str) -> bool {
    use regex_syntax::ast::{self, Ast};

    let ast = match ast::parse::Parser::new().parse(pattern) {
        Ok(ast) => ast,
        Err(_) => return false,
    };

    fn check_ast(ast: &Ast) -> bool {
        match ast {
            Ast::Empty(_) => true,
            Ast::Flags(_) => true,
            Ast::Literal(lit) => {
                // Only check the literal character. If it is uppercase,
                // the pattern is not "all lowercase".
                !lit.c.is_uppercase()
            }
            Ast::Dot(_) => true,
            Ast::Assertion(_) => true,
            Ast::ClassUnicode(_) => true,
            Ast::ClassPerl(_) => true,
            Ast::ClassBracketed(class) => {
                // Check for literals inside bracket classes.
                check_class_set(&class.kind)
            }
            Ast::Repetition(rep) => check_ast(&rep.ast),
            Ast::Group(group) => check_ast(&group.ast),
            Ast::Alternation(alt) => alt.asts.iter().all(check_ast),
            Ast::Concat(concat) => concat.asts.iter().all(check_ast),
        }
    }

    fn check_class_set(set: &ast::ClassSet) -> bool {
        match set {
            ast::ClassSet::Item(item) => check_class_set_item(item),
            ast::ClassSet::BinaryOp(op) => {
                check_class_set(&op.lhs) && check_class_set(&op.rhs)
            }
        }
    }

    fn check_class_set_item(item: &ast::ClassSetItem) -> bool {
        match item {
            ast::ClassSetItem::Empty(_) => true,
            ast::ClassSetItem::Literal(lit) => !lit.c.is_uppercase(),
            ast::ClassSetItem::Range(range) => {
                !range.start.c.is_uppercase() && !range.end.c.is_uppercase()
            }
            ast::ClassSetItem::Ascii(_) => true,
            ast::ClassSetItem::Unicode(_) => true,
            ast::ClassSetItem::Perl(_) => true,
            ast::ClassSetItem::Bracketed(class) => {
                check_class_set(&class.kind)
            }
            ast::ClassSetItem::Union(union) => {
                union.items.iter().all(check_class_set_item)
            }
        }
    }

    check_ast(&ast)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::{Captures as _, Matcher};

    // -----------------------------------------------------------------------
    // Error tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_display() {
        let err = Error::new("something went wrong");
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn test_error_debug() {
        let err = Error::new("test error");
        let debug = format!("{:?}", err);
        assert!(debug.contains("test error"));
    }

    #[test]
    fn test_error_from_build_error() {
        // Trigger a BuildError by building an invalid regex.
        let result = meta::Regex::new("[invalid");
        assert!(result.is_err());
        let err: Error = result.unwrap_err().into();
        assert!(!err.to_string().is_empty());
    }

    // -----------------------------------------------------------------------
    // Smart case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_all_lowercase_simple() {
        assert!(is_all_lowercase("hello"));
        assert!(is_all_lowercase("hello world"));
        assert!(is_all_lowercase("foo.*bar"));
        assert!(is_all_lowercase(r"\d+"));
        assert!(is_all_lowercase(""));
    }

    #[test]
    fn test_is_all_lowercase_with_uppercase() {
        assert!(!is_all_lowercase("Hello"));
        assert!(!is_all_lowercase("heLLo"));
        assert!(!is_all_lowercase("FOO"));
    }

    #[test]
    fn test_is_all_lowercase_with_classes() {
        // Character classes with uppercase literals.
        assert!(!is_all_lowercase("[A-Z]"));
        assert!(is_all_lowercase("[a-z]"));
        assert!(is_all_lowercase(r"\w+"));
    }

    #[test]
    fn test_is_all_lowercase_invalid_pattern() {
        // Invalid patterns return false (safer default).
        assert!(!is_all_lowercase("[unclosed"));
    }

    // -----------------------------------------------------------------------
    // Builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_default() {
        let builder = RegexMatcherBuilder::new();
        assert!(!builder.case_insensitive);
        assert!(builder.case_smart);
        assert!(!builder.multi_line);
        assert!(builder.unicode);
    }

    #[test]
    fn test_builder_build_simple() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("hello")
            .unwrap();
        let result = matcher.find_at(b"say hello world", 0).unwrap();
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 9);
    }

    #[test]
    fn test_builder_case_insensitive() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .case_insensitive(true)
            .build("hello")
            .unwrap();
        let result = matcher.find_at(b"say HELLO world", 0).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_builder_smart_case_lowercase_pattern() {
        // Pattern is all lowercase, so smart case makes it case-insensitive.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(true)
            .case_insensitive(false)
            .build("hello")
            .unwrap();
        let result = matcher.find_at(b"say HELLO world", 0).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_builder_smart_case_uppercase_pattern() {
        // Pattern has uppercase chars, so smart case keeps it case-sensitive.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(true)
            .case_insensitive(false)
            .build("Hello")
            .unwrap();
        let result = matcher.find_at(b"say hello world", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_builder_fixed_strings() {
        // Without fixed_strings, "." matches any char.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .fixed_strings(true)
            .build("a.b")
            .unwrap();
        let result = matcher.find_at(b"a.b", 0).unwrap();
        assert!(result.is_some());
        // "aXb" should NOT match because we're matching the literal "a.b".
        let result = matcher.find_at(b"aXb", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_builder_word() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .word(true)
            .build("foo")
            .unwrap();
        // "foo" as a whole word.
        let result = matcher.find_at(b"say foo bar", 0).unwrap();
        assert!(result.is_some());
        // "foobar" should NOT match at word boundary.
        let result = matcher.find_at(b"foobar", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_builder_invalid_pattern() {
        let result = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("[invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_build_many() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build_many(&["foo", "bar"])
            .unwrap();
        let result = matcher.find_at(b"say bar hello", 0).unwrap();
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 7);
    }

    #[test]
    fn test_builder_build_many_dedup() {
        // Duplicate patterns should be deduplicated.
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build_many(&["foo", "foo", "bar"])
            .unwrap();
        let result = matcher.find_at(b"say foo hello", 0).unwrap();
        assert!(result.is_some());
    }

    // -----------------------------------------------------------------------
    // RegexMatcher trait implementation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_matcher_find_at_with_offset() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        // Search starting at offset 5 should find the second "foo".
        let result = matcher.find_at(b"foo foo", 4).unwrap();
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 7);
    }

    #[test]
    fn test_matcher_find_at_no_match() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("xyz")
            .unwrap();
        let result = matcher.find_at(b"hello world", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_matcher_captures() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build(r"(\w+)\s+(\w+)")
            .unwrap();
        let mut caps = matcher.new_captures().unwrap();
        let found = matcher.captures_at(b"hello world", 0, &mut caps).unwrap();
        assert!(found);

        // Group 0: overall match.
        let m0 = caps.get(0).unwrap();
        assert_eq!(m0.start(), 0);
        assert_eq!(m0.end(), 11);

        // Group 1: first word.
        let m1 = caps.get(1).unwrap();
        assert_eq!(m1.start(), 0);
        assert_eq!(m1.end(), 5);

        // Group 2: second word.
        let m2 = caps.get(2).unwrap();
        assert_eq!(m2.start(), 6);
        assert_eq!(m2.end(), 11);
    }

    #[test]
    fn test_matcher_capture_count() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build(r"(\w+)\s+(\w+)")
            .unwrap();
        // captures_len returns total groups including group 0.
        assert_eq!(matcher.capture_count(), 3);
    }

    #[test]
    fn test_matcher_capture_index_named() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build(r"(?P<first>\w+)\s+(?P<second>\w+)")
            .unwrap();
        assert_eq!(matcher.capture_index("first"), Some(1));
        assert_eq!(matcher.capture_index("second"), Some(2));
        assert_eq!(matcher.capture_index("nonexistent"), None);
    }

    #[test]
    fn test_matcher_line_terminator() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .line_terminator(Some(b'\n'))
            .build("foo")
            .unwrap();
        let lt = matcher.line_terminator().unwrap();
        assert_eq!(lt.as_byte(), b'\n');
        assert!(!lt.is_crlf());
    }

    #[test]
    fn test_matcher_crlf_line_terminator() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .crlf(true)
            .build("foo")
            .unwrap();
        let lt = matcher.line_terminator().unwrap();
        assert!(lt.is_crlf());
        assert_eq!(lt.as_byte(), b'\n');
    }

    #[test]
    fn test_matcher_non_matching_bytes() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .line_terminator(Some(b'\n'))
            .build("foo")
            .unwrap();
        let nmb = matcher.non_matching_bytes().unwrap();
        assert!(nmb.contains(b'\n'));
        assert!(!nmb.contains(b'a'));
    }

    #[test]
    fn test_matcher_no_line_terminator() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        assert!(matcher.line_terminator().is_none());
        assert!(matcher.non_matching_bytes().is_none());
    }

    // -----------------------------------------------------------------------
    // RegexCaptures tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_captures_len_no_groups() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        let mut caps = matcher.new_captures().unwrap();
        let found = matcher.captures_at(b"foo", 0, &mut caps).unwrap();
        assert!(found);
        // Even with no explicit groups, group 0 (overall match) exists.
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_captures_get_out_of_bounds() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        let mut caps = matcher.new_captures().unwrap();
        matcher.captures_at(b"foo", 0, &mut caps).unwrap();
        // Group 1 doesn't exist for a pattern without capture groups.
        assert!(caps.get(1).is_none());
        assert!(caps.get(100).is_none());
    }

    #[test]
    fn test_captures_no_match() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        let mut caps = matcher.new_captures().unwrap();
        let found = matcher.captures_at(b"bar", 0, &mut caps).unwrap();
        assert!(!found);
        assert!(caps.get(0).is_none());
    }

    // -----------------------------------------------------------------------
    // Line wrapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_line_mode() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .line(true)
            .build("foo")
            .unwrap();
        // "foo" on its own line should match.
        let result = matcher.find_at(b"bar\nfoo\nbaz", 0).unwrap();
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 7);
    }

    // -----------------------------------------------------------------------
    // find_iter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_iter() {
        let matcher = RegexMatcherBuilder::new()
            .case_smart(false)
            .build("foo")
            .unwrap();
        let mut matches = Vec::new();
        matcher
            .find_iter(b"foo bar foo baz foo", |m| {
                matches.push(m);
                true
            })
            .unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start(), 0);
        assert_eq!(matches[1].start(), 8);
        assert_eq!(matches[2].start(), 16);
    }
}
