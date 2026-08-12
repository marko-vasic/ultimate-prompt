//! Gitignore pattern parsing and matching.
//!
//! This module provides the ability to parse gitignore files and match
//! file paths against the patterns therein. The matching semantics follow
//! the specification described in `gitignore(5)`.

use std::fs::File;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};

use crate::Error;

/// A result from matching a path against a gitignore rule.
///
/// This includes the glob pattern that matched along with its source
/// location (file path and line number).
#[derive(Clone, Debug)]
pub struct GlobResult {
    /// The glob pattern that matched.
    glob: String,
    /// The file path from which this pattern was read, if any.
    from: Option<PathBuf>,
    /// The line number in the file (1-indexed).
    line: u64,
}

impl GlobResult {
    /// Returns the glob pattern string that produced this match.
    pub fn glob(&self) -> &str {
        &self.glob
    }

    /// Returns the path of the file this pattern came from, if any.
    pub fn from(&self) -> Option<&Path> {
        self.from.as_deref()
    }

    /// Returns the line number where this pattern was defined.
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// The result of matching a path against a set of gitignore rules.
///
/// The lifetime `'a` is tied to the `Gitignore` value that produced
/// this match.
#[derive(Clone, Debug)]
pub enum Match<'a> {
    /// The path did not match any rule.
    None,
    /// The path matched an ignore rule (should be excluded).
    Ignore(&'a GlobResult),
    /// The path matched a negated (whitelisted) rule (should be included).
    Whitelist(&'a GlobResult),
}

impl<'a> Match<'a> {
    /// Returns true if this match indicates the path should be ignored.
    pub fn is_ignore(&self) -> bool {
        matches!(self, Match::Ignore(_))
    }

    /// Returns true if this match indicates the path is whitelisted.
    pub fn is_whitelist(&self) -> bool {
        matches!(self, Match::Whitelist(_))
    }

    /// Returns true if there was no match.
    pub fn is_none(&self) -> bool {
        matches!(self, Match::None)
    }

    /// Invert the match result: swap Ignore and Whitelist.
    pub fn invert(self) -> Match<'a> {
        match self {
            Match::None => Match::None,
            Match::Ignore(g) => Match::Whitelist(g),
            Match::Whitelist(g) => Match::Ignore(g),
        }
    }
}

/// A single parsed gitignore rule.
#[derive(Clone, Debug)]
struct GitignoreRule {
    /// The original pattern text.
    original: String,
    /// The glob pattern (converted for globset).
    glob: String,
    /// Whether this is a negated pattern (whitelist).
    is_whitelist: bool,
    /// Whether this pattern only matches directories.
    is_only_dir: bool,
    /// Source file, if any.
    from: Option<PathBuf>,
    /// Line number in the source file.
    line: u64,
}

/// A compiled set of gitignore rules from a single gitignore file.
///
/// # Example
///
/// ```no_run
/// use ignore::gitignore::Gitignore;
/// use std::path::Path;
///
/// let (gi, err) = Gitignore::from_path(Path::new(".gitignore"));
/// if let Some(err) = err {
///     eprintln!("warning: {}", err);
/// }
/// // Check if a file should be ignored
/// let m = gi.matched(Path::new("target/debug/foo"), false);
/// assert!(m.is_ignore());
/// ```
#[derive(Clone, Debug)]
pub struct Gitignore {
    /// The root directory this gitignore applies to.
    root: PathBuf,
    /// The compiled rules.
    rules: Vec<GitignoreRule>,
    /// The GlobResult records corresponding to each rule.
    globs: Vec<GlobResult>,
    /// The compiled glob set for matching.
    set: GlobSet,
    /// Number of ignore (non-negated) patterns.
    num_ignores: u64,
    /// Number of whitelist (negated) patterns.
    num_whitelists: u64,
}

impl Gitignore {
    /// Create a new empty gitignore with the given root.
    pub fn empty() -> Gitignore {
        Gitignore {
            root: PathBuf::new(),
            rules: Vec::new(),
            globs: Vec::new(),
            set: GlobSet::empty(),
            num_ignores: 0,
            num_whitelists: 0,
        }
    }

