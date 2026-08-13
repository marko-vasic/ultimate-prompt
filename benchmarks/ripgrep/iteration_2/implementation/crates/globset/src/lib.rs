/*!
The `globset` crate provides cross-platform glob matching.

Glob patterns are converted to regular expressions for matching. A single
glob can be compiled into a [`Glob`] or [`GlobMatcher`] for matching against
individual paths. Multiple globs can be combined into a [`GlobSet`] for
efficient simultaneous matching.

# Glob Syntax

Standard glob syntax is supported:

- `?` matches any single character (except `/` when `literal_separator` is on).
- `*` matches zero or more characters (except `/` when `literal_separator` is on).
- `**` matches zero or more directories/path components.
- `[abc]` matches any one character inside the brackets.
- `[a-z]` matches a range of characters.
- `[!abc]` negates the character class.
- `{a,b,c}` matches any of the comma-separated alternatives.
- `\` escapes the next special character (when `backslash_escape` is enabled).
*/

use std::fmt;
use std::path::Path;

use bstr::ByteSlice;
use regex::Regex;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// An error that occurs when parsing or compiling a glob pattern.
#[derive(Clone, Debug)]
pub struct Error {
    glob: Option<String>,
    kind: ErrorKind,
}

impl Error {
    /// Return the glob pattern that caused this error, if available.
    pub fn glob(&self) -> Option<&str> {
        self.glob.as_deref()
    }

    /// Return the kind of this error.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.glob {
            Some(g) => write!(f, "glob '{}': {}", g, self.kind),
            None => write!(f, "glob error: {}", self.kind),
        }
    }
}

/// The kind of error that can occur when parsing a glob pattern.
#[derive(Clone, Debug)]
pub enum ErrorKind {
    /// A generic invalid glob pattern error with a message.
    InvalidGlob(String),
    /// An unclosed character class (e.g., `[abc`).
    UnclosedClass,
    /// An unclosed alternate group (e.g., `{a,b`).
    UnclosedAlternate,
    /// An invalid character range in a class (e.g., `[z-a]`).
    InvalidRange(char, char),
    /// A nested alternate group, which is not supported.
    NestedAlternate,
    /// An error from the underlying regex engine.
    Regex(String),
    /// Hints that destructuring should not be exhaustive.
    #[doc(hidden)]
    __Nonexhaustive,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::InvalidGlob(msg) => write!(f, "{}", msg),
            ErrorKind::UnclosedClass => {
                write!(f, "unclosed character class")
            }
            ErrorKind::UnclosedAlternate => {
                write!(f, "unclosed alternate group")
            }
            ErrorKind::InvalidRange(lo, hi) => {
                write!(f, "invalid character range: {}-{}", lo, hi)
            }
            ErrorKind::NestedAlternate => {
                write!(f, "nested alternates are not supported")
            }
            ErrorKind::Regex(err) => write!(f, "regex error: {}", err),
            ErrorKind::__Nonexhaustive => unreachable!(),
        }
    }
}

// ---------------------------------------------------------------------------
// Candidate – pre-normalised path for efficient repeated matching
// ---------------------------------------------------------------------------

/// A pre-normalised candidate path, suitable for repeated matching against
/// multiple globs.
///
/// The path is converted to use `/` as separator on all platforms and stored
/// as a byte slice.
#[derive(Clone, Debug)]
pub struct Candidate<'a> {
    path: Vec<u8>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Candidate<'a> {
    /// Create a new `Candidate` from the given path reference.
    pub fn new<P: AsRef<Path> + ?Sized>(path: &P) -> Candidate<'a> {
        let p = path.as_ref();
        let bytes = normalize_path(p);
        Candidate { path: bytes, _marker: std::marker::PhantomData }
    }

    /// Return the normalised path bytes.
    fn path_bytes(&self) -> &[u8] {
        &self.path
    }
}

/// Normalise a `Path` to a byte string with `/` separators.
fn normalize_path(path: &Path) -> Vec<u8> {
    let s = path.to_string_lossy();
    // On Windows, replace `\` with `/`.
    let normalized = s.replace('\\', "/");
    normalized.into_bytes()
}

