/*!
Cross-platform glob pattern matching.

The `globset` crate provides glob pattern matching with support for multiple
patterns. It converts glob patterns into regular expressions and uses the
`regex-automata` crate for efficient matching.

# Glob Pattern Syntax

- `*` matches any sequence of characters except path separators
- `**` matches any sequence of characters including path separators (recursive)
- `?` matches any single character except a path separator
- `[abc]` matches any character in the set
- `[!abc]` or `[^abc]` matches any character not in the set
- `[a-z]` matches any character in the range
- `{a,b,c}` matches any of the comma-separated alternatives
- `\` escapes the next character

# Matching Behavior

If a glob pattern contains no path separator (`/`), it matches only against
the basename of a path. If it contains a separator or uses `**`, it matches
against the full path.

A pattern ending with `/` only matches directories.

# Examples

```
use globset::Glob;

let glob = Glob::new("*.rs").unwrap();
let matcher = glob.compile_matcher();
assert!(matcher.is_match("foo.rs"));
assert!(matcher.is_match("src/main.rs"));
assert!(!matcher.is_match("foo.txt"));
```

```
use globset::{GlobSetBuilder, Glob};

let mut builder = GlobSetBuilder::new();
builder.add(Glob::new("*.rs").unwrap());
builder.add(Glob::new("*.toml").unwrap());
let set = builder.build().unwrap();

assert!(set.is_match("main.rs"));
assert!(set.is_match("Cargo.toml"));
assert!(!set.is_match("readme.md"));
assert_eq!(set.matches("main.rs"), vec![0]);
assert_eq!(set.matches("Cargo.toml"), vec![1]);
```
*/

mod glob;

use std::fmt;

use bstr::ByteSlice;

// Re-export glob types
pub use crate::glob::Glob;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// The kind of error that can occur in globset.
#[derive(Clone, Debug)]
enum ErrorKind {
    /// An invalid glob pattern.
    InvalidGlob(String),
    /// An error compiling a regex.
    Regex(String),
}

