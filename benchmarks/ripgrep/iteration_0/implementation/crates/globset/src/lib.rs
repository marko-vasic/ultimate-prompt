//! A crate for fast glob pattern matching.
//!
//! This crate provides cross-platform glob matching with support for common
//! glob syntax including `*`, `**`, `?`, character classes, alternation, and
//! escape sequences. Globs are compiled to regular expressions and matched
//! using `regex-automata`.
//!
//! # Example
//!
//! ```
//! use globset::{Glob, GlobSet, GlobSetBuilder};
//!
//! let glob = Glob::new("*.rs").unwrap();
//! assert!(glob.is_match("foo.rs"));
//! assert!(!glob.is_match("foo.txt"));
//!
//! let mut builder = GlobSetBuilder::new();
//! builder.add(Glob::new("*.rs").unwrap());
//! builder.add(Glob::new("*.toml").unwrap());
//! let set = builder.build().unwrap();
//! assert!(set.is_match("lib.rs"));
//! assert_eq!(set.matches("lib.rs"), vec![0]);
//! ```

use std::fmt;
use std::path::Path;

use regex_automata::meta::Regex;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// An error that occurs when parsing or compiling a glob pattern.
#[derive(Clone, Debug)]
pub struct GlobSetError {
    message: String,
}

impl GlobSetError {
    /// Create a new `GlobSetError` with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        GlobSetError {
            message: message.into(),
        }
    }
}

impl fmt::Display for GlobSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "glob error: {}", self.message)
    }
}

impl std::error::Error for GlobSetError {}

// ---------------------------------------------------------------------------
// Candidate – pre-processed path for efficient matching
// ---------------------------------------------------------------------------

/// A pre-processed path for efficient matching against multiple globs.
///
/// A `Candidate` normalizes path separators and pre-computes the basename
/// offset so that basename-only globs can be tested without repeated work.
#[derive(Clone, Debug)]
pub struct Candidate<'a> {
    /// The full path as bytes, with separators normalized to `/`.
    path: Vec<u8>,
    /// The byte offset where the basename begins within `path`.
    basename_offset: usize,
    /// Lifetime tie to the original path (not strictly needed, but keeps
    /// the API consistent with the real `globset` crate).
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Candidate<'a> {
    /// Create a new `Candidate` from the given path.
    ///
    /// The path is converted to bytes and all backslash separators are
    /// normalised to forward slashes. The basename byte offset is computed
    /// once so that it can be reused across many glob matches.
    pub fn new<P: AsRef<Path> + ?Sized>(path: &'a P) -> Candidate<'a> {
        let path_str = path.as_ref().to_string_lossy();
        let normalized: Vec<u8> = path_str
            .as_bytes()
            .iter()
            .map(|&b| if b == b'\\' { b'/' } else { b })
            .collect();
        let basename_offset = normalized
            .iter()
            .rposition(|&b| b == b'/')
            .map(|i| i + 1)
            .unwrap_or(0);
        Candidate {
            path: normalized,
            basename_offset,
            _marker: std::marker::PhantomData,
        }
    }

    /// Returns the full normalised path as a byte slice.
    #[inline]
    pub fn path_bytes(&self) -> &[u8] {
        &self.path
    }

    /// Returns the basename portion of the path as a byte slice.
    #[inline]
    pub fn basename_bytes(&self) -> &[u8] {
        &self.path[self.basename_offset..]
    }
}

// ---------------------------------------------------------------------------
// Glob-to-regex translation
// ---------------------------------------------------------------------------