// ---------------------------------------------------------------------------
// Glob
// ---------------------------------------------------------------------------

/// A single compiled glob pattern.
///
/// A `Glob` is created from a string pattern and compiles it into a regular
/// expression for matching file paths.
#[derive(Clone, Debug)]
pub struct Glob {
    /// The original glob pattern.
    glob: String,
    /// The regex string generated from the glob.
    re: String,
    /// Options used during compilation.
    opts: GlobOptions,
}

/// Internal glob compilation options.
#[derive(Clone, Debug)]
struct GlobOptions {
    case_insensitive: bool,
    literal_separator: bool,
    backslash_escape: bool,
    empty_alternates: bool,
}

impl Default for GlobOptions {
    fn default() -> GlobOptions {
        GlobOptions {
            case_insensitive: false,
            literal_separator: false,
            backslash_escape: true,
            empty_alternates: false,
        }
    }
}

impl Glob {
    /// Create a new `Glob` from the given pattern using default options.
    pub fn new(glob: &str) -> Result<Glob, Error> {
        GlobBuilder::new(glob).build()
    }

    /// Test whether the given path matches this glob pattern.
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        let candidate = Candidate::new(&path);
        self.is_match_candidate(&candidate)
    }

    /// Compile this glob into a `GlobMatcher` for efficient repeated matching.
    pub fn compile_matcher(&self) -> GlobMatcher {
        let re = self.compile_regex().expect("regex should be valid");
        GlobMatcher { glob: self.clone(), re }
    }

    /// Return the original glob pattern string.
    pub fn glob(&self) -> &str {
        &self.glob
    }

    /// Return the compiled regex string.
    pub fn regex(&self) -> &str {
        &self.re
    }

    /// Internal: compile the regex.
    fn compile_regex(&self) -> Result<Regex, Error> {
        Regex::new(&self.re).map_err(|e| Error {
            glob: Some(self.glob.clone()),
            kind: ErrorKind::Regex(e.to_string()),
        })
    }

    /// Internal: test against a candidate.
    fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        if let Ok(re) = self.compile_regex() {
            let path_str = candidate.path_bytes().to_str_lossy();
            re.is_match(&path_str)
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// GlobBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring and compiling a [`Glob`].
#[derive(Clone, Debug)]
pub struct GlobBuilder {
    glob: String,
    opts: GlobOptions,
}

impl GlobBuilder {
    /// Create a new glob builder with the given pattern.
    pub fn new(glob: &str) -> GlobBuilder {
        GlobBuilder { glob: glob.to_string(), opts: GlobOptions::default() }
    }

    /// Enable or disable case-insensitive matching.
    pub fn case_insensitive(&mut self, yes: bool) -> &mut GlobBuilder {
        self.opts.case_insensitive = yes;
        self
    }

    /// Enable or disable treating `*` and `?` as unable to match `/`.
    ///
    /// When enabled, `*` and `?` will not match the path separator `/`.
    pub fn literal_separator(&mut self, yes: bool) -> &mut GlobBuilder {
        self.opts.literal_separator = yes;
        self
    }

    /// Enable or disable backslash escaping.
    ///
    /// When enabled, `\` escapes the character that follows it.
    pub fn backslash_escape(&mut self, yes: bool) -> &mut GlobBuilder {
        self.opts.backslash_escape = yes;
        self
    }

    /// Enable or disable empty alternates in `{}` groups.
    pub fn empty_alternates(&mut self, yes: bool) -> &mut GlobBuilder {
        self.opts.empty_alternates = yes;
        self
    }

    /// Compile the glob pattern into a [`Glob`].
    pub fn build(&self) -> Result<Glob, Error> {
        let re = glob_to_regex(&self.glob, &self.opts)?;
        Ok(Glob { glob: self.glob.clone(), re, opts: self.opts.clone() })
    }
}

// ---------------------------------------------------------------------------
// GlobMatcher
// ---------------------------------------------------------------------------

/// A compiled matcher for a single glob pattern.
///
/// This is more efficient than using [`Glob::is_match`] directly when matching
/// many paths, because the regex is compiled once.
#[derive(Clone, Debug)]
pub struct GlobMatcher {
    glob: Glob,
    re: Regex,
}

impl GlobMatcher {
    /// Test whether the given path matches this glob.
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        let candidate = Candidate::new(&path);
        self.is_match_candidate(&candidate)
    }

    /// Test whether the given pre-normalised candidate path matches this glob.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        let path_str = candidate.path_bytes().to_str_lossy();
        self.re.is_match(&path_str)
    }

    /// Return a reference to the underlying [`Glob`].
    pub fn glob(&self) -> &Glob {
        &self.glob
    }
}