/// An error that can occur when parsing or compiling glob patterns.
#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    /// Create a new glob parse error.
    pub(crate) fn glob(pattern: &str, msg: &str) -> Error {
        Error {
            kind: ErrorKind::InvalidGlob(format!(
                "invalid glob pattern '{}': {}",
                pattern, msg
            )),
        }
    }

    /// Create a new regex error.
    pub(crate) fn regex(msg: &str) -> Error {
        Error {
            kind: ErrorKind::Regex(msg.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::InvalidGlob(msg) => write!(f, "{msg}"),
            ErrorKind::Regex(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

// ---------------------------------------------------------------------------
// Candidate
// ---------------------------------------------------------------------------

/// A pre-processed path for efficient glob matching.
///
/// A `Candidate` stores a path and its pre-computed basename so that
/// glob matching routines can avoid repeatedly computing the basename
/// for each pattern match.
///
/// # Example
///
/// ```
/// use globset::Candidate;
///
/// let candidate = Candidate::new("src/main.rs");
/// assert_eq!(candidate.path(), b"src/main.rs");
/// assert_eq!(candidate.basename(), b"main.rs");
/// ```
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    /// The full path as bytes.
    path: &'a [u8],
    /// The basename portion of the path.
    basename: &'a [u8],
}

impl<'a> Candidate<'a> {
    /// Create a new candidate from a path string.
    ///
    /// The path is normalized to use forward slashes on all platforms
    /// for consistent matching.
    pub fn new(path: &'a str) -> Candidate<'a> {
        let path_bytes = path.as_bytes();
        let basename = match path_bytes.rfind_byte(b'/') {
            Some(pos) => &path_bytes[pos + 1..],
            None => {
                // Also check for Windows backslash
                match path_bytes.rfind_byte(b'\\') {
                    Some(pos) => &path_bytes[pos + 1..],
                    None => path_bytes,
                }
            }
        };
        Candidate {
            path: path_bytes,
            basename,
        }
    }

    /// Returns the full path as a byte slice.
    pub fn path(&self) -> &[u8] {
        self.path
    }

    /// Returns the basename of the path as a byte slice.
    pub fn basename(&self) -> &[u8] {
        self.basename
    }
}

// ---------------------------------------------------------------------------
// GlobMatcher
// ---------------------------------------------------------------------------

/// A compiled glob pattern matcher.
///
/// A `GlobMatcher` is created by calling [`Glob::compile_matcher`]. It
/// wraps a compiled regular expression and provides efficient path matching.
///
/// # Example
///
/// ```
/// use globset::Glob;
///
/// let glob = Glob::new("*.rs").unwrap();
/// let matcher = glob.compile_matcher();
/// assert!(matcher.is_match("foo.rs"));
/// ```
pub struct GlobMatcher {
    /// The original glob that produced this matcher.
    glob: Glob,
    /// The compiled regex used for matching.
    re: regex_automata::meta::Regex,
}

impl GlobMatcher {
    /// Returns `true` if the given path matches this glob pattern.
    ///
    /// The path is matched using forward slashes as separators on all
    /// platforms.
    pub fn is_match(&self, path: &str) -> bool {
        let normalized = normalize_path(path);
        self.re.is_match(normalized.as_bytes())
    }

    /// Returns `true` if the given pre-processed candidate matches this
    /// glob pattern.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        let path = normalize_path_bytes(candidate.path());
        self.re.is_match(&path)
    }

    /// Returns a reference to the underlying `Glob`.
    pub fn glob(&self) -> &Glob {
        &self.glob
    }
}

impl fmt::Debug for GlobMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobMatcher")
            .field("glob", &self.glob)
            .field("regex", &self.glob.regex())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// GlobSet
// ---------------------------------------------------------------------------

/// A set of glob patterns that can be matched simultaneously.
///
/// A `GlobSet` is built using a [`GlobSetBuilder`] and efficiently matches
/// a path against all contained patterns at once.
///
/// # Example
///
/// ```
/// use globset::{GlobSetBuilder, Glob};
///
/// let mut builder = GlobSetBuilder::new();
/// builder.add(Glob::new("*.rs").unwrap());
/// builder.add(Glob::new("*.toml").unwrap());
/// let set = builder.build().unwrap();
///
/// assert!(set.is_match("main.rs"));
/// assert!(set.is_match("Cargo.toml"));
/// assert!(!set.is_match("readme.md"));
/// ```
#[derive(Clone)]
pub struct GlobSet {
    /// The list of compiled globs and their regex matchers.
    globs: Vec<Glob>,
    /// Compiled regexes corresponding to each glob.
    regexes: Vec<regex_automata::meta::Regex>,
}

impl GlobSet {
    /// Returns an empty `GlobSet` that matches nothing.
    pub fn empty() -> GlobSet {
        GlobSet {
            globs: Vec::new(),
            regexes: Vec::new(),
        }
    }

    /// Returns `true` if this set contains no glob patterns.
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty()
    }

    /// Returns the number of glob patterns in this set.
    pub fn len(&self) -> usize {
        self.globs.len()
    }

    /// Returns `true` if the given path matches any glob in this set.
    pub fn is_match(&self, path: &str) -> bool {
        if self.is_empty() {
            return false;
        }
        let normalized = normalize_path(path);
        let path_bytes = normalized.as_bytes();
        for re in &self.regexes {
            if re.is_match(path_bytes) {
                return true;
            }
        }
        false
    }

    /// Returns `true` if the given candidate matches any glob in this set.
    pub fn is_match_candidate(&self, candidate: &Candidate<'_>) -> bool {
        if self.is_empty() {
            return false;
        }
        let path = normalize_path_bytes(candidate.path());
        for re in &self.regexes {
            if re.is_match(&path) {
                return true;
            }
        }
        false
    }

    /// Returns a list of indices of all globs that match the given path.
    ///
    /// The indices correspond to the order in which the globs were added
    /// to the builder.
    pub fn matches(&self, path: &str) -> Vec<usize> {
        if self.is_empty() {
            return Vec::new();
        }
        let normalized = normalize_path(path);
        let path_bytes = normalized.as_bytes();
        let mut result = Vec::new();
        for (i, re) in self.regexes.iter().enumerate() {
            if re.is_match(path_bytes) {
                result.push(i);
            }
        }
        result
    }

    /// Returns a list of indices of all globs that match the given candidate.
    pub fn matches_candidate(&self, candidate: &Candidate<'_>) -> Vec<usize> {
        if self.is_empty() {
            return Vec::new();
        }
        let path = normalize_path_bytes(candidate.path());
        let mut result = Vec::new();
        for (i, re) in self.regexes.iter().enumerate() {
            if re.is_match(&path) {
                result.push(i);
            }
        }
        result
    }
}

impl fmt::Debug for GlobSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobSet")
            .field("len", &self.globs.len())
            .field("globs", &self.globs)
            .finish()
    }
}

impl Default for GlobSet {
    fn default() -> GlobSet {
        GlobSet::empty()
    }
}

// ---------------------------------------------------------------------------
// GlobSetBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing a `GlobSet`.
///
/// # Example
///
/// ```
/// use globset::{GlobSetBuilder, Glob};
///
/// let mut builder = GlobSetBuilder::new();
/// builder.add(Glob::new("*.rs").unwrap());
/// builder.add(Glob::new("src/**/*.toml").unwrap());
/// let set = builder.build().unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct GlobSetBuilder {
    globs: Vec<Glob>,
}