/// Translate a glob pattern to a regular expression string.
///
/// # Supported syntax
///
/// | Pattern   | Meaning |
/// |-----------|---------|
/// | `*`       | Match anything except `/` |
/// | `**`      | Match anything including `/` |
/// | `?`       | Match any single character except `/` |
/// | `[abc]`   | Character class |
/// | `[!abc]` / `[^abc]` | Negated character class |
/// | `{a,b,c}` | Alternation |
/// | `\x`      | Escape character `x` |
///
/// # Path matching rules
///
/// - If the glob contains no `/`, it is a *basename-only* pattern and will
///   only be tested against the filename component of a path.
/// - If the glob contains `/`, it matches the *full* normalised path.
/// - A leading `**/` matches any number of leading path components.
/// - A trailing `/**` matches any number of trailing path components.
pub fn glob_to_regex(glob: &str) -> Result<String, GlobSetError> {
    let mut re = String::with_capacity(glob.len() * 2);
    re.push('^');

    let chars: Vec<char> = glob.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            '*' => {
                // Check for `**`
                if i + 1 < len && chars[i + 1] == '*' {
                    // `**` — match everything including `/`
                    // Consume any trailing `/` that is part of `**/`
                    i += 2;
                    if i < len && chars[i] == '/' {
                        // `**/` at the start or in the middle
                        re.push_str("(?:.*/)?");
                        i += 1;
                    } else {
                        // `**` at the end or standalone
                        re.push_str(".*");
                    }
                } else {
                    // Single `*` — match anything except `/`
                    re.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                re.push_str("[^/]");
                i += 1;
            }
            '[' => {
                // Character class
                i += 1;
                let mut class = String::new();
                class.push('[');
                if i < len && (chars[i] == '!' || chars[i] == '^') {
                    class.push('^');
                    i += 1;
                }
                // Allow `]` as the first character in the class
                if i < len && chars[i] == ']' {
                    class.push(']');
                    i += 1;
                }
                let mut found_close = false;
                while i < len {
                    if chars[i] == ']' {
                        class.push(']');
                        i += 1;
                        found_close = true;
                        break;
                    } else if chars[i] == '\\' && i + 1 < len {
                        class.push('\\');
                        class.push(chars[i + 1]);
                        i += 2;
                    } else {
                        class.push(chars[i]);
                        i += 1;
                    }
                }
                if !found_close {
                    return Err(GlobSetError::new(format!(
                        "unclosed character class in glob: `{}`",
                        glob
                    )));
                }
                re.push_str(&class);
            }
            '{' => {
                // Alternation: `{a,b,c}` -> `(?:a|b|c)`
                i += 1;
                let mut alt = String::new();
                alt.push_str("(?:");
                let mut depth = 1u32;
                let mut first = true;
                while i < len && depth > 0 {
                    match chars[i] {
                        '{' => {
                            depth += 1;
                            alt.push('{');
                            i += 1;
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                            alt.push('}');
                            i += 1;
                        }
                        ',' if depth == 1 => {
                            alt.push('|');
                            first = false;
                            i += 1;
                        }
                        '\\' if i + 1 < len => {
                            alt.push('\\');
                            alt.push(chars[i + 1]);
                            i += 2;
                        }
                        '*' => {
                            if i + 1 < len && chars[i + 1] == '*' {
                                alt.push_str(".*");
                                i += 2;
                            } else {
                                alt.push_str("[^/]*");
                                i += 1;
                            }
                        }
                        '?' => {
                            alt.push_str("[^/]");
                            i += 1;
                        }
                        c => {
                            // Escape regex-special characters
                            if is_regex_meta(c) {
                                alt.push('\\');
                            }
                            alt.push(c);
                            i += 1;
                        }
                    }
                }
                if depth != 0 {
                    return Err(GlobSetError::new(format!(
                        "unclosed alternation '{{' in glob: `{}`",
                        glob
                    )));
                }
                let _ = first; // suppress unused warning
                alt.push(')');
                re.push_str(&alt);
            }
            '\\' => {
                // Escape next character
                i += 1;
                if i < len {
                    if is_regex_meta(chars[i]) {
                        re.push('\\');
                    }
                    re.push(chars[i]);
                    i += 1;
                } else {
                    return Err(GlobSetError::new(format!(
                        "trailing backslash in glob: `{}`",
                        glob
                    )));
                }
            }
            '.' => {
                re.push_str("\\.");
                i += 1;
            }
            c => {
                if is_regex_meta(c) {
                    re.push('\\');
                }
                re.push(c);
                i += 1;
            }
        }
    }

    re.push('$');
    Ok(re)
}

