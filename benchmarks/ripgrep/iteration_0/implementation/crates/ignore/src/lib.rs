//! Fast recursive directory traversal with gitignore support.
//!
//! This crate provides facilities for walking a directory tree while
//! respecting `.gitignore`, `.ignore`, and `.rgignore` files. It also
//! supports file-type filtering, override globs (like `--glob`), hidden
//! file skipping, and parallel traversal.
//!
//! # Example
//!
//! ```no_run
//! use ignore::WalkBuilder;
//!
//! for entry in WalkBuilder::new(".").build() {
//!     match entry {
//!         Ok(e) => println!("{}", e.path().display()),
//!         Err(e) => eprintln!("error: {}", e),
//!     }
//! }
//! ```

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// An error that can occur during directory walking or ignore-rule processing.
#[derive(Debug)]
pub struct Error {
    message: String,
    kind: ErrorKind,
}

/// The specific kind of error.
#[derive(Debug)]
enum ErrorKind {
    /// An I/O error.
    Io(io::Error),
    /// A glob pattern error.
    Glob(String),
    /// A parse error (e.g. malformed gitignore line).
    Parse(String),
}

impl Error {
    /// Create a new I/O error.
    fn io(err: io::Error) -> Self {
        Error {
            message: err.to_string(),
            kind: ErrorKind::Io(err),
        }
    }

    /// Create a new glob error.
    fn glob(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Error {
            message: msg.clone(),
            kind: ErrorKind::Glob(msg),
        }
    }