impl GlobSetBuilder {
    /// Create a new empty builder.
    pub fn new() -> GlobSetBuilder {
        GlobSetBuilder { globs: Vec::new() }
    }

    /// Add a glob pattern to this builder.
    pub fn add(&mut self, glob: Glob) -> &mut GlobSetBuilder {
        self.globs.push(glob);
        self
    }

    /// Build the `GlobSet` from the added patterns.
    ///
    /// Returns an error if any of the compiled regex patterns fail to
    /// compile (this should not typically happen since globs are validated
    /// at parse time).
    pub fn build(&self) -> Result<GlobSet, Error> {
        let mut regexes = Vec::with_capacity(self.globs.len());
        for glob in &self.globs {
            let re = regex_automata::meta::Regex::new(glob.regex())
                .map_err(|e| {
                    Error::regex(&format!(
                        "error compiling regex for glob '{}': {}",
                        glob.glob(),
                        e
                    ))
                })?;
            regexes.push(re);
        }
        Ok(GlobSet {
            globs: self.globs.clone(),
            regexes,
        })
    }
}

impl Default for GlobSetBuilder {
    fn default() -> GlobSetBuilder {
        GlobSetBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Normalize a path string to use forward slashes.
fn normalize_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

/// Normalize path bytes to use forward slashes.
fn normalize_path_bytes(path: &[u8]) -> Vec<u8> {
    if cfg!(windows) {
        path.iter()
            .map(|&b| if b == b'\\' { b'/' } else { b })
            .collect()
    } else {
        path.to_vec()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Glob tests --

    #[test]
    fn test_glob_star_basename() {
        let glob = Glob::new("*.rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("foo.rs"));
        assert!(matcher.is_match("src/foo.rs"));
        assert!(matcher.is_match("a/b/c/foo.rs"));
        assert!(!matcher.is_match("foo.txt"));
        assert!(!matcher.is_match("foo.rs.bak"));
    }

    #[test]
    fn test_glob_star_path() {
        let glob = Glob::new("src/*.rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("src/main.rs"));
        assert!(matcher.is_match("src/lib.rs"));
        assert!(!matcher.is_match("test/main.rs"));
        assert!(!matcher.is_match("src/sub/main.rs"));
    }

    #[test]
    fn test_glob_double_star() {
        let glob = Glob::new("**/*.rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("foo.rs"));
        assert!(matcher.is_match("src/foo.rs"));
        assert!(matcher.is_match("a/b/c/foo.rs"));
        assert!(!matcher.is_match("foo.txt"));
    }

    #[test]
    fn test_glob_double_star_suffix() {
        let glob = Glob::new("src/**").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("src/foo.rs"));
        assert!(matcher.is_match("src/a/b/c.rs"));
        assert!(matcher.is_match("src/"));
        assert!(!matcher.is_match("test/foo.rs"));
    }

    #[test]
    fn test_glob_question_mark() {
        let glob = Glob::new("?.rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("a.rs"));
        assert!(matcher.is_match("b.rs"));
        assert!(!matcher.is_match("ab.rs"));
        assert!(!matcher.is_match(".rs"));
    }

    #[test]
    fn test_glob_character_class() {
        let glob = Glob::new("[abc].rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("a.rs"));
        assert!(matcher.is_match("b.rs"));
        assert!(matcher.is_match("c.rs"));
        assert!(!matcher.is_match("d.rs"));
    }

    #[test]
    fn test_glob_negated_class() {
        let glob = Glob::new("[!abc].rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(!matcher.is_match("a.rs"));
        assert!(!matcher.is_match("b.rs"));
        assert!(matcher.is_match("d.rs"));
        assert!(matcher.is_match("x.rs"));
    }

    #[test]
    fn test_glob_alternation() {
        let glob = Glob::new("*.{rs,toml}").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("foo.rs"));
        assert!(matcher.is_match("foo.toml"));
        assert!(!matcher.is_match("foo.txt"));
    }

    #[test]
    fn test_glob_only_dir() {
        let glob = Glob::new("target/").unwrap();
        assert!(glob.is_only_dir());

        let glob = Glob::new("target").unwrap();
        assert!(!glob.is_only_dir());
    }

    #[test]
    fn test_glob_escaped_star() {
        let glob = Glob::new("\\*").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("*"));
        assert!(!matcher.is_match("foo"));
    }

    #[test]
    fn test_glob_literal() {
        let glob = Glob::new("Makefile").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("Makefile"));
        assert!(matcher.is_match("src/Makefile"));
        assert!(!matcher.is_match("Makefile.bak"));
    }

    #[test]
    fn test_glob_regex_method() {
        let glob = Glob::new("*.rs").unwrap();
        let regex = glob.regex();
        assert!(!regex.is_empty());
    }

    // -- Candidate tests --

    #[test]
    fn test_candidate_simple() {
        let c = Candidate::new("src/main.rs");
        assert_eq!(c.path(), b"src/main.rs");
        assert_eq!(c.basename(), b"main.rs");
    }

    #[test]
    fn test_candidate_no_separator() {
        let c = Candidate::new("main.rs");
        assert_eq!(c.path(), b"main.rs");
        assert_eq!(c.basename(), b"main.rs");
    }

    #[test]
    fn test_candidate_deep_path() {
        let c = Candidate::new("a/b/c/d/main.rs");
        assert_eq!(c.path(), b"a/b/c/d/main.rs");
        assert_eq!(c.basename(), b"main.rs");
    }

    // -- GlobMatcher candidate tests --

    #[test]
    fn test_matcher_candidate() {
        let glob = Glob::new("*.rs").unwrap();
        let matcher = glob.compile_matcher();
        let candidate = Candidate::new("src/main.rs");
        assert!(matcher.is_match_candidate(&candidate));

        let candidate2 = Candidate::new("readme.md");
        assert!(!matcher.is_match_candidate(&candidate2));
    }

    // -- GlobSet tests --

    #[test]
    fn test_globset_basic() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        let set = builder.build().unwrap();

        assert!(set.is_match("main.rs"));
        assert!(set.is_match("Cargo.toml"));
        assert!(!set.is_match("readme.md"));
    }

    #[test]
    fn test_globset_matches() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        builder.add(Glob::new("main.*").unwrap());
        let set = builder.build().unwrap();

        assert_eq!(set.matches("main.rs"), vec![0, 2]);
        assert_eq!(set.matches("Cargo.toml"), vec![1]);
        assert_eq!(set.matches("readme.md"), Vec::<usize>::new());
    }

    #[test]
    fn test_globset_empty() {
        let set = GlobSet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.is_match("anything"));
        assert!(set.matches("anything").is_empty());
    }

    #[test]
    fn test_globset_candidate() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let set = builder.build().unwrap();

        let candidate = Candidate::new("src/main.rs");
        assert!(set.is_match_candidate(&candidate));

        let candidate2 = Candidate::new("readme.md");
        assert!(!set.is_match_candidate(&candidate2));
    }

    #[test]
    fn test_globset_matches_candidate() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("main.*").unwrap());
        let set = builder.build().unwrap();