/// Returns `true` if `c` is a regex metacharacter that needs escaping.
fn is_regex_meta(c: char) -> bool {
    matches!(
        c,
        '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']'
            | '{' | '}' | '^' | '$' | '\\' | '@' | '%'
    )
}

/// Returns `true` if the glob pattern should only match the basename of a
/// path rather than the full path.
///
/// A glob is basename-only when it does not contain any `/` characters,
/// unless the only slashes are part of `**/` or `/**` constructs at the
/// very start or end.
fn is_basename_only(glob: &str) -> bool {
    // Simple heuristic: if there are no `/` in the glob at all, it is
    // basename-only. Otherwise it matches the full path.
    !glob.contains('/')
}

// ---------------------------------------------------------------------------
// CompiledGlob – internal helper
// ---------------------------------------------------------------------------

/// A single compiled glob ready for matching.
#[derive(Clone, Debug)]
struct CompiledGlob {
    /// The original glob pattern string.
    pattern: String,
    /// The regex string this glob compiles to.
    regex_str: String,
    /// The compiled regex.
    regex: Regex,
    /// `true` if this glob should only match the basename of a path.
    basename_only: bool,
}

impl CompiledGlob {
    fn new(glob: &str) -> Result<Self, GlobSetError> {
        let basename_only = is_basename_only(glob);
        let regex_str = glob_to_regex(glob)?;
        let regex = Regex::new(&regex_str).map_err(|e| {
            GlobSetError::new(format!(
                "failed to compile regex `{}` for glob `{}`: {}",
                regex_str, glob, e
            ))
        })?;
        Ok(CompiledGlob {
            pattern: glob.to_string(),
            regex_str,
            regex,
            basename_only,
        })
    }

    /// Test whether the given candidate matches this glob.
    #[inline]
    fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        let haystack = if self.basename_only {
            candidate.basename_bytes()
        } else {
            candidate.path_bytes()
        };
        self.regex.is_match(haystack)
    }
}

// ---------------------------------------------------------------------------
// Glob – public single-glob API
// ---------------------------------------------------------------------------

/// A single compiled glob pattern.
///
/// A `Glob` can be used on its own to match paths, or it can be added to a
/// [`GlobSetBuilder`] to build a [`GlobSet`] that matches multiple patterns
/// simultaneously.
///
/// # Example
///
/// ```
/// use globset::Glob;
///
/// let glob = Glob::new("*.rs").unwrap();
/// assert!(glob.is_match("main.rs"));
/// assert!(!glob.is_match("main.go"));
/// ```
#[derive(Clone, Debug)]
pub struct Glob {
    compiled: CompiledGlob,
}

impl Glob {
    /// Parse and compile a glob pattern.
    ///
    /// Returns an error if the pattern is invalid.
    pub fn new(glob: &str) -> Result<Glob, GlobSetError> {
        let compiled = CompiledGlob::new(glob)?;
        log::trace!("compiled glob `{}` to regex `{}`", glob, compiled.regex_str);
        Ok(Glob { compiled })
    }

    /// Returns the regex string that this glob compiles to.
    pub fn regex(&self) -> &str {
        &self.compiled.regex_str
    }

    /// Returns the original glob pattern string.
    pub fn glob(&self) -> &str {
        &self.compiled.pattern
    }

    /// Test whether the given path matches this glob.
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        let candidate = Candidate::new(path.as_ref());
        self.is_match_candidate(&candidate)
    }

    /// Test whether a pre-processed [`Candidate`] matches this glob.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        self.compiled.is_match_candidate(candidate)
    }
}

// ---------------------------------------------------------------------------
// GlobSet – match multiple globs simultaneously
// ---------------------------------------------------------------------------