// ---------------------------------------------------------------------------
// GlobSet
// ---------------------------------------------------------------------------

/// A set of compiled glob patterns that can be matched simultaneously.
///
/// After building a `GlobSet`, you can check whether any glob in the set
/// matches a given path, or retrieve the indices of all matching globs.
#[derive(Clone, Debug)]
pub struct GlobSet {
    globs: Vec<Glob>,
    patterns: Vec<Regex>,
}

impl GlobSet {
    /// Create an empty `GlobSet` that matches nothing.
    pub fn empty() -> GlobSet {
        GlobSet { globs: Vec::new(), patterns: Vec::new() }
    }

    /// Return `true` if this set contains no globs.
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty()
    }

    /// Return the number of globs in this set.
    pub fn len(&self) -> usize {
        self.globs.len()
    }

    /// Test whether any glob in the set matches the given path.
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        let candidate = Candidate::new(&path);
        self.is_match_candidate(&candidate)
    }

    /// Test whether any glob in the set matches the given candidate.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        let path_str = candidate.path_bytes().to_str_lossy();
        for re in &self.patterns {
            if re.is_match(&path_str) {
                return true;
            }
        }
        false
    }

    /// Return the indices of all globs that match the given path.
    pub fn matches<P: AsRef<Path>>(&self, path: P) -> Vec<usize> {
        let candidate = Candidate::new(&path);
        self.matches_candidate(&candidate)
    }

    /// Return the indices of all globs that match the given candidate.
    pub fn matches_candidate(&self, candidate: &Candidate<'_>) -> Vec<usize> {
        let mut matches = Vec::new();
        self.matches_candidate_into(candidate, &mut matches);
        matches
    }

    /// Append the indices of all matching globs to `matches`, reusing the
    /// allocation.
    pub fn matches_candidate_into(
        &self,
        candidate: &Candidate<'_>,
        matches: &mut Vec<usize>,
    ) {
        let path_str = candidate.path_bytes().to_str_lossy();
        for (i, re) in self.patterns.iter().enumerate() {
            if re.is_match(&path_str) {
                matches.push(i);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GlobSetBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a [`GlobSet`].
pub struct GlobSetBuilder {
    globs: Vec<Glob>,
}

impl GlobSetBuilder {
    /// Create a new, empty `GlobSetBuilder`.
    pub fn new() -> GlobSetBuilder {
        GlobSetBuilder { globs: Vec::new() }
    }

    /// Add a glob to the set.
    pub fn add(&mut self, glob: Glob) -> &mut GlobSetBuilder {
        self.globs.push(glob);
        self
    }

    /// Compile all added globs into a [`GlobSet`].
    pub fn build(&self) -> Result<GlobSet, Error> {
        let mut patterns = Vec::with_capacity(self.globs.len());
        for glob in &self.globs {
            let re = glob.compile_regex()?;
            patterns.push(re);
        }
        Ok(GlobSet { globs: self.globs.clone(), patterns })
    }
}

// ---------------------------------------------------------------------------
// Glob-to-regex compilation
// ---------------------------------------------------------------------------

/// Convert a glob pattern string into a regex string.
fn glob_to_regex(glob: &str, opts: &GlobOptions) -> Result<String, Error> {
    let mut re = String::new();
    // Anchored match: the entire path must match.
    if opts.case_insensitive {
        re.push_str("(?i)");
    }
    re.push('^');

    let chars: Vec<char> = glob.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_alternate = false;

    while i < len {
        let c = chars[i];
        match c {
            '\\' if opts.backslash_escape => {
                i += 1;
                if i >= len {
                    return Err(Error {
                        glob: Some(glob.to_string()),
                        kind: ErrorKind::InvalidGlob(
                            "trailing backslash".to_string(),
                        ),
                    });
                }
                re.push_str(&regex_syntax::escape(&chars[i].to_string()));
                i += 1;
            }
            '?' => {
                if opts.literal_separator {
                    re.push_str("[^/]");
                } else {
                    re.push('.');
                }
                i += 1;
            }
            '*' => {
                // Check for `**`
                if i + 1 < len && chars[i + 1] == '*' {
                    // `**` – matches everything including `/`.
                    // Consume any surrounding `/` for nicer semantics.
                    i += 2;

                    // `**/` at start or `/**/` in middle or `/**` at end
                    // We want `**` to match zero or more path components.
                    // Check if preceded by `/` or at start AND followed by
                    // `/` or at end.
                    let preceded_by_slash = {
                        // Check the character before the first `*`.
                        let star_start = i - 2;
                        star_start == 0
                            || (star_start > 0 && chars[star_start - 1] == '/')
                    };
                    let followed_by_slash =
                        i < len && chars[i] == '/';

                    if preceded_by_slash && followed_by_slash {
                        // Pattern like `a/**/b` → match `a/b` or `a/.../b`.
                        // Consume the trailing `/`.
                        i += 1;
                        re.push_str("(?:.+/)?");
                    } else if preceded_by_slash && i == len {
                        // Pattern ends with `/**` → match everything after.
                        re.push_str(".*");
                    } else {
                        // Standalone `**` or unusual placement → match
                        // everything.
                        re.push_str(".*");
                    }
                } else {
                    // Single `*`.
                    if opts.literal_separator {
                        re.push_str("[^/]*");
                    } else {
                        re.push_str(".*");
                    }
                    i += 1;
                }
            }
            '[' => {
                // Character class.
                i += 1;
                let mut class = String::new();
                class.push('[');

                if i < len && (chars[i] == '!' || chars[i] == '^') {
                    class.push('^');
                    i += 1;
                }

                let class_start = i;
                while i < len && chars[i] != ']' {
                    if chars[i] == '\\' && opts.backslash_escape && i + 1 < len
                    {
                        class.push_str(
                            &regex_syntax::escape(&chars[i + 1].to_string()),
                        );
                        i += 2;
                    } else if i + 2 < len
                        && chars[i + 1] == '-'
                        && chars[i + 2] != ']'
                    {
                        // Range like `a-z`.
                        let lo = chars[i];
                        let hi = chars[i + 2];
                        if lo > hi {
                            return Err(Error {
                                glob: Some(glob.to_string()),
                                kind: ErrorKind::InvalidRange(lo, hi),
                            });
                        }
                        class.push_str(
                            &regex_syntax::escape(&lo.to_string()),
                        );
                        class.push('-');
                        class.push_str(
                            &regex_syntax::escape(&hi.to_string()),
                        );
                        i += 3;
                    } else {
                        // Escape special regex chars that are not special in
                        // glob character classes. Inside a regex class, we
                        // only need to escape `]`, `\`, `^` (at start), and
                        // `-` (when not at start/end). We'll do minimal
                        // escaping.
                        let ch = chars[i];
                        match ch {
                            ']' => class.push_str("\\]"),
                            '\\' => class.push_str("\\\\"),
                            '^' => class.push_str("\\^"),
                            _ => class.push(ch),
                        }
                        i += 1;
                    }
                }

                if i >= len {
                    return Err(Error {
                        glob: Some(glob.to_string()),
                        kind: ErrorKind::UnclosedClass,
                    });
                }
                if i == class_start {
                    // Empty class like `[]` – this shouldn't match anything.
                    // We handle by producing a regex that won't match.
                    // But push the `]` anyway.
                }
                class.push(']');
                i += 1; // skip `]`
                re.push_str(&class);
            }
            '{' => {
                if in_alternate {
                    return Err(Error {
                        glob: Some(glob.to_string()),
                        kind: ErrorKind::NestedAlternate,
                    });
                }
                in_alternate = true;
                re.push_str("(?:");
                i += 1;

                // Check for empty alternate like `{}`
                if i < len && chars[i] == '}' {
                    if !opts.empty_alternates {
                        return Err(Error {
                            glob: Some(glob.to_string()),
                            kind: ErrorKind::InvalidGlob(
                                "empty alternate group".to_string(),
                            ),
                        });
                    }
                }
            }
            ',' if in_alternate => {
                re.push('|');
                i += 1;
            }
            '}' if in_alternate => {
                re.push(')');
                in_alternate = false;
                i += 1;
            }
            _ => {
                // Escape regex-special characters.
                re.push_str(&regex_syntax::escape(&c.to_string()));
                i += 1;
            }
        }
    }

    if in_alternate {
        return Err(Error {
            glob: Some(glob.to_string()),
            kind: ErrorKind::UnclosedAlternate,
        });
    }

    re.push('$');
    Ok(re)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        let g = Glob::new("foo.txt").unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(!g.is_match(Path::new("bar.txt")));
    }

    #[test]
    fn test_question_mark() {
        let g = Glob::new("fo?.txt").unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(g.is_match(Path::new("fob.txt")));
        assert!(!g.is_match(Path::new("fooo.txt")));
    }

    #[test]
    fn test_star() {
        let g = Glob::new("*.txt").unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(g.is_match(Path::new(".txt")));
        // Without literal_separator, `*` matches `/`.
        assert!(g.is_match(Path::new("a/b.txt")));
    }

    #[test]
    fn test_star_literal_separator() {
        let g = GlobBuilder::new("*.txt")
            .literal_separator(true)
            .build()
            .unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(!g.is_match(Path::new("a/foo.txt")));
    }

    #[test]
    fn test_double_star() {
        let g = Glob::new("**/*.txt").unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(g.is_match(Path::new("a/foo.txt")));
        assert!(g.is_match(Path::new("a/b/c/foo.txt")));
    }

    #[test]
    fn test_double_star_prefix() {
        let g = Glob::new("src/**").unwrap();
        assert!(g.is_match(Path::new("src/foo.rs")));
        assert!(g.is_match(Path::new("src/a/b/c.rs")));
    }

    #[test]
    fn test_double_star_middle() {
        let g = Glob::new("a/**/b").unwrap();
        assert!(g.is_match(Path::new("a/b")));
        assert!(g.is_match(Path::new("a/x/b")));
        assert!(g.is_match(Path::new("a/x/y/z/b")));
    }

    #[test]
    fn test_character_class() {
        let g = Glob::new("[abc].txt").unwrap();
        assert!(g.is_match(Path::new("a.txt")));
        assert!(g.is_match(Path::new("b.txt")));
        assert!(!g.is_match(Path::new("d.txt")));
    }

    #[test]
    fn test_character_class_range() {
        let g = Glob::new("[a-z].txt").unwrap();
        assert!(g.is_match(Path::new("a.txt")));
        assert!(g.is_match(Path::new("z.txt")));
        assert!(!g.is_match(Path::new("A.txt")));
    }

    #[test]
    fn test_character_class_negation() {
        let g = Glob::new("[!a].txt").unwrap();
        assert!(!g.is_match(Path::new("a.txt")));
        assert!(g.is_match(Path::new("b.txt")));
    }

    #[test]
    fn test_alternates() {
        let g = Glob::new("{foo,bar,baz}.txt").unwrap();
        assert!(g.is_match(Path::new("foo.txt")));
        assert!(g.is_match(Path::new("bar.txt")));
        assert!(g.is_match(Path::new("baz.txt")));
        assert!(!g.is_match(Path::new("qux.txt")));
    }

    #[test]
    fn test_unclosed_class() {
        assert!(Glob::new("[abc").is_err());
    }

    #[test]
    fn test_unclosed_alternate() {
        assert!(Glob::new("{a,b").is_err());
    }

    #[test]
    fn test_case_insensitive() {
        let g = GlobBuilder::new("foo.txt")
            .case_insensitive(true)
            .build()
            .unwrap();
        assert!(g.is_match(Path::new("FOO.TXT")));
        assert!(g.is_match(Path::new("Foo.Txt")));
    }

    #[test]
    fn test_backslash_escape() {
        let g = GlobBuilder::new("foo\\*bar")
            .backslash_escape(true)
            .build()
            .unwrap();
        assert!(g.is_match(Path::new("foo*bar")));
        assert!(!g.is_match(Path::new("fooXbar")));
    }

    #[test]
    fn test_glob_matcher() {
        let g = Glob::new("*.rs").unwrap();
        let m = g.compile_matcher();
        assert!(m.is_match(Path::new("lib.rs")));
        assert!(!m.is_match(Path::new("lib.txt")));
    }

    #[test]
    fn test_glob_set() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        let set = builder.build().unwrap();

        assert!(set.is_match(Path::new("lib.rs")));
        assert!(set.is_match(Path::new("Cargo.toml")));
        assert!(!set.is_match(Path::new("README.md")));
    }

    #[test]
    fn test_glob_set_matches() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("lib.*").unwrap());
        let set = builder.build().unwrap();

        let m = set.matches(Path::new("lib.rs"));
        assert_eq!(m, vec![0, 1]);

        let m = set.matches(Path::new("main.rs"));
        assert_eq!(m, vec![0]);
    }

    #[test]
    fn test_glob_set_empty() {
        let set = GlobSet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.is_match(Path::new("anything")));
    }

    #[test]
    fn test_candidate() {
        let g = Glob::new("a/b/c.txt").unwrap();
        let m = g.compile_matcher();
        let c = Candidate::new(&Path::new("a/b/c.txt"));
        assert!(m.is_match_candidate(&c));
    }

    #[test]
    fn test_glob_returns_pattern_and_regex() {
        let g = Glob::new("*.txt").unwrap();
        assert_eq!(g.glob(), "*.txt");
        assert!(!g.regex().is_empty());
    }

    #[test]
    fn test_error_display() {
        let err = Glob::new("[abc").unwrap_err();
        assert!(err.to_string().contains("unclosed"));
        assert_eq!(err.glob(), Some("[abc"));
    }

    #[test]
    fn test_invalid_range() {
        let err = Glob::new("[z-a]").unwrap_err();
        match err.kind() {
            ErrorKind::InvalidRange('z', 'a') => {}
            other => panic!("unexpected error kind: {:?}", other),
        }
    }

    #[test]
    fn test_nested_alternates() {
        let err = Glob::new("{a,{b,c}}").unwrap_err();
        match err.kind() {
            ErrorKind::NestedAlternate => {}
            other => panic!("unexpected error kind: {:?}", other),
        }
    }

    #[test]
    fn test_empty_alternate_disallowed() {
        assert!(Glob::new("{}").is_err());
    }

    #[test]
    fn test_empty_alternate_allowed() {
        let g = GlobBuilder::new("{}")
            .empty_alternates(true)
            .build()
            .unwrap();
        // Empty alternate matches empty string at that position.
        assert!(g.is_match(Path::new("")));
    }

    #[test]
    fn test_question_literal_separator() {
        let g = GlobBuilder::new("a?b")
            .literal_separator(true)
            .build()
            .unwrap();
        assert!(g.is_match(Path::new("axb")));
        assert!(!g.is_match(Path::new("a/b")));
    }

    #[test]
    fn test_matches_candidate_into() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.txt").unwrap());
        let set = builder.build().unwrap();

        let c = Candidate::new(&Path::new("lib.rs"));
        let mut matches = Vec::new();
        set.matches_candidate_into(&c, &mut matches);
        assert_eq!(matches, vec![0]);

        // Reuse the vec.
        matches.clear();
        let c2 = Candidate::new(&Path::new("readme.txt"));
        set.matches_candidate_into(&c2, &mut matches);
        assert_eq!(matches, vec![1]);
    }
}