    /// Parse gitignore rules from a file path.
    ///
    /// Returns the compiled `Gitignore` and an optional error. The gitignore
    /// is still usable even when an error is returned (partial parse).
    pub fn from_path(path: &Path) -> (Gitignore, Option<Error>) {
        let root = path.parent().unwrap_or(Path::new(""));
        let mut builder = GitignoreBuilder::new(root);
        let err = builder.add(path);
        match builder.build() {
            Ok(gi) => (gi, err),
            Err(build_err) => {
                let gi = Gitignore::empty();
                (gi, Some(build_err))
            }
        }
    }

    /// Parse gitignore rules from a reader.
    ///
    /// The root path is used for anchoring patterns that start with `/`.
    pub fn from_reader<R: Read>(root: &Path, rdr: R) -> (Gitignore, Option<Error>) {
        let mut builder = GitignoreBuilder::new(root);
        let buf = io::BufReader::new(rdr);
        let mut errs = Vec::new();
        for (i, line) in buf.lines().enumerate() {
            match line {
                Ok(line) => {
                    if let Err(e) = builder.add_line(None, &line) {
                        errs.push(e);
                    }
                }
                Err(e) => {
                    errs.push(Error::io(root, e));
                }
            }
        }
        let first_err = if errs.is_empty() {
            None
        } else {
            Some(Error::multi(errs))
        };
        match builder.build() {
            Ok(gi) => (gi, first_err),
            Err(build_err) => (Gitignore::empty(), Some(build_err)),
        }
    }

    /// Check whether the given path matches any rule in this gitignore.
    ///
    /// The path should be relative to the gitignore root. `is_dir` should
    /// be set to true if the path refers to a directory.
    ///
    /// Rules are checked in reverse order (last matching rule wins), per
    /// the gitignore specification.
    pub fn matched<'a>(&'a self, path: &Path, is_dir: bool) -> Match<'a> {
        if self.rules.is_empty() {
            return Match::None;
        }

        // Normalize path: make it relative to root if possible, use forward slashes
        let rel_path = if path.is_absolute() {
            path.strip_prefix(&self.root)
                .unwrap_or(path)
                .to_path_buf()
        } else {
            path.to_path_buf()
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");

        // Find all matching globs
        let matches = self.set.matches(&path_str);
        if matches.is_empty() {
            return Match::None;
        }

        // Last matching rule wins (highest index)
        let last_idx = *matches.last().unwrap();
        let rule = &self.rules[last_idx];

        // If the rule is directory-only but the path is not a directory, skip
        if rule.is_only_dir && !is_dir {
            // Try to find an earlier match that doesn't require directory
            for &idx in matches.iter().rev().skip(1) {
                let r = &self.rules[idx];
                if !r.is_only_dir || is_dir {
                    if r.is_whitelist {
                        return Match::Whitelist(&self.globs[idx]);
                    } else {
                        return Match::Ignore(&self.globs[idx]);
                    }
                }
            }
            return Match::None;
        }

        if rule.is_whitelist {
            Match::Whitelist(&self.globs[last_idx])
        } else {
            Match::Ignore(&self.globs[last_idx])
        }
    }

    /// Returns true if this gitignore has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Returns the number of ignore (non-negated) patterns.
    pub fn num_ignores(&self) -> u64 {
        self.num_ignores
    }

    /// Returns the number of whitelist (negated) patterns.
    pub fn num_whitelists(&self) -> u64 {
        self.num_whitelists
    }

    /// Returns the root path of this gitignore.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A builder for constructing a `Gitignore` matcher.
///
/// # Example
///
/// ```
/// use ignore::gitignore::GitignoreBuilder;
/// use std::path::Path;
///
/// let mut builder = GitignoreBuilder::new(Path::new("/home/user/project"));
/// builder.add_line(None, "*.o").unwrap();
/// builder.add_line(None, "target/").unwrap();
/// builder.add_line(None, "!important.o").unwrap();
/// let gi = builder.build().unwrap();
/// ```
pub struct GitignoreBuilder {
    /// The root directory.
    root: PathBuf,
    /// Accumulated rules.
    rules: Vec<GitignoreRule>,
    /// Current line number counter (for add_line without a file).
    line_count: u64,
}

impl GitignoreBuilder {
    /// Create a new builder rooted at the given directory.
    pub fn new(root: &Path) -> GitignoreBuilder {
        GitignoreBuilder {
            root: root.to_path_buf(),
            rules: Vec::new(),
            line_count: 0,
        }
    }