/// A set of glob patterns that can be matched against a path simultaneously.
///
/// Internally each glob is compiled to its own regex. Matching iterates over
/// all compiled globs and tests each one. This is simple, correct, and fast
/// enough for the typical number of patterns used in tools like ripgrep.
///
/// # Example
///
/// ```
/// use globset::{Glob, GlobSet, GlobSetBuilder};
///
/// let mut builder = GlobSetBuilder::new();
/// builder.add(Glob::new("*.rs").unwrap());
/// builder.add(Glob::new("*.toml").unwrap());
/// let set = builder.build().unwrap();
///
/// assert!(set.is_match("lib.rs"));
/// assert!(set.is_match("Cargo.toml"));
/// assert!(!set.is_match("README.md"));
///
/// assert_eq!(set.matches("lib.rs"), vec![0]);
/// assert_eq!(set.matches("Cargo.toml"), vec![1]);
/// ```
#[derive(Clone, Debug)]
pub struct GlobSet {
    globs: Vec<CompiledGlob>,
}

impl GlobSet {
    /// Create an empty `GlobSet` that matches nothing.
    pub fn empty() -> GlobSet {
        GlobSet { globs: Vec::new() }
    }

    /// Returns `true` if this set contains no glob patterns.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty()
    }

    /// Returns the number of glob patterns in this set.
    #[inline]
    pub fn len(&self) -> usize {
        self.globs.len()
    }

    /// Returns `true` if any glob in this set matches the given path.
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        let candidate = Candidate::new(path.as_ref());
        self.is_match_candidate(&candidate)
    }

    /// Returns `true` if any glob in this set matches the given candidate.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        for glob in &self.globs {
            if glob.is_match_candidate(candidate) {
                return true;
            }
        }
        false
    }

    /// Returns the indices of all globs that match the given path.
    pub fn matches<P: AsRef<Path>>(&self, path: P) -> Vec<usize> {
        let candidate = Candidate::new(path.as_ref());
        self.matches_candidate(&candidate)
    }

    /// Returns the indices of all globs that match the given candidate.
    pub fn matches_candidate(&self, candidate: &Candidate<'_>) -> Vec<usize> {
        let mut result = Vec::new();
        self.matches_candidate_into(candidate, &mut result);
        result
    }

    /// Appends the indices of all matching globs into the provided vector.
    ///
    /// The vector is **not** cleared before appending.
    pub fn matches_candidate_into(
        &self,
        candidate: &Candidate<'_>,
        into: &mut Vec<usize>,
    ) {
        for (i, glob) in self.globs.iter().enumerate() {
            if glob.is_match_candidate(candidate) {
                into.push(i);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GlobSetBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a [`GlobSet`].
///
/// # Example
///
/// ```
/// use globset::{Glob, GlobSetBuilder};
///
/// let mut builder = GlobSetBuilder::new();
/// builder.add(Glob::new("*.rs").unwrap());
/// builder.add(Glob::new("src/**/*.rs").unwrap());
/// let set = builder.build().unwrap();
/// assert!(set.is_match("main.rs"));
/// ```
#[derive(Clone, Debug)]
pub struct GlobSetBuilder {
    globs: Vec<Glob>,
}

impl GlobSetBuilder {
    /// Create a new, empty `GlobSetBuilder`.
    pub fn new() -> GlobSetBuilder {
        GlobSetBuilder { globs: Vec::new() }
    }

    /// Add a compiled [`Glob`] to this builder.
    pub fn add(&mut self, glob: Glob) -> &mut GlobSetBuilder {
        self.globs.push(glob);
        self
    }

    /// Build a [`GlobSet`] from the globs added so far.
    ///
    /// This always succeeds since each individual glob was already validated
    /// at `Glob::new` time. The `Result` return type is kept for API
    /// compatibility and future-proofing.
    pub fn build(&self) -> Result<GlobSet, GlobSetError> {
        let compiled = self
            .globs
            .iter()
            .map(|g| g.compiled.clone())
            .collect();
        Ok(GlobSet { globs: compiled })
    }
}

impl Default for GlobSetBuilder {
    fn default() -> Self {
        GlobSetBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- glob_to_regex tests ---

    #[test]
    fn test_star_matches_filename() {
        let glob = Glob::new("*.rs").unwrap();
        assert!(glob.is_match("foo.rs"));
        assert!(glob.is_match("bar.rs"));
        assert!(!glob.is_match("foo.txt"));
        // basename-only: should match even with directory prefix
        assert!(glob.is_match("src/foo.rs"));
    }

    #[test]
    fn test_star_does_not_cross_separator() {
        let glob = Glob::new("src/*").unwrap();
        // Full-path glob (contains `/`)
        assert!(glob.is_match("src/foo.rs"));
        assert!(!glob.is_match("src/sub/foo.rs"));
    }

    #[test]
    fn test_double_star_recursive() {
        let glob = Glob::new("src/**/*.rs").unwrap();
        assert!(glob.is_match("src/foo.rs"));
        assert!(glob.is_match("src/a/b/c/foo.rs"));
        assert!(!glob.is_match("tests/foo.rs"));
    }

    #[test]
    fn test_question_mark() {
        let glob = Glob::new("?.rs").unwrap();
        assert!(glob.is_match("a.rs"));
        assert!(!glob.is_match("ab.rs"));
        assert!(!glob.is_match("/.rs"));
    }

    #[test]
    fn test_character_class() {
        let glob = Glob::new("[abc].rs").unwrap();
        assert!(glob.is_match("a.rs"));
        assert!(glob.is_match("b.rs"));
        assert!(!glob.is_match("d.rs"));
    }

    #[test]
    fn test_negated_character_class() {
        let glob = Glob::new("[!abc].rs").unwrap();
        assert!(!glob.is_match("a.rs"));
        assert!(glob.is_match("d.rs"));
    }

    #[test]
    fn test_alternation() {
        let glob = Glob::new("*.{rs,toml}").unwrap();
        assert!(glob.is_match("foo.rs"));
        assert!(glob.is_match("foo.toml"));
        assert!(!glob.is_match("foo.txt"));
    }

    #[test]
    fn test_escape() {
        let glob = Glob::new("\\*.rs").unwrap();
        assert!(glob.is_match("*.rs"));
        assert!(!glob.is_match("foo.rs"));
    }

    #[test]
    fn test_basename_only_detection() {
        assert!(is_basename_only("*.rs"));
        assert!(is_basename_only("foo.txt"));
        assert!(!is_basename_only("src/*.rs"));
        assert!(!is_basename_only("**/foo.rs"));
    }

    #[test]
    fn test_leading_double_star() {
        let glob = Glob::new("**/foo.rs").unwrap();
        assert!(glob.is_match("foo.rs"));
        assert!(glob.is_match("src/foo.rs"));
        assert!(glob.is_match("a/b/c/foo.rs"));
        assert!(!glob.is_match("foo.txt"));
    }

    #[test]
    fn test_trailing_double_star() {
        let glob = Glob::new("src/**").unwrap();
        assert!(glob.is_match("src/foo.rs"));
        assert!(glob.is_match("src/a/b/c.txt"));
        assert!(!glob.is_match("tests/foo.rs"));
    }

    // --- Candidate tests ---

    #[test]
    fn test_candidate_basename() {
        let c = Candidate::new("src/lib.rs");
        assert_eq!(c.basename_bytes(), b"lib.rs");
        assert_eq!(c.path_bytes(), b"src/lib.rs");
    }

    #[test]
    fn test_candidate_no_separator() {
        let c = Candidate::new("lib.rs");
        assert_eq!(c.basename_bytes(), b"lib.rs");
        assert_eq!(c.path_bytes(), b"lib.rs");
    }

    #[test]
    fn test_candidate_backslash_normalized() {
        let c = Candidate::new("src\\sub\\lib.rs");
        assert_eq!(c.path_bytes(), b"src/sub/lib.rs");
        assert_eq!(c.basename_bytes(), b"lib.rs");
    }

    // --- GlobSet tests ---

    #[test]
    fn test_glob_set_empty() {
        let set = GlobSet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.is_match("anything"));
    }

    #[test]
    fn test_glob_set_matches() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        builder.add(Glob::new("Makefile").unwrap());
        let set = builder.build().unwrap();

        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());

        assert!(set.is_match("foo.rs"));
        assert!(set.is_match("Cargo.toml"));
        assert!(set.is_match("Makefile"));
        assert!(!set.is_match("README.md"));

        assert_eq!(set.matches("foo.rs"), vec![0]);
        assert_eq!(set.matches("Cargo.toml"), vec![1]);
        assert_eq!(set.matches("Makefile"), vec![2]);
        assert!(set.matches("README.md").is_empty());
    }

    #[test]
    fn test_glob_set_multiple_matches() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("lib.*").unwrap());
        let set = builder.build().unwrap();

        // "lib.rs" matches both patterns
        let m = set.matches("lib.rs");
        assert_eq!(m, vec![0, 1]);
    }

    #[test]
    fn test_glob_set_candidate_reuse() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        let set = builder.build().unwrap();

        let c = Candidate::new("src/lib.rs");
        assert!(set.is_match_candidate(&c));
        assert_eq!(set.matches_candidate(&c), vec![0]);
    }

    #[test]
    fn test_glob_set_matches_candidate_into() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("lib.*").unwrap());
        let set = builder.build().unwrap();

        let c = Candidate::new("lib.rs");
        let mut results = Vec::new();
        set.matches_candidate_into(&c, &mut results);
        assert_eq!(results, vec![0, 1]);
    }

    // --- Error tests ---

    #[test]
    fn test_unclosed_bracket() {
        assert!(Glob::new("[abc").is_err());
    }

    #[test]
    fn test_trailing_backslash() {
        assert!(Glob::new("foo\\").is_err());
    }

    #[test]
    fn test_unclosed_brace() {
        assert!(Glob::new("{a,b").is_err());
    }

    #[test]
    fn test_error_display() {
        let err = GlobSetError::new("test error");
        assert_eq!(err.to_string(), "glob error: test error");
    }

    #[test]
    fn test_glob_regex_accessor() {
        let glob = Glob::new("*.rs").unwrap();
        let re = glob.regex();
        assert!(re.starts_with('^'));
        assert!(re.ends_with('$'));
    }

    #[test]
    fn test_glob_glob_accessor() {
        let glob = Glob::new("*.rs").unwrap();
        assert_eq!(glob.glob(), "*.rs");
    }

    // --- Edge case tests ---

    #[test]
    fn test_literal_dot_in_extension() {
        let glob = Glob::new("*.tar.gz").unwrap();
        assert!(glob.is_match("archive.tar.gz"));
        assert!(!glob.is_match("archive.tar.bz2"));
    }

    #[test]
    fn test_double_star_middle() {
        let glob = Glob::new("src/**/test.rs").unwrap();
        assert!(glob.is_match("src/test.rs"));
        assert!(glob.is_match("src/foo/test.rs"));
        assert!(glob.is_match("src/foo/bar/test.rs"));
        assert!(!glob.is_match("lib/test.rs"));
    }

    #[test]
    fn test_empty_glob() {
        let glob = Glob::new("").unwrap();
        assert!(glob.is_match(""));
        assert!(!glob.is_match("anything"));
    }

    #[test]
    fn test_exact_match() {
        let glob = Glob::new("Makefile").unwrap();
        assert!(glob.is_match("Makefile"));
        // basename-only
        assert!(glob.is_match("src/Makefile"));
        assert!(!glob.is_match("Makefile.bak"));
    }
}