        let candidate = Candidate::new("main.rs");
        assert_eq!(set.matches_candidate(&candidate), vec![0, 1]);
    }

    #[test]
    fn test_globset_len() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        builder.add(Glob::new("*.toml").unwrap());
        let set = builder.build().unwrap();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
    }

    // -- Edge case tests --

    #[test]
    fn test_glob_empty_pattern() {
        let glob = Glob::new("").unwrap();
        let matcher = glob.compile_matcher();
        // Empty pattern matches empty string but also acts as basename match
        assert!(matcher.is_match(""));
    }

    #[test]
    fn test_glob_star_matches_empty() {
        let glob = Glob::new("*").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match(""));
        assert!(matcher.is_match("bar.rs"));
    }

    #[test]
    fn test_glob_double_star_standalone() {
        let glob = Glob::new("**").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("foo/bar"));
        assert!(matcher.is_match("a/b/c/d"));
    }

    #[test]
    fn test_glob_character_range() {
        let glob = Glob::new("[a-z].txt").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("a.txt"));
        assert!(matcher.is_match("m.txt"));
        assert!(matcher.is_match("z.txt"));
        assert!(!matcher.is_match("A.txt"));
        assert!(!matcher.is_match("1.txt"));
    }

    #[test]
    fn test_glob_double_star_middle() {
        let glob = Glob::new("src/**/*.rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(matcher.is_match("src/main.rs"));
        assert!(matcher.is_match("src/a/b/c.rs"));
        assert!(!matcher.is_match("test/main.rs"));
    }

    #[test]
    fn test_error_display() {
        let err = Error::glob("test[", "unclosed bracket");
        assert!(err.to_string().contains("test["));
        assert!(err.to_string().contains("unclosed bracket"));

        let err = Error::regex("bad regex");
        assert!(err.to_string().contains("bad regex"));
    }

    #[test]
    fn test_glob_display() {
        let glob = Glob::new("*.rs").unwrap();
        assert_eq!(format!("{glob}"), "*.rs");
    }

    #[test]
    fn test_glob_caret_negation() {
        let glob = Glob::new("[^abc].rs").unwrap();
        let matcher = glob.compile_matcher();
        assert!(!matcher.is_match("a.rs"));
        assert!(matcher.is_match("d.rs"));
    }
}