    /// Add a single gitignore pattern line.
    ///
    /// `from` is the optional path of the gitignore file this line came from.
    /// The line is parsed according to gitignore syntax: comments, negation,
    /// trailing slashes, leading slashes, and `**` are all handled.
    pub fn add_line(
        &mut self,
        from: Option<&Path>,
        line: &str,
    ) -> Result<&mut GitignoreBuilder, Error> {
        self.line_count += 1;
        let line_num = self.line_count;

        // Strip trailing whitespace (unless escaped with backslash)
        let line = strip_trailing_spaces(line);

        // Skip blank lines
        if line.is_empty() {
            return Ok(self);
        }

        // Skip comments
        if line.starts_with('#') {
            return Ok(self);
        }

        // Check for negation
        let (is_whitelist, line) = if let Some(rest) = line.strip_prefix('!') {
            (true, rest.to_string())
        } else {
            (false, line.to_string())
        };

        // Check for trailing slash (directory-only)
        let (is_only_dir, pattern) = if line.ends_with('/') {
            (true, line[..line.len() - 1].to_string())
        } else {
            (false, line.clone())
        };

        // Convert the gitignore pattern to a glob
        let glob_pattern = gitignore_to_glob(&pattern, &self.root);

        // Try to compile the glob
        let glob = Glob::new(&glob_pattern).map_err(|e| {
            Error::parse(
                from,
                Some(line_num),
                format!("error parsing glob '{}': {}", glob_pattern, e),
            )
        })?;

        self.rules.push(GitignoreRule {
            original: line.clone(),
            glob: glob_pattern,
            is_whitelist,
            is_only_dir,
            from: from.map(|p| p.to_path_buf()),
            line: line_num,
        });

        Ok(self)
    }

    /// Add all patterns from a gitignore file.
    ///
    /// Returns an error if the file cannot be read, but individual
    /// parse errors are silently ignored (the rules that could be
    /// parsed are still added).
    pub fn add(&mut self, path: &Path) -> Option<Error> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Some(Error::io(path, e)),
        };
        let rdr = io::BufReader::new(file);
        let mut errs = Vec::new();
        // Reset line count for this file
        let saved = self.line_count;
        self.line_count = 0;
        for line in rdr.lines() {
            match line {
                Ok(line) => {
                    if let Err(e) = self.add_line(Some(path), &line) {
                        errs.push(e);
                    }
                }
                Err(e) => {
                    errs.push(Error::io(path, e));
                }
            }
        }
        if errs.is_empty() {
            None
        } else {
            Some(Error::multi(errs))
        }
    }

    /// Build a `Gitignore` matcher from all added rules.
    pub fn build(&self) -> Result<Gitignore, Error> {
        let mut set_builder = GlobSetBuilder::new();
        let mut globs = Vec::with_capacity(self.rules.len());

        for rule in &self.rules {
            let glob = Glob::new(&rule.glob).map_err(|e| {
                Error::parse(
                    rule.from.as_deref(),
                    Some(rule.line),
                    format!("error compiling glob '{}': {}", rule.glob, e),
                )
            })?;
            set_builder.add(glob);
            globs.push(GlobResult {
                glob: rule.original.clone(),
                from: rule.from.clone(),
                line: rule.line,
            });
        }

        let set = set_builder.build().map_err(|e| {
            Error::glob(&format!("error building glob set: {}", e))
        })?;

        let num_ignores = self.rules.iter().filter(|r| !r.is_whitelist).count() as u64;
        let num_whitelists = self.rules.iter().filter(|r| r.is_whitelist).count() as u64;

        Ok(Gitignore {
            root: self.root.clone(),
            rules: self.rules.clone(),
            globs,
            set,
            num_ignores,
            num_whitelists,
        })
    }
}

/// Convert a gitignore pattern to a glob pattern suitable for the globset crate.
///
/// Gitignore patterns have slightly different semantics from standard globs:
/// - A leading `/` anchors to the gitignore's directory
/// - No leading `/` and no internal `/` means match against basename
/// - `**` is handled by globset natively
fn gitignore_to_glob(pattern: &str, _root: &Path) -> String {
    let mut pat = pattern.to_string();

    // If the pattern starts with `/`, it's anchored to root
    let anchored = pat.starts_with('/');
    if anchored {
        pat = pat[1..].to_string();
    }

    // Check if pattern contains a path separator (excluding trailing)
    let has_slash = pat.contains('/');

    if anchored || has_slash {
        // Pattern is relative to root - use as-is (globset handles this)
        pat
    } else {
        // Pattern should match against basename anywhere in tree
        // Prefix with **/ to match at any level
        format!("**/{}", pat)
    }
}