    /// Create a new parse error.
    fn parse(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Error {
            message: msg.clone(),
            kind: ErrorKind::Parse(msg),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Io(err) => write!(f, "I/O error: {}", err),
            ErrorKind::Glob(msg) => write!(f, "glob error: {}", msg),
            ErrorKind::Parse(msg) => write!(f, "parse error: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::io(err)
    }
}

impl From<globset::GlobSetError> for Error {
    fn from(err: globset::GlobSetError) -> Self {
        Error::glob(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Match type
// ---------------------------------------------------------------------------

/// Describes the result of matching a path against a set of rules.
///
/// A `Match::None` indicates no rules matched. `Match::Ignore` indicates
/// the path should be ignored/excluded. `Match::Whitelist` indicates the
/// path is explicitly included (e.g. via a negated gitignore pattern or
/// a whitelist override glob).
#[derive(Clone, Debug)]
pub enum Match {
    /// No rules matched the path.
    None,
    /// The path should be ignored.
    Ignore,
    /// The path is whitelisted (explicitly included).
    Whitelist,
}

impl Match {
    /// Returns `true` if this match indicates the path should be ignored.
    pub fn is_ignore(&self) -> bool {
        matches!(self, Match::Ignore)
    }

    /// Returns `true` if this match indicates the path is whitelisted.
    pub fn is_whitelist(&self) -> bool {
        matches!(self, Match::Whitelist)
    }

    /// Returns `true` if no rules matched.
    pub fn is_none(&self) -> bool {
        matches!(self, Match::None)
    }
}

// ---------------------------------------------------------------------------
// File Types
// ---------------------------------------------------------------------------

/// A file type definition, mapping a name to a set of glob patterns.
#[derive(Clone, Debug)]
struct FileType {
    /// The name of the file type (e.g. "rust", "py").
    name: String,
    /// The glob patterns associated with this file type.
    globs: Vec<String>,
}

/// A matcher for file types.
///
/// `Types` is built by [`TypesBuilder`] and can determine whether a file
/// path matches a selected or negated file type.
#[derive(Clone, Debug)]
pub struct Types {
    /// Selected file types (whitelist).
    selected: Vec<FileType>,
    /// Negated file types (blacklist).
    negated: Vec<FileType>,
    /// A compiled glob set for all selected type patterns.
    glob_set_selected: globset::GlobSet,
    /// A compiled glob set for all negated type patterns.
    glob_set_negated: globset::GlobSet,
    /// Whether any type selection is active.
    has_selected: bool,
}

impl Types {
    /// Returns `true` if no type filtering is active.
    pub fn is_empty(&self) -> bool {
        !self.has_selected && self.negated.is_empty()
    }

    /// Test whether the given path matches a selected or negated file type.
    ///
    /// - Returns `Match::None` if no type filtering is active.
    /// - Returns `Match::Whitelist` if the file matches a selected type.
    /// - Returns `Match::Ignore` if the file matches a negated type or
    ///   if selected types exist but the file doesn't match any of them.
    ///
    /// Directories always return `Match::None` since type matching only
    /// applies to files.
    pub fn matched<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> Match {
        // Type matching doesn't apply to directories.
        if is_dir {
            return Match::None;
        }

        // If no type filtering is active at all, everything passes.
        if self.is_empty() {
            return Match::None;
        }

        let path = path.as_ref();

        // Check negated types first — they take precedence.
        if !self.negated.is_empty() && self.glob_set_negated.is_match(path) {
            return Match::Ignore;
        }

        // If there are selected types, the file must match one.
        if self.has_selected {
            if self.glob_set_selected.is_match(path) {
                return Match::Whitelist;
            }
            return Match::Ignore;
        }

        Match::None
    }
}

/// A builder for constructing a [`Types`] file type matcher.
///
/// # Example
///
/// ```
/// use ignore::TypesBuilder;
///
/// let mut builder = TypesBuilder::new();
/// builder.add_defaults();
/// builder.select("rust");
/// let types = builder.build().unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct TypesBuilder {
    /// All known file types.
    types: Vec<FileType>,
    /// Names of selected types.
    selected_names: Vec<String>,
    /// Names of negated types.
    negated_names: Vec<String>,
}

impl TypesBuilder {
    /// Create a new, empty `TypesBuilder`.
    pub fn new() -> Self {
        TypesBuilder {
            types: Vec::new(),
            selected_names: Vec::new(),
            negated_names: Vec::new(),
        }
    }

    /// Add the built-in default file types.
    ///
    /// This includes common languages and file formats like Rust, Python,
    /// JavaScript, JSON, YAML, and many more.
    pub fn add_defaults(&mut self) -> &mut Self {
        let defaults: &[(&str, &str)] = &[
            ("rust", "*.rs"),
            ("py", "*.py"),
            ("python", "*.py"),
            ("js", "*.js"),
            ("javascript", "*.js"),
            ("ts", "*.ts"),
            ("typescript", "*.ts"),
            ("json", "*.json"),
            ("yaml", "*.yaml"),
            ("yaml", "*.yml"),
            ("toml", "*.toml"),
            ("html", "*.html"),
            ("html", "*.htm"),
            ("css", "*.css"),
            ("xml", "*.xml"),
            ("md", "*.md"),
            ("markdown", "*.md"),
            ("c", "*.c"),
            ("c", "*.h"),
            ("cpp", "*.cpp"),
            ("cpp", "*.hpp"),
            ("cpp", "*.cc"),
            ("cpp", "*.hh"),
            ("cpp", "*.cxx"),
            ("java", "*.java"),
            ("go", "*.go"),
            ("rb", "*.rb"),
            ("ruby", "*.rb"),
            ("sh", "*.sh"),
            ("bash", "*.bash"),
            ("txt", "*.txt"),
            ("csv", "*.csv"),
            ("sql", "*.sql"),
            ("php", "*.php"),
            ("swift", "*.swift"),
            ("kt", "*.kt"),
            ("kotlin", "*.kt"),
            ("scala", "*.scala"),
            ("r", "*.r"),
            ("r", "*.R"),
            ("lua", "*.lua"),
            ("perl", "*.pl"),
            ("perl", "*.pm"),
            ("zig", "*.zig"),
            ("dart", "*.dart"),
            ("make", "Makefile"),
            ("make", "makefile"),
            ("make", "*.mk"),
            ("cmake", "CMakeLists.txt"),
            ("cmake", "*.cmake"),
            ("docker", "Dockerfile"),
            ("docker", "*.dockerfile"),
            ("proto", "*.proto"),
            ("tex", "*.tex"),
            ("vim", "*.vim"),
            ("elisp", "*.el"),
            ("clojure", "*.clj"),
            ("clojure", "*.cljs"),
            ("haskell", "*.hs"),
            ("erlang", "*.erl"),
            ("ocaml", "*.ml"),
            ("ocaml", "*.mli"),
        ];

        for &(name, glob) in defaults {
            // Ignoring errors on defaults since they are known-good patterns.
            let _ = self.add(name, glob);
        }
        self
    }

    /// Add a glob pattern for a named file type.
    ///
    /// If the file type name already exists, the glob is added to it.
    /// Otherwise a new file type is created.
    pub fn add(&mut self, name: &str, glob: &str) -> Result<&mut Self, Error> {
        // Validate the glob pattern by attempting to compile it.
        globset::Glob::new(glob)?;

        // Find or create the file type entry.
        if let Some(ft) = self.types.iter_mut().find(|ft| ft.name == name) {
            if !ft.globs.contains(&glob.to_string()) {
                ft.globs.push(glob.to_string());
            }
        } else {
            self.types.push(FileType {
                name: name.to_string(),
                globs: vec![glob.to_string()],
            });
        }
        Ok(self)
    }

    /// Remove all glob patterns for the given file type name.
    pub fn clear(&mut self, name: &str) -> &mut Self {
        self.types.retain(|ft| ft.name != name);
        self
    }

    /// Select the given file type name for inclusion.
    ///
    /// When types are built with selected types, only files matching at
    /// least one selected type will be included.
    pub fn select(&mut self, name: &str) -> &mut Self {
        self.selected_names.push(name.to_string());
        self
    }

    /// Negate the given file type name.
    ///
    /// Files matching a negated type will be excluded.
    pub fn negate(&mut self, name: &str) -> &mut Self {
        self.negated_names.push(name.to_string());
        self
    }

    /// Build the [`Types`] matcher from the current configuration.
    ///
    /// Returns an error if any glob pattern fails to compile (unlikely
    /// since patterns are validated at `add` time).
    pub fn build(&self) -> Result<Types, Error> {
        // Collect selected file types.
        let selected: Vec<FileType> = self
            .types
            .iter()
            .filter(|ft| self.selected_names.contains(&ft.name))
            .cloned()
            .collect();

        // Collect negated file types.
        let negated: Vec<FileType> = self
            .types
            .iter()
            .filter(|ft| self.negated_names.contains(&ft.name))
            .cloned()
            .collect();

        // Build glob set for selected types.
        let mut sel_builder = globset::GlobSetBuilder::new();
        for ft in &selected {
            for g in &ft.globs {
                sel_builder.add(globset::Glob::new(g)?);
            }
        }
        let glob_set_selected = sel_builder.build()?;

        // Build glob set for negated types.
        let mut neg_builder = globset::GlobSetBuilder::new();
        for ft in &negated {
            for g in &ft.globs {
                neg_builder.add(globset::Glob::new(g)?);
            }
        }
        let glob_set_negated = neg_builder.build()?;

        let has_selected = !self.selected_names.is_empty();

        Ok(Types {
            selected,
            negated,
            glob_set_selected,
            glob_set_negated,
            has_selected,
        })
    }
}

impl Default for TypesBuilder {
    fn default() -> Self {
        TypesBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Gitignore
// ---------------------------------------------------------------------------

/// A single gitignore rule.
#[derive(Clone, Debug)]
struct GitignoreRule {
    /// The compiled glob set for this rule's pattern.
    glob: globset::GlobSet,
    /// Whether this rule is a negation (line started with `!`).
    is_negation: bool,
    /// Whether this rule only applies to directories (pattern ended with `/`).
    is_dir_only: bool,
    /// Whether this rule is root-relative (original pattern had leading `/`).
    is_root_only: bool,
    /// The original pattern string (for debugging).
    original: String,
}

/// A matcher that applies gitignore-style rules to paths.
///
/// Rules are evaluated in order, with the last matching rule winning.
/// Negation patterns (starting with `!`) re-include previously ignored
/// files.
#[derive(Clone, Debug)]
pub struct Gitignore {
    /// The ordered list of rules.
    rules: Vec<GitignoreRule>,
    /// The root directory that this gitignore is relative to.
    root: PathBuf,
}

impl Gitignore {
    /// Create an empty `Gitignore` that matches nothing.
    pub fn empty() -> Gitignore {
        Gitignore {
            rules: Vec::new(),
            root: PathBuf::new(),
        }
    }

    /// Test whether the given path matches any gitignore rule.
    ///
    /// Rules are evaluated in order, with later rules overriding earlier
    /// ones. A negation pattern re-includes a previously ignored path.
    ///
    /// - `Match::Ignore` — the path should be ignored.
    /// - `Match::Whitelist` — the path was negated (re-included).
    /// - `Match::None` — no rule matched.
    pub fn matched<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> Match {
        if self.rules.is_empty() {
            return Match::None;
        }

        let path = path.as_ref();

        // Try to make path relative to root.
        let relative = path.strip_prefix(&self.root).unwrap_or(path);

        // Evaluate rules in reverse order — last matching rule wins.
        for rule in self.rules.iter().rev() {
            // Skip directory-only rules for non-directories.
            if rule.is_dir_only && !is_dir {
                continue;
            }

            // For root-only rules (leading `/`), only match if the
            // relative path has no parent directory components.
            if rule.is_root_only {
                // Only match against the relative path, and it should
                // have at most one component (the entry itself).
                let component_count = relative.components().count();
                if component_count > 1 {
                    continue;
                }
            }

            // Test against relative path.
            if rule.glob.is_match(relative) {
                if rule.is_negation {
                    return Match::Whitelist;
                } else {
                    return Match::Ignore;
                }
            }
        }

        Match::None
    }

    /// Returns `true` if this gitignore contains no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// A builder for constructing a [`Gitignore`] matcher.
///
/// # Example
///
/// ```no_run
/// use ignore::GitignoreBuilder;
///
/// let mut builder = GitignoreBuilder::new("/my/project");
/// builder.add_line(None, "target/").unwrap();
/// builder.add_line(None, "*.o").unwrap();
/// builder.add_line(None, "!important.o").unwrap();
/// let gitignore = builder.build().unwrap();
/// ```
pub struct GitignoreBuilder {
    /// The root directory for this gitignore.
    root: PathBuf,
    /// Collected rules: (pattern, is_negation, is_dir_only).
    /// Collected rules: (pattern, is_negation, is_dir_only, is_root_only).
    rules: Vec<(String, bool, bool, bool)>,
}

impl GitignoreBuilder {
    /// Create a new builder rooted at the given directory.
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        GitignoreBuilder {
            root: root.as_ref().to_path_buf(),
            rules: Vec::new(),
        }
    }

    /// Parse and add a single gitignore-format line.
    ///
    /// The `from` parameter is optional and used only for error reporting.
    ///
    /// Gitignore rules:
    /// - Lines starting with `#` are comments.
    /// - Trailing whitespace is stripped unless escaped with `\`.
    /// - `!` prefix negates a pattern.
    /// - Trailing `/` means directory-only matching.
    /// - Patterns containing `/` (other than trailing) match the full
    ///   relative path; otherwise they match only the basename.
    /// - `**` has special meaning (any path depth).
    pub fn add_line(
        &mut self,
        _from: Option<&Path>,
        line: &str,
    ) -> Result<&mut Self, Error> {
        let mut line = line.to_string();

        // Blank lines are ignored.
        if line.trim().is_empty() {
            return Ok(self);
        }

        // Lines starting with `#` are comments.
        if line.starts_with('#') {
            return Ok(self);
        }

        // Strip trailing unescaped whitespace.
        // We look for trailing spaces that are not preceded by `\`.
        while line.ends_with(' ') && !line.ends_with("\\ ") {
            line.pop();
        }
        // Unescape escaped trailing spaces.
        if line.ends_with("\\ ") {
            // Remove the backslash, keep the space.
            let len = line.len();
            line.replace_range(len - 2..len - 1, "");
        }

        if line.is_empty() {
            return Ok(self);
        }

        // Check for negation.
        let is_negation = line.starts_with('!');
        if is_negation {
            line = line[1..].to_string();
        }

        // Check for directory-only (trailing `/`).
        let is_dir_only = line.ends_with('/');
        if is_dir_only {
            line.pop(); // remove trailing `/`
        }

        if line.is_empty() {
            return Ok(self);
        }

        // Determine if this is a basename-only or full-path pattern.
        // If the pattern contains `/` (not counting a leading one), it's
        // a full-path pattern. Otherwise it's basename-only.
        let mut is_root_only = false;
        let has_slash = if line.starts_with('/') {
            // Leading slash: root-relative pattern, remove the slash.
            line = line[1..].to_string();
            is_root_only = true;
            true
        } else {
            line.contains('/')
        };

        // Build the actual glob pattern.
        let glob_pattern = if has_slash {
            // Full-path pattern: match relative to root.
            line.clone()
        } else {
            // Basename-only: prepend `**/` to match at any depth.
            format!("**/{}", line)
        };

        self.rules.push((glob_pattern, is_negation, is_dir_only, is_root_only));
        Ok(self)
    }

    /// Read and parse all lines from a gitignore-format file.
    ///
    /// If the file does not exist, this is a no-op (not an error).
    pub fn add_file<P: AsRef<Path>>(&mut self, path: P) -> Result<&mut Self, Error> {
        let path = path.as_ref();
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(self);
            }
            Err(err) => return Err(Error::io(err)),
        };

        for line in contents.lines() {
            self.add_line(Some(path), line)?;
        }
        Ok(self)
    }

    /// Build the [`Gitignore`] matcher.
    pub fn build(&self) -> Result<Gitignore, Error> {
        let mut rules = Vec::new();
        for (pattern, is_negation, is_dir_only, is_root_only) in &self.rules {
            let glob = match globset::Glob::new(pattern) {
                Ok(g) => g,
                Err(err) => {
                    log::warn!("skipping invalid gitignore pattern '{}': {}", pattern, err);
                    continue;
                }
            };
            let mut builder = globset::GlobSetBuilder::new();
            builder.add(glob);
            let glob_set = builder.build()?;

            rules.push(GitignoreRule {
                glob: glob_set,
                is_negation: *is_negation,
                is_dir_only: *is_dir_only,
                is_root_only: *is_root_only,
                original: pattern.clone(),
            });
        }

        Ok(Gitignore {
            rules,
            root: self.root.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Override (for --glob flags)
// ---------------------------------------------------------------------------

/// A single override rule.
#[derive(Clone, Debug)]
struct OverrideRule {
    /// The compiled glob set for this rule's pattern.
    glob: globset::GlobSet,
    /// Whether this is a negation (exclusion) pattern.
    is_negation: bool,
}

/// A matcher for override globs (like `--glob` flags).
///
/// Override globs provide explicit include/exclude patterns that take
/// precedence over gitignore rules.
///
/// - Globs without `!` prefix are whitelist (inclusion) patterns.
/// - Globs with `!` prefix are exclusion patterns.
/// - If any whitelist patterns exist, only files matching a whitelist
///   pattern are included.
/// - Exclusion patterns take precedence over whitelist patterns.
#[derive(Clone, Debug)]
pub struct Override {
    /// The ordered list of override rules.
    rules: Vec<OverrideRule>,
    /// Whether any whitelist (non-negation) rules exist.
    has_whitelist: bool,
}

impl Override {
    /// Create an empty `Override` that has no effect.
    pub fn empty() -> Override {
        Override {
            rules: Vec::new(),
            has_whitelist: false,
        }
    }

    /// Test whether the given path matches any override rule.
    ///
    /// - `Match::Ignore` — the path should be excluded.
    /// - `Match::Whitelist` — the path is whitelisted.
    /// - `Match::None` — no override matched (file is neither
    ///   explicitly included nor excluded).
    pub fn matched<P: AsRef<Path>>(&self, path: P, _is_dir: bool) -> Match {
        if self.rules.is_empty() {
            return Match::None;
        }

        let path = path.as_ref();

        // Check exclusion patterns first — they take precedence.
        for rule in &self.rules {
            if rule.is_negation && rule.glob.is_match(path) {
                return Match::Ignore;
            }
        }

        // If there are whitelist patterns, check if the file matches any.
        if self.has_whitelist {
            for rule in &self.rules {
                if !rule.is_negation && rule.glob.is_match(path) {
                    return Match::Whitelist;
                }
            }
            // Whitelist patterns exist but this file doesn't match any.
            return Match::Ignore;
        }

        Match::None
    }

    /// Returns `true` if this override contains no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// A builder for constructing an [`Override`] matcher.
///
/// # Example
///
/// ```
/// use ignore::OverrideBuilder;
///
/// let mut builder = OverrideBuilder::new(".");
/// builder.add("*.rs").unwrap();
/// builder.add("!test_*.rs").unwrap();
/// let overrides = builder.build().unwrap();
/// ```
pub struct OverrideBuilder {
    /// The root directory for resolving relative patterns.
    root: PathBuf,
    /// Collected rules: (pattern, is_negation).
    rules: Vec<(String, bool)>,
}

impl OverrideBuilder {
    /// Create a new builder rooted at the given directory.
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        OverrideBuilder {
            root: root.as_ref().to_path_buf(),
            rules: Vec::new(),
        }
    }

    /// Add a glob pattern to the override set.
    ///
    /// Patterns prefixed with `!` are exclusion (negation) patterns.
    /// All other patterns are whitelist (inclusion) patterns.
    pub fn add(&mut self, glob: &str) -> Result<&mut Self, Error> {
        let (pattern, is_negation) = if let Some(stripped) = glob.strip_prefix('!') {
            (stripped.to_string(), true)
        } else {
            (glob.to_string(), false)
        };

        // Validate the glob.
        globset::Glob::new(&pattern)?;
        self.rules.push((pattern, is_negation));
        Ok(self)
    }

    /// Build the [`Override`] matcher.
    pub fn build(&self) -> Result<Override, Error> {
        let mut rules = Vec::new();
        let mut has_whitelist = false;

        for (pattern, is_negation) in &self.rules {
            let glob = globset::Glob::new(pattern)?;
            let mut builder = globset::GlobSetBuilder::new();
            builder.add(glob);
            let glob_set = builder.build()?;

            if !is_negation {
                has_whitelist = true;
            }

            rules.push(OverrideRule {
                glob: glob_set,
                is_negation: *is_negation,
            });
        }

        Ok(Override {
            rules,
            has_whitelist,
        })
    }
}

// ---------------------------------------------------------------------------
// DirEntry
// ---------------------------------------------------------------------------

/// A directory entry encountered during walking.
///
/// Wraps file path, type, depth, and optional metadata for entries
/// produced by [`Walk`] or [`WalkParallel`].
#[derive(Debug)]
pub struct DirEntry {
    /// The full path to this entry.
    path: PathBuf,
    /// The file type of this entry, if available.
    file_type: Option<fs::FileType>,
    /// The depth of this entry relative to the walk root.
    depth: usize,
    /// Cached metadata, if available.
    metadata: Option<fs::Metadata>,
}

impl DirEntry {
    /// Create a `DirEntry` from a `walkdir::DirEntry`.
    fn from_walkdir(entry: walkdir::DirEntry) -> Self {
        let path = entry.path().to_path_buf();
        let file_type = entry.file_type().into();
        let depth = entry.depth();
        let metadata = entry.metadata().ok();
        DirEntry {
            path,
            file_type: Some(file_type),
            depth,
            metadata,
        }
    }

    /// Create a `DirEntry` from a plain path (used for root entries).
    fn from_path<P: AsRef<Path>>(path: P, depth: usize) -> Self {
        let path = path.as_ref().to_path_buf();
        let metadata = fs::metadata(&path).ok();
        let file_type = metadata.as_ref().map(|m| m.file_type());
        DirEntry {
            path,
            file_type,
            depth,
            metadata,
        }
    }

    /// Returns the full path to this entry.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consume this entry and return its path.
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Returns the file type of this entry, if available.
    pub fn file_type(&self) -> Option<fs::FileType> {
        self.file_type
    }

    /// Returns `true` if this entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type.as_ref().is_some_and(|ft| ft.is_dir())
    }

    /// Returns `true` if this entry is a regular file.
    pub fn is_file(&self) -> bool {
        self.file_type.as_ref().is_some_and(|ft| ft.is_file())
    }

    /// Returns the depth of this entry relative to the walk root.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the metadata for this entry.
    ///
    /// If metadata was cached during construction, it is returned directly.
    /// Otherwise, a fresh `stat` call is performed.
    pub fn metadata(&self) -> Result<fs::Metadata, Error> {
        if let Some(ref md) = self.metadata {
            Ok(md.clone())
        } else {
            fs::metadata(&self.path).map_err(Error::io)
        }
    }

    /// Returns the file name component of this entry's path.
    pub fn file_name(&self) -> &OsStr {
        self.path.file_name().unwrap_or(self.path.as_os_str())
    }

    /// Returns `true` if this entry represents stdin.
    ///
    /// This is always `false` for entries produced by directory walking;
    /// stdin entries are only created explicitly by the caller.
    pub fn is_stdin(&self) -> bool {
        self.path.as_os_str() == "-"
    }
}

// ---------------------------------------------------------------------------
// IgnoreStack — internal ignore-rule aggregation
// ---------------------------------------------------------------------------

/// Internal helper that aggregates ignore rules from multiple sources:
/// `.gitignore`, `.ignore`, `.rgignore` files encountered during traversal.
#[derive(Clone)]
struct IgnoreStack {
    /// Stack of gitignore matchers, one per directory level.
    gitignores: Vec<Gitignore>,
}

impl IgnoreStack {
    fn new() -> Self {
        IgnoreStack {
            gitignores: Vec::new(),
        }
    }

    /// Push ignore rules from the given directory.
    fn push_dir(&mut self, dir: &Path, no_ignore: bool, no_ignore_vcs: bool) {
        if no_ignore {
            return;
        }

        // Load .ignore and .rgignore (always, unless no_ignore is set).
        for name in &[".rgignore", ".ignore"] {
            let path = dir.join(name);
            if path.exists() {
                let mut builder = GitignoreBuilder::new(dir);
                if builder.add_file(&path).is_ok() {
                    if let Ok(gi) = builder.build() {
                        if !gi.is_empty() {
                            self.gitignores.push(gi);
                        }
                    }
                }
            }
        }

        // Load .gitignore (unless no_ignore_vcs is set).
        if !no_ignore_vcs {
            let gitignore_path = dir.join(".gitignore");
            if gitignore_path.exists() {
                let mut builder = GitignoreBuilder::new(dir);
                if builder.add_file(&gitignore_path).is_ok() {
                    if let Ok(gi) = builder.build() {
                        if !gi.is_empty() {
                            self.gitignores.push(gi);
                        }
                    }
                }
            }
        }
    }

    /// Test a path against all loaded ignore rules.
    fn matched(&self, path: &Path, is_dir: bool) -> Match {
        // Later rules override earlier ones. Iterate in reverse.
        for gi in self.gitignores.iter().rev() {
            let m = gi.matched(path, is_dir);
            if !m.is_none() {
                return m;
            }
        }
        Match::None
    }
}

// ---------------------------------------------------------------------------
// SortBy
// ---------------------------------------------------------------------------

/// Controls how entries are sorted within each directory.
#[derive(Clone, Copy, Debug)]
pub enum SortBy {
    /// No sorting — entries are yielded in filesystem order.
    None,
    /// Sort by path in ascending order.
    Path,
    /// Sort by path in descending order.
    PathReverse,
}

// ---------------------------------------------------------------------------
// WalkBuilder
// ---------------------------------------------------------------------------

/// A builder for configuring directory walking.
///
/// `WalkBuilder` creates either a single-threaded [`Walk`] iterator or
/// a multi-threaded [`WalkParallel`] walker.
///
/// # Example
///
/// ```no_run
/// use ignore::WalkBuilder;
///
/// let walk = WalkBuilder::new("src")
///     .hidden(true)
///     .max_depth(Some(10))
///     .build();
///
/// for entry in walk {
///     println!("{}", entry.unwrap().path().display());
/// }
/// ```
pub struct WalkBuilder {
    /// Root paths to walk.
    paths: Vec<PathBuf>,
    /// Maximum directory depth to traverse.
    max_depth: Option<usize>,
    /// Whether to follow symbolic links.
    follow_links: bool,
    /// Maximum file size in bytes; files larger are skipped.
    max_filesize: Option<u64>,
    /// If `true`, skip hidden files (those starting with `.`).
    hidden: bool,
    /// If `true`, skip all ignore files (`.ignore`, `.rgignore`, `.gitignore`).
    no_ignore: bool,
    /// If `true`, skip VCS ignore files (`.gitignore`).
    no_ignore_vcs: bool,
    /// If `true`, skip global gitignore.
    no_ignore_global: bool,
    /// If `true`, don't read ignore files from parent directories.
    no_ignore_parent: bool,
    /// File type matcher.
    types: Types,
    /// Override glob matcher.
    overrides: Override,
    /// Number of threads for parallel walking.
    threads: usize,
    /// Sort order for entries within each directory.
    sort_by: SortBy,
    /// If `true`, don't cross filesystem boundaries.
    same_file_system: bool,
    /// Optional filter predicate applied to each entry.
    filter_entry: Option<Arc<dyn Fn(&DirEntry) -> bool + Send + Sync>>,
}

impl WalkBuilder {
    /// Create a new `WalkBuilder` with a single root path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        // Build empty types so we have a valid default.
        let types = TypesBuilder::new().build().expect("empty types should build");
        WalkBuilder {
            paths: vec![path.as_ref().to_path_buf()],
            max_depth: Option::None,
            follow_links: false,
            max_filesize: Option::None,
            hidden: true,
            no_ignore: false,
            no_ignore_vcs: false,
            no_ignore_global: false,
            no_ignore_parent: false,
            types,
            overrides: Override::empty(),
            threads: 1,
            sort_by: SortBy::None,
            same_file_system: false,
            filter_entry: Option::None,
        }
    }

    /// Add an additional root path to walk.
    pub fn add<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Set the maximum directory depth.
    ///
    /// `None` means no limit.
    pub fn max_depth(&mut self, depth: Option<usize>) -> &mut Self {
        self.max_depth = depth;
        self
    }

    /// Set whether to follow symbolic links.
    pub fn follow_links(&mut self, yes: bool) -> &mut Self {
        self.follow_links = yes;
        self
    }

    /// Set the maximum file size in bytes.
    ///
    /// Files larger than this are skipped. `None` means no limit.
    pub fn max_filesize(&mut self, size: Option<u64>) -> &mut Self {
        self.max_filesize = size;
        self
    }

    /// Set whether to skip hidden files and directories.
    ///
    /// Hidden files are those whose names start with `.`. Defaults to `true`.
    pub fn hidden(&mut self, yes: bool) -> &mut Self {
        self.hidden = yes;
        self
    }

    /// Set whether to skip all ignore files.
    ///
    /// When `true`, `.ignore`, `.rgignore`, and `.gitignore` files are
    /// all skipped.
    pub fn no_ignore(&mut self, yes: bool) -> &mut Self {
        self.no_ignore = yes;
        self
    }

    /// Set whether to skip VCS ignore files (`.gitignore`).
    pub fn no_ignore_vcs(&mut self, yes: bool) -> &mut Self {
        self.no_ignore_vcs = yes;
        self
    }

    /// Set whether to skip the global gitignore.
    pub fn no_ignore_global(&mut self, yes: bool) -> &mut Self {
        self.no_ignore_global = yes;
        self
    }

    /// Set whether to skip ignore files from parent directories.
    pub fn no_ignore_parent(&mut self, yes: bool) -> &mut Self {
        self.no_ignore_parent = yes;
        self
    }

    /// Set the file type matcher.
    pub fn types(&mut self, types: Types) -> &mut Self {
        self.types = types;
        self
    }

    /// Set the override glob matcher.
    pub fn overrides(&mut self, overrides: Override) -> &mut Self {
        self.overrides = overrides;
        self
    }

    /// Set the number of threads for parallel walking.
    pub fn threads(&mut self, threads: usize) -> &mut Self {
        self.threads = std::cmp::max(1, threads);
        self
    }

    /// Sort entries by file name in ascending order.
    pub fn sort_by_file_name(&mut self) -> &mut Self {
        self.sort_by = SortBy::Path;
        self
    }

    /// Set whether to stay on the same file system.
    pub fn same_file_system(&mut self, yes: bool) -> &mut Self {
        self.same_file_system = yes;
        self
    }

    /// Set a custom filter predicate.
    ///
    /// The predicate is called for every entry. If it returns `false`,
    /// the entry (and its children, if a directory) is skipped.
    pub fn filter_entry<P: Fn(&DirEntry) -> bool + Send + Sync + 'static>(
        &mut self,
        filter: P,
    ) -> &mut Self {
        self.filter_entry = Some(Arc::new(filter));
        self
    }

    /// Build a single-threaded [`Walk`] iterator.
    pub fn build(&self) -> Walk {
        let entries = self.collect_entries();
        Walk {
            inner: Box::new(entries.into_iter()),
        }
    }

    /// Build a multi-threaded [`WalkParallel`] walker.
    pub fn build_parallel(&self) -> WalkParallel {
        WalkParallel {
            paths: self.paths.clone(),
            max_depth: self.max_depth,
            follow_links: self.follow_links,
            max_filesize: self.max_filesize,
            hidden: self.hidden,
            no_ignore: self.no_ignore,
            no_ignore_vcs: self.no_ignore_vcs,
            no_ignore_global: self.no_ignore_global,
            no_ignore_parent: self.no_ignore_parent,
            types: self.types.clone(),
            overrides: self.overrides.clone(),
            threads: self.threads,
            sort_by: self.sort_by,
            same_file_system: self.same_file_system,
            filter_entry: self.filter_entry.clone(),
        }
    }

    /// Internal: collect all entries using walkdir, applying filters.
    fn collect_entries(&self) -> Vec<Result<DirEntry, Error>> {
        let mut results = Vec::new();

        for root in &self.paths {
            self.walk_root(root, &mut results);
        }

        // Apply sorting if requested.
        match self.sort_by {
            SortBy::Path => {
                results.sort_by(|a, b| {
                    let pa = a.as_ref().map(|e| e.path().to_path_buf()).ok();
                    let pb = b.as_ref().map(|e| e.path().to_path_buf()).ok();
                    pa.cmp(&pb)
                });
            }
            SortBy::PathReverse => {
                results.sort_by(|a, b| {
                    let pa = a.as_ref().map(|e| e.path().to_path_buf()).ok();
                    let pb = b.as_ref().map(|e| e.path().to_path_buf()).ok();
                    pb.cmp(&pa)
                });
            }
            SortBy::None => {}
        }

        results
    }

    /// Walk a single root path, pushing results into `results`.
    fn walk_root(&self, root: &Path, results: &mut Vec<Result<DirEntry, Error>>) {
        // If the root doesn't exist, yield an error.
        if !root.exists() {
            results.push(Err(Error::io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("path not found: {}", root.display()),
            ))));
            return;
        }

        // If root is a file, just yield it directly (subject to filters).
        if root.is_file() {
            let entry = DirEntry::from_path(root, 0);
            if self.should_include(&entry) {
                results.push(Ok(entry));
            }
            return;
        }

        // Get the root device ID for same_file_system checking.
        #[cfg(unix)]
        let root_dev = if self.same_file_system {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(root).ok().map(|m| m.dev())
        } else {
            None
        };

        // Build the walkdir walker.
        let mut walkdir = walkdir::WalkDir::new(root);
        if let Some(depth) = self.max_depth {
            walkdir = walkdir.max_depth(depth);
        }
        walkdir = walkdir.follow_links(self.follow_links);

        // Build an ignore stack for this root.
        let mut ignore_stack = IgnoreStack::new();

        // Load parent ignore files if not disabled.
        if !self.no_ignore_parent && !self.no_ignore {
            if let Ok(abs_root) = fs::canonicalize(root) {
                let mut ancestors: Vec<&Path> = abs_root.ancestors().skip(1).collect();
                ancestors.reverse();
                for ancestor in ancestors {
                    ignore_stack.push_dir(ancestor, false, self.no_ignore_vcs);
                }
            }
        }

        // Track which directories we've loaded ignore files for.
        let mut loaded_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

        let walker = walkdir.into_iter();

        for result in walker {
            match result {
                Ok(wd_entry) => {
                    let path = wd_entry.path().to_path_buf();
                    let is_dir = wd_entry.file_type().is_dir();
                    let depth = wd_entry.depth();

                    // Check same file system.
                    #[cfg(unix)]
                    if let Some(dev) = root_dev {
                        use std::os::unix::fs::MetadataExt;
                        if let Ok(md) = fs::metadata(&path) {
                            if md.dev() != dev {
                                continue;
                            }
                        }
                    }

                    // For directories, load their ignore files.
                    if is_dir && !loaded_dirs.contains(&path) {
                        ignore_stack.push_dir(&path, self.no_ignore, self.no_ignore_vcs);
                        loaded_dirs.insert(path.clone());
                    }

                    let entry = DirEntry {
                        path: path.clone(),
                        file_type: Some(wd_entry.file_type()),
                        depth,
                        metadata: wd_entry.metadata().ok(),
                    };

                    // Check if hidden.
                    if self.hidden && depth > 0 {
                        if let Some(name) = path.file_name() {
                            if name.to_string_lossy().starts_with('.') {
                                continue;
                            }
                        }
                    }

                    // Check ignore rules.
                    if !self.no_ignore {
                        let m = ignore_stack.matched(&path, is_dir);
                        if m.is_ignore() {
                            continue;
                        }
                    }

                    // Check override globs.
                    if !self.overrides.is_empty() {
                        let m = self.overrides.matched(&path, is_dir);
                        if m.is_ignore() {
                            continue;
                        }
                    }

                    // Check file types (only for files).
                    if !is_dir && !self.types.is_empty() {
                        let m = self.types.matched(&path, is_dir);
                        if m.is_ignore() {
                            continue;
                        }
                    }

                    // Check max file size.
                    if !is_dir {
                        if let Some(max_size) = self.max_filesize {
                            if let Some(ref md) = entry.metadata {
                                if md.len() > max_size {
                                    continue;
                                }
                            }
                        }
                    }

                    // Check custom filter.
                    if let Some(ref filter) = self.filter_entry {
                        if !filter(&entry) {
                            continue;
                        }
                    }

                    results.push(Ok(entry));
                }
                Err(err) => {
                    results.push(Err(Error::io(io::Error::new(
                        io::ErrorKind::Other,
                        err.to_string(),
                    ))));
                }
            }
        }
    }

    /// Check whether an entry should be included (for single-file roots).
    fn should_include(&self, entry: &DirEntry) -> bool {
        let is_dir = entry.is_dir();

        // Check hidden.
        if self.hidden {
            if let Some(name) = entry.path().file_name() {
                if name.to_string_lossy().starts_with('.') {
                    return false;
                }
            }
        }

        // Check overrides.
        if !self.overrides.is_empty() {
            let m = self.overrides.matched(entry.path(), is_dir);
            if m.is_ignore() {
                return false;
            }
        }

        // Check file types.
        if !is_dir && !self.types.is_empty() {
            let m = self.types.matched(entry.path(), is_dir);
            if m.is_ignore() {
                return false;
            }
        }

        // Check max file size.
        if !is_dir {
            if let Some(max_size) = self.max_filesize {
                if let Ok(md) = entry.metadata() {
                    if md.len() > max_size {
                        return false;
                    }
                }
            }
        }

        // Check custom filter.
        if let Some(ref filter) = self.filter_entry {
            if !filter(entry) {
                return false;
            }
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Walk (single-threaded iterator)
// ---------------------------------------------------------------------------

/// A single-threaded recursive directory walker.
///
/// Created by [`WalkBuilder::build`]. Yields [`DirEntry`] values for
/// each file and directory that passes the configured filters.
pub struct Walk {
    /// The underlying iterator over collected entries.
    inner: Box<dyn Iterator<Item = Result<DirEntry, Error>> + Send>,
}

impl Iterator for Walk {
    type Item = Result<DirEntry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

// ---------------------------------------------------------------------------
// WalkParallel
// ---------------------------------------------------------------------------

/// A multi-threaded recursive directory walker.
///
/// Created by [`WalkBuilder::build_parallel`]. Entries are visited via
/// a [`ParallelVisitor`] trait implementation.
pub struct WalkParallel {
    /// Root paths to walk.
    paths: Vec<PathBuf>,
    /// Maximum directory depth.
    max_depth: Option<usize>,
    /// Whether to follow symbolic links.
    follow_links: bool,
    /// Maximum file size.
    max_filesize: Option<u64>,
    /// Whether to skip hidden files.
    hidden: bool,
    /// Whether to skip all ignore files.
    no_ignore: bool,
    /// Whether to skip VCS ignore files.
    no_ignore_vcs: bool,
    /// Whether to skip global gitignore.
    no_ignore_global: bool,
    /// Whether to skip parent ignore files.
    no_ignore_parent: bool,
    /// File type matcher.
    types: Types,
    /// Override glob matcher.
    overrides: Override,
    /// Number of threads.
    threads: usize,
    /// Sort order.
    sort_by: SortBy,
    /// Whether to stay on the same filesystem.
    same_file_system: bool,
    /// Custom filter.
    filter_entry: Option<Arc<dyn Fn(&DirEntry) -> bool + Send + Sync>>,
}

/// The state returned by a [`ParallelVisitor`] after visiting an entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkState {
    /// Continue walking.
    Continue,
    /// Skip the current directory's remaining children.
    Skip,
    /// Stop walking entirely.
    Quit,
}

/// A visitor that processes directory entries during parallel walking.
///
/// Each thread gets its own visitor instance, created by
/// [`ParallelVisitorBuilder::build`].
pub trait ParallelVisitor: Send {
    /// Visit a single directory entry.
    ///
    /// Return [`WalkState::Continue`] to keep walking,
    /// [`WalkState::Skip`] to skip remaining children of the current
    /// directory, or [`WalkState::Quit`] to stop walking entirely.
    fn visit(&mut self, entry: Result<DirEntry, Error>) -> WalkState;
}

/// A factory for creating thread-local [`ParallelVisitor`] instances.
pub trait ParallelVisitorBuilder<'s>: Send + Sync {
    /// The concrete visitor type.
    type Visitor: ParallelVisitor + 's;

    /// Create a new visitor for a worker thread.
    fn build(&'s self) -> Self::Visitor;
}

impl WalkParallel {
    /// Run the parallel walker, visiting each entry with thread-local
    /// visitors created by the given builder.
    ///
    /// This initial implementation delegates to the single-threaded
    /// [`Walk`] iterator for simplicity, while exposing the correct
    /// parallel API. True work-stealing parallelism can be added later.
    pub fn run<'s, B>(self, builder: &'s B)
    where
        B: ParallelVisitorBuilder<'s>,
    {
        let quit = Arc::new(AtomicBool::new(false));
        let num_threads = self.threads;

        // Build a WalkBuilder with the same configuration and collect all entries.
        let mut walk_builder = WalkBuilder::new(&self.paths[0]);
        for path in self.paths.iter().skip(1) {
            walk_builder.add(path);
        }
        walk_builder
            .max_depth(self.max_depth)
            .follow_links(self.follow_links)
            .max_filesize(self.max_filesize)
            .hidden(self.hidden)
            .no_ignore(self.no_ignore)
            .no_ignore_vcs(self.no_ignore_vcs)
            .no_ignore_global(self.no_ignore_global)
            .no_ignore_parent(self.no_ignore_parent)
            .types(self.types.clone())
            .overrides(self.overrides.clone())
            .same_file_system(self.same_file_system);

        if let Some(ref filter) = self.filter_entry {
            let f = filter.clone();
            walk_builder.filter_entry(move |e| f(e));
        }

        // Collect all entries.
        let all_entries: Vec<Result<DirEntry, Error>> = walk_builder.collect_entries();

        // Partition entries across threads.
        let num_threads = std::cmp::min(num_threads, std::cmp::max(1, all_entries.len()));
        if num_threads == 0 {
            return;
        }

        // Split entries into chunks for each thread.
        let chunk_size = (all_entries.len() + num_threads - 1) / num_threads;

        // We need to move entries into chunks. Use indexed access.
        let mut chunks: Vec<Vec<Result<DirEntry, Error>>> =
            Vec::with_capacity(num_threads);
        let mut iter = all_entries.into_iter();
        for _ in 0..num_threads {
            let chunk: Vec<_> = iter.by_ref().take(chunk_size).collect();
            if !chunk.is_empty() {
                chunks.push(chunk);
            }
        }

        // Use thread::scope to process chunks in parallel.
        std::thread::scope(|s| {
            let quit_ref = &quit;
            let mut handles = Vec::new();

            for chunk in chunks {
                let mut visitor = builder.build();
                let handle = s.spawn(move || {
                    for entry in chunk {
                        if quit_ref.load(Ordering::Relaxed) {
                            break;
                        }
                        match visitor.visit(entry) {
                            WalkState::Continue => {}
                            WalkState::Skip => {}
                            WalkState::Quit => {
                                quit_ref.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.join();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- Error tests ---

    #[test]
    fn test_error_display_io() {
        let err = Error::from(io::Error::new(io::ErrorKind::NotFound, "not found"));
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_error_display_glob() {
        let err = Error::glob("bad pattern");
        assert!(err.to_string().contains("bad pattern"));
    }

    #[test]
    fn test_error_display_parse() {
        let err = Error::parse("bad line");
        assert!(err.to_string().contains("bad line"));
    }

    // --- Match tests ---

    #[test]
    fn test_match_predicates() {
        assert!(Match::Ignore.is_ignore());
        assert!(!Match::Ignore.is_whitelist());
        assert!(!Match::Ignore.is_none());

        assert!(!Match::Whitelist.is_ignore());
        assert!(Match::Whitelist.is_whitelist());
        assert!(!Match::Whitelist.is_none());

        assert!(!Match::None.is_ignore());
        assert!(!Match::None.is_whitelist());
        assert!(Match::None.is_none());
    }

    // --- Types tests ---

    #[test]
    fn test_types_empty() {
        let types = TypesBuilder::new().build().unwrap();
        assert!(types.is_empty());
        assert!(types.matched("foo.rs", false).is_none());
    }

    #[test]
    fn test_types_select_rust() {
        let mut builder = TypesBuilder::new();
        builder.add_defaults();
        builder.select("rust");
        let types = builder.build().unwrap();

        assert!(!types.is_empty());
        assert!(types.matched("foo.rs", false).is_whitelist());
        assert!(types.matched("foo.py", false).is_ignore());
        // Directories pass through.
        assert!(types.matched("src", true).is_none());
    }

    #[test]
    fn test_types_negate() {
        let mut builder = TypesBuilder::new();
        builder.add_defaults();
        builder.negate("rust");
        let types = builder.build().unwrap();

        assert!(!types.is_empty());
        assert!(types.matched("foo.rs", false).is_ignore());
        assert!(types.matched("foo.py", false).is_none());
    }

    #[test]
    fn test_types_add_custom() {
        let mut builder = TypesBuilder::new();
        builder.add("mytype", "*.xyz").unwrap();
        builder.select("mytype");
        let types = builder.build().unwrap();

        assert!(types.matched("foo.xyz", false).is_whitelist());
        assert!(types.matched("foo.abc", false).is_ignore());
    }

    #[test]
    fn test_types_clear() {
        let mut builder = TypesBuilder::new();
        builder.add_defaults();
        builder.select("rust");
        builder.clear("rust");
        // After clearing, "rust" is gone, so select("rust") matches nothing.
        let types = builder.build().unwrap();
        // has_selected is true (selected_names still contains "rust")
        // but no glob patterns, so everything is ignored.
        assert!(types.matched("foo.rs", false).is_ignore());
    }

    // --- Gitignore tests ---

    #[test]
    fn test_gitignore_empty() {
        let gi = Gitignore::empty();
        assert!(gi.is_empty());
        assert!(gi.matched("anything", false).is_none());
    }

    #[test]
    fn test_gitignore_basic_pattern() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "*.o").unwrap();
        let gi = builder.build().unwrap();

        assert!(gi.matched("/project/foo.o", false).is_ignore());
        assert!(gi.matched("/project/sub/bar.o", false).is_ignore());
        assert!(gi.matched("/project/foo.c", false).is_none());
    }

    #[test]
    fn test_gitignore_negation() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "*.o").unwrap();
        builder.add_line(None, "!important.o").unwrap();
        let gi = builder.build().unwrap();

        assert!(gi.matched("/project/foo.o", false).is_ignore());
        assert!(gi.matched("/project/important.o", false).is_whitelist());
    }

    #[test]
    fn test_gitignore_dir_only() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "target/").unwrap();
        let gi = builder.build().unwrap();

        // Should match directories.
        assert!(gi.matched("/project/target", true).is_ignore());
        // Should NOT match files.
        assert!(gi.matched("/project/target", false).is_none());
    }

    #[test]
    fn test_gitignore_comment() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "# this is a comment").unwrap();
        builder.add_line(None, "").unwrap();
        builder.add_line(None, "   ").unwrap();
        let gi = builder.build().unwrap();

        assert!(gi.is_empty());
    }

    #[test]
    fn test_gitignore_slash_pattern() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "build/output").unwrap();
        let gi = builder.build().unwrap();

        // Full-path pattern, should match exact subpath.
        assert!(gi.matched("/project/build/output", false).is_ignore());
        // Should not match basename-only.
        assert!(gi.matched("/project/other/output", false).is_none());
    }

    #[test]
    fn test_gitignore_leading_slash() {
        let mut builder = GitignoreBuilder::new("/project");
        builder.add_line(None, "/build").unwrap();
        let gi = builder.build().unwrap();

        assert!(gi.matched("/project/build", true).is_ignore());
        // Leading slash means root-relative, not at any depth.
        assert!(gi.matched("/project/sub/build", true).is_none());
    }

    // --- Override tests ---

    #[test]
    fn test_override_empty() {
        let ov = Override::empty();
        assert!(ov.is_empty());
        assert!(ov.matched("anything", false).is_none());
    }

    #[test]
    fn test_override_whitelist() {
        let mut builder = OverrideBuilder::new(".");
        builder.add("*.rs").unwrap();
        let ov = builder.build().unwrap();

        assert!(ov.matched("foo.rs", false).is_whitelist());
        assert!(ov.matched("foo.py", false).is_ignore());
    }

    #[test]
    fn test_override_negation() {
        let mut builder = OverrideBuilder::new(".");
        builder.add("*.rs").unwrap();
        builder.add("!test_*.rs").unwrap();
        let ov = builder.build().unwrap();

        assert!(ov.matched("foo.rs", false).is_whitelist());
        assert!(ov.matched("test_foo.rs", false).is_ignore());
    }

    #[test]
    fn test_override_exclusion_only() {
        let mut builder = OverrideBuilder::new(".");
        builder.add("!*.log").unwrap();
        let ov = builder.build().unwrap();

        // No whitelist patterns, so non-matching files pass through.
        assert!(ov.matched("foo.rs", false).is_none());
        // Matching exclusion pattern should ignore.
        assert!(ov.matched("debug.log", false).is_ignore());
    }

    // --- DirEntry tests ---

    #[test]
    fn test_direntry_is_stdin() {
        let entry = DirEntry::from_path("-", 0);
        assert!(entry.is_stdin());

        let entry = DirEntry::from_path("foo.rs", 0);
        assert!(!entry.is_stdin());
    }

    #[test]
    fn test_direntry_file_name() {
        let entry = DirEntry::from_path("/some/path/foo.rs", 0);
        assert_eq!(entry.file_name(), "foo.rs");
    }

    #[test]
    fn test_direntry_depth() {
        let entry = DirEntry::from_path("foo.rs", 3);
        assert_eq!(entry.depth(), 3);
    }

    // --- WalkBuilder basic tests ---

    #[test]
    fn test_walkbuilder_defaults() {
        let builder = WalkBuilder::new(".");
        assert_eq!(builder.paths.len(), 1);
        assert!(builder.hidden);
        assert!(!builder.no_ignore);
        assert!(!builder.follow_links);
    }

    #[test]
    fn test_walkbuilder_add_path() {
        let mut builder = WalkBuilder::new("src");
        builder.add("tests");
        assert_eq!(builder.paths.len(), 2);
    }

    #[test]
    fn test_walkbuilder_chaining() {
        let mut builder = WalkBuilder::new(".");
        builder
            .max_depth(Some(5))
            .follow_links(true)
            .hidden(false)
            .no_ignore(true)
            .threads(4);

        assert_eq!(builder.max_depth, Some(5));
        assert!(builder.follow_links);
        assert!(!builder.hidden);
        assert!(builder.no_ignore);
        assert_eq!(builder.threads, 4);
    }

    // --- WalkState tests ---

    #[test]
    fn test_walkstate_equality() {
        assert_eq!(WalkState::Continue, WalkState::Continue);
        assert_eq!(WalkState::Skip, WalkState::Skip);
        assert_eq!(WalkState::Quit, WalkState::Quit);
        assert_ne!(WalkState::Continue, WalkState::Quit);
    }
}