/// Strip trailing spaces from a line, respecting backslash escapes.
fn strip_trailing_spaces(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut end = bytes.len();

    while end > 0 && bytes[end - 1] == b' ' {
        // Check if the space is escaped
        let mut num_backslashes = 0;
        let mut pos = end - 1;
        while pos > 0 && bytes[pos - 1] == b'\\' {
            num_backslashes += 1;
            pos -= 1;
        }
        if num_backslashes % 2 == 1 {
            // Space is escaped, stop stripping
            break;
        }
        end -= 1;
    }

    line[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_trailing_spaces() {
        assert_eq!(strip_trailing_spaces("foo  "), "foo");
        assert_eq!(strip_trailing_spaces("foo"), "foo");
        assert_eq!(strip_trailing_spaces("  "), "");
        assert_eq!(strip_trailing_spaces("foo\\ "), "foo\\ ");
    }

    #[test]
    fn test_empty_gitignore() {
        let gi = Gitignore::empty();
        assert!(gi.is_empty());
        assert!(gi.matched(Path::new("foo"), false).is_none());
    }

    #[test]
    fn test_comment_and_blank() {
        let mut builder = GitignoreBuilder::new(Path::new("/"));
        builder.add_line(None, "# comment").unwrap();
        builder.add_line(None, "").unwrap();
        builder.add_line(None, "   ").unwrap();
        let gi = builder.build().unwrap();
        assert!(gi.is_empty());
    }

    #[test]
    fn test_simple_ignore() {
        let mut builder = GitignoreBuilder::new(Path::new("/project"));
        builder.add_line(None, "*.o").unwrap();
        let gi = builder.build().unwrap();
        assert!(gi.matched(Path::new("foo.o"), false).is_ignore());
        assert!(gi.matched(Path::new("bar.rs"), false).is_none());
    }

    #[test]
    fn test_negation() {
        let mut builder = GitignoreBuilder::new(Path::new("/project"));
        builder.add_line(None, "*.o").unwrap();
        builder.add_line(None, "!important.o").unwrap();
        let gi = builder.build().unwrap();
        assert!(gi.matched(Path::new("foo.o"), false).is_ignore());
        assert!(gi.matched(Path::new("important.o"), false).is_whitelist());
    }

    #[test]
    fn test_directory_only() {
        let mut builder = GitignoreBuilder::new(Path::new("/project"));
        builder.add_line(None, "build/").unwrap();
        let gi = builder.build().unwrap();
        assert!(gi.matched(Path::new("build"), true).is_ignore());
        assert!(gi.matched(Path::new("build"), false).is_none());
    }

    #[test]
    fn test_anchored_pattern() {
        let mut builder = GitignoreBuilder::new(Path::new("/project"));
        builder.add_line(None, "/target").unwrap();
        let gi = builder.build().unwrap();
        assert!(gi.matched(Path::new("target"), false).is_ignore());
    }

    #[test]
    fn test_num_counts() {
        let mut builder = GitignoreBuilder::new(Path::new("/"));
        builder.add_line(None, "*.o").unwrap();
        builder.add_line(None, "*.a").unwrap();
        builder.add_line(None, "!keep.o").unwrap();
        let gi = builder.build().unwrap();
        assert_eq!(gi.num_ignores(), 2);
        assert_eq!(gi.num_whitelists(), 1);
    }

    #[test]
    fn test_match_invert() {
        let gr = GlobResult {
            glob: "*.o".to_string(),
            from: None,
            line: 1,
        };
        let m = Match::Ignore(&gr);
        assert!(m.is_ignore());
        let m2 = m.invert();
        assert!(m2.is_whitelist());
    }

    #[test]
    fn test_gitignore_to_glob_anchored() {
        let result = gitignore_to_glob("/target", Path::new("/project"));
        assert_eq!(result, "target");
    }

    #[test]
    fn test_gitignore_to_glob_basename() {
        let result = gitignore_to_glob("*.o", Path::new("/project"));
        assert_eq!(result, "**/*.o");
    }

    #[test]
    fn test_gitignore_to_glob_with_slash() {
        let result = gitignore_to_glob("src/generated", Path::new("/project"));
        assert_eq!(result, "src/generated");
    }
}
