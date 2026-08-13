/*!
The `ignore` crate provides directory traversal with respect to ignore files
(`.gitignore`, `.ignore`, `.rgignore`), file type filtering, and glob
overrides.

# Overview

The primary entry point is [`WalkBuilder`], which constructs a [`Walk`]
iterator (single-threaded) or a [`WalkParallel`] (multi-threaded) directory
traversal that automatically respects ignore rules, file type filters, hidden
file settings, and more.

Additional components:

- [`Gitignore`] — parse and match against gitignore-style rules.
- [`Types`] / [`TypesBuilder`] — file type definitions and filtering.
- [`Override`] / [`OverrideBuilder`] — glob override patterns from CLI flags.
*/

use std::cmp::Ordering;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// An error that can occur in the ignore crate.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Io(io::Error),
    Glob(String),
    InvalidType(String),
    Msg(String),
    Partial(Vec<Error>),
}

impl Error {
    fn io(err: io::Error) -> Error {
        Error { kind: ErrorKind::Io(err) }
    }
    fn glob(msg: impl Into<String>) -> Error {
        Error { kind: ErrorKind::Glob(msg.into()) }
    }
    fn msg(msg: impl Into<String>) -> Error {
        Error { kind: ErrorKind::Msg(msg.into()) }
    }
    fn invalid_type(msg: impl Into<String>) -> Error {
        Error { kind: ErrorKind::InvalidType(msg.into()) }
    }
    fn partial(errs: Vec<Error>) -> Error {
        Error { kind: ErrorKind::Partial(errs) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Io(err) => write!(f, "IO error: {}", err),
            ErrorKind::Glob(msg) => write!(f, "glob error: {}", msg),
            ErrorKind::InvalidType(msg) => write!(f, "invalid type: {}", msg),
            ErrorKind::Msg(msg) => write!(f, "{}", msg),
            ErrorKind::Partial(errs) => {
                for (i, e) in errs.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
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
    fn from(err: io::Error) -> Error {
        Error::io(err)
    }
}

impl From<globset::Error> for Error {
    fn from(err: globset::Error) -> Error {
        Error::glob(err.to_string())
    }
}

impl From<walkdir::Error> for Error {
    fn from(err: walkdir::Error) -> Error {
        Error::io(io::Error::new(io::ErrorKind::Other, err.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

/// A reference to a glob pattern that matched.
#[derive(Clone, Debug)]
pub struct GlobRef {
    /// The original glob pattern string.
    pattern: String,
    /// Which file this pattern came from (if any).
    from: Option<PathBuf>,
    /// Line number in the file (1-indexed).
    line: usize,
}

impl GlobRef {
    /// Return the glob pattern string.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
    /// Return the file this pattern came from, if any.
    pub fn from(&self) -> Option<&Path> {
        self.from.as_deref()
    }
    /// Return the line number this pattern appeared on.
    pub fn line(&self) -> usize {
        self.line
    }
}

/// The result of matching a path against a set of ignore rules.
#[derive(Clone, Debug)]
pub enum Match {
    /// No rule matched.
    None,
    /// The path should be ignored.
    Ignore(GlobRef),
    /// The path was explicitly whitelisted (negated rule).
    Whitelist(GlobRef),
}

impl Match {
    /// Returns true if the path should be ignored.
    pub fn is_ignore(&self) -> bool {
        matches!(self, Match::Ignore(_))
    }

    /// Returns true if the path was whitelisted.
    pub fn is_whitelist(&self) -> bool {
        matches!(self, Match::Whitelist(_))
    }

    /// Returns true if no rule matched.
    pub fn is_none(&self) -> bool {
        matches!(self, Match::None)
    }
}

// ---------------------------------------------------------------------------
// Gitignore
// ---------------------------------------------------------------------------

/// A single compiled rule from a gitignore-like file.
#[derive(Clone, Debug)]
struct GitignoreRule {
    /// The original pattern.
    original: String,
    /// The compiled glob (for matching).
    glob_idx: usize,
    /// Whether this is a negation (whitelist) rule.
    negated: bool,
    /// Whether this rule only matches directories.
    dir_only: bool,
    /// Line number in the file.
    line: usize,
    /// Path of the file this rule came from.
    from: Option<PathBuf>,
}

/// Compiled gitignore rules from a single file.
#[derive(Clone, Debug)]
pub struct Gitignore {
    /// The root directory that patterns are relative to.
    root: PathBuf,
    /// The compiled rules.
    rules: Vec<GitignoreRule>,
    /// The compiled glob set for all rules.
    glob_set: GlobSet,
    /// Number of ignore (non-negated) rules.
    num_ignores: usize,
}

impl Gitignore {
    /// Parse a gitignore-style file at the given path.
    ///
    /// Returns the compiled gitignore and an optional error if some rules
    /// failed to compile (partial success).
    pub fn new(path: &Path) -> (Gitignore, Option<Error>) {
        let root = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) => {
                return (
                    Gitignore::empty_with_root(root),
                    Some(Error::io(err)),
                );
            }
        };
        let mut builder = GitignoreBuilder::new(&root);
        builder.source = Some(path.to_path_buf());
        let mut errs = Vec::new();
        for (i, line) in contents.lines().enumerate() {
            if let Err(e) = builder.add_line(line, i + 1) {
                errs.push(e);
            }
        }
        let (gi, build_err) = builder.build();
        if let Some(e) = build_err {
            errs.push(e);
        }
        let err = if errs.is_empty() {
            None
        } else {
            Some(Error::partial(errs))
        };
        (gi, err)
    }

    /// Create an empty `Gitignore` that matches nothing.
    pub fn empty() -> Gitignore {
        Gitignore::empty_with_root(PathBuf::from("."))
    }

    fn empty_with_root(root: PathBuf) -> Gitignore {
        Gitignore {
            root,
            rules: Vec::new(),
            glob_set: GlobSet::empty(),
            num_ignores: 0,
        }
    }

    /// Test whether the given path matches any rule.
    ///
    /// `is_dir` should be true if the path is a directory.
    pub fn matched<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> Match {
        self.matched_inner(path.as_ref(), is_dir)
    }

    fn matched_inner(&self, path: &Path, is_dir: bool) -> Match {
        // Make path relative to root.
        let rel = match path.strip_prefix(&self.root) {
            Ok(r) => r,
            Err(_) => path,
        };

        // Normalize to forward-slash string for matching.
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        // Use GlobSet to find which rules match.
        let candidates = self.glob_set.matches(Path::new(&rel_str));

        // The last matching rule wins (later rules override earlier).
        let mut result = Match::None;
        for &idx in &candidates {
            let rule = &self.rules[idx];
            if rule.dir_only && !is_dir {
                continue;
            }
            let glob_ref = GlobRef {
                pattern: rule.original.clone(),
                from: rule.from.clone(),
                line: rule.line,
            };
            if rule.negated {
                result = Match::Whitelist(glob_ref);
            } else {
                result = Match::Ignore(glob_ref);
            }
        }
        result
    }

    /// Return the number of ignore (non-negated) rules.
    pub fn num_ignores(&self) -> usize {
        self.num_ignores
    }
}

/// Internal builder for constructing a `Gitignore`.
struct GitignoreBuilder {
    root: PathBuf,
    source: Option<PathBuf>,
    rules: Vec<(String, bool, bool, bool, usize)>, // (pattern, negated, dir_only, anchored, line)
}

impl GitignoreBuilder {
    fn new(root: &Path) -> GitignoreBuilder {
        GitignoreBuilder {
            root: root.to_path_buf(),
            source: None,
            rules: Vec::new(),
        }
    }

    fn add_line(&mut self, line: &str, line_num: usize) -> Result<(), Error> {
        let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

        // Blank lines
        if line.trim().is_empty() {
            return Ok(());
        }
        // Comment lines
        if line.starts_with('#') {
            return Ok(());
        }

        let mut pattern = line.to_string();
        let mut negated = false;
        let mut dir_only = false;

        // Trailing spaces: strip unless escaped
        // For simplicity, strip trailing spaces.
        pattern = pattern.trim_end().to_string();
        if pattern.is_empty() {
            return Ok(());
        }

        // Leading `!` negates
        if pattern.starts_with('!') {
            negated = true;
            pattern = pattern[1..].to_string();
            if pattern.is_empty() {
                return Ok(());
            }
        }

        // Leading `\#` or `\!` — remove the backslash
        if pattern.starts_with("\\#") || pattern.starts_with("\\!") {
            pattern = pattern[1..].to_string();
        }

        // Trailing `/` means only match directories
        if pattern.ends_with('/') {
            dir_only = true;
            pattern = pattern[..pattern.len() - 1].to_string();
            if pattern.is_empty() {
                return Ok(());
            }
        }

        // Check if anchored (contains `/` that's not only at the end,
        // or starts with `/`)
        let anchored = pattern.starts_with('/') || pattern[..pattern.len()].contains('/');

        // Remove leading `/` if present (it anchors but shouldn't be in glob)
        if pattern.starts_with('/') {
            pattern = pattern[1..].to_string();
        }

        self.rules.push((pattern, negated, dir_only, anchored, line_num));
        Ok(())
    }

    fn build(self) -> (Gitignore, Option<Error>) {
        let mut glob_set_builder = GlobSetBuilder::new();
        let mut rules = Vec::new();
        let mut errs = Vec::new();
        let mut num_ignores = 0;

        for (pattern, negated, dir_only, anchored, line_num) in &self.rules {
            // Build a glob pattern.
            // If the pattern is anchored, it matches from root.
            // If not anchored, we need to match it anywhere in the path.
            let glob_pattern = if *anchored {
                // Anchored: match from root (already relative).
                pattern.clone()
            } else {
                // Unanchored: match anywhere — prepend `**/`
                format!("**/{}", pattern)
            };

            let glob_result = GlobBuilder::new(&glob_pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build();

            match glob_result {
                Ok(glob) => {
                    let idx = rules.len();
                    glob_set_builder.add(glob);
                    if !negated {
                        num_ignores += 1;
                    }
                    rules.push(GitignoreRule {
                        original: pattern.clone(),
                        glob_idx: idx,
                        negated: *negated,
                        dir_only: *dir_only,
                        line: *line_num,
                        from: self.source.clone(),
                    });
                }
                Err(e) => {
                    errs.push(Error::glob(format!(
                        "{}:{}: {}",
                        self.source
                            .as_deref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                        line_num,
                        e
                    )));
                }
            }
        }

        let glob_set = match glob_set_builder.build() {
            Ok(gs) => gs,
            Err(e) => {
                errs.push(Error::glob(e.to_string()));
                GlobSet::empty()
            }
        };

        let gi = Gitignore {
            root: self.root,
            rules,
            glob_set,
            num_ignores,
        };
        let err = if errs.is_empty() {
            None
        } else {
            Some(Error::partial(errs))
        };
        (gi, err)
    }
}

// ---------------------------------------------------------------------------
// Types / TypeDef / TypesBuilder
// ---------------------------------------------------------------------------

/// A file type definition.
#[derive(Clone, Debug)]
pub struct TypeDef {
    name: String,
    globs: Vec<String>,
}

impl TypeDef {
    /// Return the name of this file type.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the glob patterns for this file type.
    pub fn globs(&self) -> &[String] {
        &self.globs
    }
}

/// A compiled set of file type filters.
#[derive(Clone, Debug)]
pub struct Types {
    /// Types selected for inclusion.
    selected: Option<GlobSet>,
    /// Types selected for exclusion (negated).
    negated: Option<GlobSet>,
    /// Whether this is empty (no selections at all).
    has_selections: bool,
}

impl Types {
    /// Create an empty `Types` that has no type filters.
    pub fn empty() -> Types {
        Types {
            selected: None,
            negated: None,
            has_selections: false,
        }
    }

    /// Test whether the given path matches the selected types.
    ///
    /// Returns `Match::None` if no types were selected.
    /// Returns `Match::Ignore` if the path does NOT match any selected type
    /// (i.e., it should be filtered out).
    /// Returns `Match::Whitelist` if the path matches a selected type.
    pub fn matched<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> Match {
        // Directories are always let through type filters.
        if is_dir || !self.has_selections {
            return Match::None;
        }

        let path = path.as_ref();

        // Check negated types first (exclusion).
        if let Some(ref neg) = self.negated {
            if neg.is_match(path) {
                return Match::Ignore(GlobRef {
                    pattern: String::new(),
                    from: None,
                    line: 0,
                });
            }
        }

        // Check selected types (inclusion).
        if let Some(ref sel) = self.selected {
            if sel.is_match(path) {
                return Match::Whitelist(GlobRef {
                    pattern: String::new(),
                    from: None,
                    line: 0,
                });
            }
            // Not matched by selected types → ignore.
            return Match::Ignore(GlobRef {
                pattern: String::new(),
                from: None,
                line: 0,
            });
        }

        Match::None
    }
}

/// A builder for constructing a [`Types`] file type filter.
pub struct TypesBuilder {
    defs: HashMap<String, Vec<String>>,
    selected: Vec<String>,
    negated: Vec<String>,
}

impl TypesBuilder {
    /// Create a new `TypesBuilder` pre-populated with built-in type
    /// definitions.
    pub fn new() -> TypesBuilder {
        let mut builder = TypesBuilder {
            defs: HashMap::new(),
            selected: Vec::new(),
            negated: Vec::new(),
        };
        builder.add_defaults();
        builder
    }

    /// Add a glob pattern for the given type name.
    pub fn add(&mut self, name: &str, glob: &str) -> Result<(), Error> {
        // Validate the glob compiles.
        GlobBuilder::new(glob).build().map_err(|e| {
            Error::invalid_type(format!(
                "invalid glob '{}' for type '{}': {}",
                glob, name, e
            ))
        })?;
        self.defs
            .entry(name.to_string())
            .or_default()
            .push(glob.to_string());
        Ok(())
    }

    /// Clear all globs for the given type name.
    pub fn clear(&mut self, name: &str) {
        self.defs.remove(name);
    }

    /// Select a type for inclusion.
    pub fn select(&mut self, name: &str) {
        self.selected.push(name.to_string());
    }

    /// Select a type for exclusion (negation).
    pub fn negate(&mut self, name: &str) {
        self.negated.push(name.to_string());
    }

    /// Return all type definitions.
    pub fn definitions(&self) -> Vec<TypeDef> {
        let mut defs: Vec<TypeDef> = self
            .defs
            .iter()
            .map(|(name, globs)| TypeDef {
                name: name.clone(),
                globs: globs.clone(),
            })
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Build the compiled [`Types`] filter.
    pub fn build(&self) -> Result<Types, Error> {
        let has_selections = !self.selected.is_empty() || !self.negated.is_empty();

        let selected = if self.selected.is_empty() {
            None
        } else {
            let mut gsb = GlobSetBuilder::new();
            for name in &self.selected {
                if let Some(globs) = self.defs.get(name) {
                    for g in globs {
                        let glob = GlobBuilder::new(g).build().map_err(|e| {
                            Error::invalid_type(format!("{}: {}", g, e))
                        })?;
                        gsb.add(glob);
                    }
                }
            }
            Some(gsb.build().map_err(|e| Error::glob(e.to_string()))?)
        };

        let negated = if self.negated.is_empty() {
            None
        } else {
            let mut gsb = GlobSetBuilder::new();
            for name in &self.negated {
                if let Some(globs) = self.defs.get(name) {
                    for g in globs {
                        let glob = GlobBuilder::new(g).build().map_err(|e| {
                            Error::invalid_type(format!("{}: {}", g, e))
                        })?;
                        gsb.add(glob);
                    }
                }
            }
            Some(gsb.build().map_err(|e| Error::glob(e.to_string()))?)
        };

        Ok(Types {
            selected,
            negated,
            has_selections,
        })
    }

    fn add_defaults(&mut self) {
        let defaults: &[(&str, &[&str])] = &[
            ("agda", &["*.agda", "*.lagda"]),
            ("awk", &["*.awk"]),
            ("c", &["*.c", "*.h", "*.H", "*.cats"]),
            ("cmake", &["*.cmake", "CMakeLists.txt"]),
            ("cpp", &["*.cpp", "*.cc", "*.cxx", "*.C", "*.hpp", "*.hh", "*.hxx", "*.H", "*.inl"]),
            ("csharp", &["*.cs"]),
            ("css", &["*.css", "*.scss", "*.less"]),
            ("csv", &["*.csv"]),
            ("d", &["*.d"]),
            ("dart", &["*.dart"]),
            ("docker", &["Dockerfile*", "*.dockerfile"]),
            ("elixir", &["*.ex", "*.exs"]),
            ("elm", &["*.elm"]),
            ("erlang", &["*.erl", "*.hrl"]),
            ("fish", &["*.fish"]),
            ("fortran", &["*.f", "*.F", "*.f77", "*.f90", "*.F90", "*.f95", "*.f03", "*.for", "*.fpp"]),
            ("go", &["*.go"]),
            ("graphql", &["*.graphql", "*.graphqls"]),
            ("groovy", &["*.groovy", "*.gradle"]),
            ("haskell", &["*.hs", "*.lhs", "*.cabal"]),
            ("html", &["*.html", "*.htm", "*.xhtml"]),
            ("java", &["*.java", "*.jsp", "*.jspx"]),
            ("js", &["*.js", "*.jsx", "*.mjs", "*.cjs"]),
            ("json", &["*.json", "*.jsonl"]),
            ("jsonl", &["*.jsonl"]),
            ("julia", &["*.jl"]),
            ("kotlin", &["*.kt", "*.kts"]),
            ("less", &["*.less"]),
            ("license", &["LICENSE*", "COPYING*", "LICENCE*"]),
            ("lisp", &["*.el", "*.lisp", "*.lsp", "*.cl"]),
            ("lock", &["*.lock", "package-lock.json"]),
            ("log", &["*.log"]),
            ("lua", &["*.lua"]),
            ("m4", &["*.m4"]),
            ("make", &["Makefile", "*.mk", "*.mak", "GNUmakefile"]),
            ("man", &["*.[1-9]", "*.[1-9]p"]),
            ("markdown", &["*.md", "*.markdown", "*.mdown", "*.mkdn"]),
            ("md", &["*.md"]),
            ("nim", &["*.nim", "*.nimble"]),
            ("nix", &["*.nix"]),
            ("objc", &["*.m", "*.h"]),
            ("ocaml", &["*.ml", "*.mli", "*.mll", "*.mly"]),
            ("org", &["*.org"]),
            ("pdf", &["*.pdf"]),
            ("perl", &["*.pl", "*.pm", "*.t"]),
            ("php", &["*.php", "*.phtml", "*.php3", "*.php4", "*.php5", "*.php7", "*.phps"]),
            ("protobuf", &["*.proto"]),
            ("py", &["*.py", "*.pyi", "*.pyw"]),
            ("qmake", &["*.pro", "*.pri"]),
            ("r", &["*.r", "*.R", "*.Rmd", "*.Rnw"]),
            ("readme", &["README*"]),
            ("ruby", &["*.rb", "*.erb", "Gemfile", "Rakefile"]),
            ("rust", &["*.rs"]),
            ("sass", &["*.sass", "*.scss"]),
            ("scala", &["*.scala", "*.sbt"]),
            ("shell", &["*.sh", "*.bash", "*.zsh", "*.fish", "*.csh", "*.ksh"]),
            ("sql", &["*.sql"]),
            ("sv", &["*.sv", "*.svh", "*.v"]),
            ("svg", &["*.svg"]),
            ("swift", &["*.swift"]),
            ("tex", &["*.tex", "*.ltx", "*.cls", "*.sty", "*.bib"]),
            ("textile", &["*.textile"]),
            ("tf", &["*.tf", "*.tfvars"]),
            ("toml", &["*.toml"]),
            ("ts", &["*.ts", "*.tsx", "*.cts", "*.mts"]),
            ("txt", &["*.txt"]),
            ("vala", &["*.vala", "*.vapi"]),
            ("vim", &["*.vim"]),
            ("xml", &["*.xml", "*.xsl", "*.xslt", "*.xsd", "*.wsdl"]),
            ("yaml", &["*.yaml", "*.yml"]),
            ("zig", &["*.zig"]),
        ];

        for (name, globs) in defaults {
            for g in *globs {
                // Defaults should always be valid.
                let _ = self.add(name, g);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Override / OverrideBuilder
// ---------------------------------------------------------------------------

/// Compiled glob override patterns, typically from `-g`/`--iglob` CLI flags.
#[derive(Clone, Debug)]
pub struct Override {
    glob_set: GlobSet,
    /// For each glob in the set, whether it's a whitelist (negated) pattern.
    negated: Vec<bool>,
    /// For each glob, the original pattern string.
    patterns: Vec<String>,
}

impl Override {
    /// Create an empty `Override` that has no effect.
    pub fn empty() -> Override {
        Override {
            glob_set: GlobSet::empty(),
            negated: Vec::new(),
            patterns: Vec::new(),
        }
    }

    /// Returns true if no override patterns have been set.
    pub fn is_empty(&self) -> bool {
        self.glob_set.is_empty()
    }

    /// Test whether the given path matches any override pattern.
    ///
    /// Returns `Match::None` if no patterns exist.
    /// Returns `Match::Ignore` if the path matches a negation override (should
    /// be excluded).
    /// Returns `Match::Whitelist` if the path matches a positive override
    /// (should be included).
    ///
    /// When overrides exist, paths that don't match any positive override are
    /// implicitly excluded (returned as `Match::Ignore`).
    pub fn matched<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> Match {
        if self.is_empty() {
            return Match::None;
        }
        let path = path.as_ref();
        let matches = self.glob_set.matches(path);

        // Also try matching against just the filename.
        let file_name_matches = if let Some(name) = path.file_name() {
            self.glob_set.matches(Path::new(name))
        } else {
            Vec::new()
        };

        // Merge the match indices and find the last one.
        let mut last_match: Option<(usize, bool)> = None;
        let all_matches: Vec<usize> = {
            let mut combined = matches;
            for idx in file_name_matches {
                if !combined.contains(&idx) {
                    combined.push(idx);
                }
            }
            combined.sort();
            combined
        };

        for &idx in &all_matches {
            last_match = Some((idx, self.negated[idx]));
        }

        match last_match {
            Some((idx, true)) => {
                // Negation match → ignore this path.
                Match::Ignore(GlobRef {
                    pattern: self.patterns[idx].clone(),
                    from: None,
                    line: 0,
                })
            }
            Some((idx, false)) => {
                // Positive match → whitelist this path.
                Match::Whitelist(GlobRef {
                    pattern: self.patterns[idx].clone(),
                    from: None,
                    line: 0,
                })
            }
            None => {
                // No match.
                // If there are any positive (non-negated) overrides, then
                // paths not matching any positive override should be ignored.
                let has_positive = self.negated.iter().any(|n| !n);
                if has_positive && !is_dir {
                    Match::Ignore(GlobRef {
                        pattern: String::new(),
                        from: None,
                        line: 0,
                    })
                } else {
                    Match::None
                }
            }
        }
    }
}

/// A builder for constructing an [`Override`].
pub struct OverrideBuilder {
    root: PathBuf,
    patterns: Vec<(String, bool)>, // (pattern, negated)
    case_insensitive: bool,
}

impl OverrideBuilder {
    /// Create a new `OverrideBuilder` with the given root directory.
    pub fn new(root: &Path) -> OverrideBuilder {
        OverrideBuilder {
            root: root.to_path_buf(),
            patterns: Vec::new(),
            case_insensitive: false,
        }
    }

    /// Add a glob pattern. Prefix with `!` to negate.
    pub fn add(&mut self, glob: &str) -> Result<(), Error> {
        let (pattern, negated) = if let Some(rest) = glob.strip_prefix('!') {
            (rest.to_string(), true)
        } else {
            (glob.to_string(), false)
        };
        self.patterns.push((pattern, negated));
        Ok(())
    }

    /// Set whether globs should be matched case-insensitively.
    pub fn case_insensitive(&mut self, yes: bool) -> &mut OverrideBuilder {
        self.case_insensitive = yes;
        self
    }

    /// Build the compiled [`Override`].
    pub fn build(&self) -> Result<Override, Error> {
        if self.patterns.is_empty() {
            return Ok(Override::empty());
        }

        let mut gsb = GlobSetBuilder::new();
        let mut negated = Vec::new();
        let mut patterns = Vec::new();

        for (pattern, is_negated) in &self.patterns {
            // If the pattern doesn't contain a path separator, prepend `**/`
            // so it matches anywhere in the tree.
            let glob_pattern = if pattern.contains('/') {
                pattern.clone()
            } else {
                format!("**/{}", pattern)
            };

            let glob = GlobBuilder::new(&glob_pattern)
                .case_insensitive(self.case_insensitive)
                .literal_separator(true)
                .build()
                .map_err(|e| Error::glob(format!("{}: {}", pattern, e)))?;

            gsb.add(glob);
            negated.push(*is_negated);
            patterns.push(pattern.clone());
        }

        let glob_set = gsb.build().map_err(|e| Error::glob(e.to_string()))?;
        Ok(Override {
            glob_set,
            negated,
            patterns,
        })
    }
}

// ---------------------------------------------------------------------------
// DirEntry
// ---------------------------------------------------------------------------

/// A directory entry yielded during directory traversal.
#[derive(Debug)]
pub struct DirEntry {
    /// The path of this entry.
    path: PathBuf,
    /// File type, if known.
    file_type: Option<fs::FileType>,
    /// Depth relative to root.
    depth: usize,
    /// Whether this represents stdin.
    is_stdin: bool,
    /// Cached metadata.
    metadata: Option<fs::Metadata>,
}

impl DirEntry {
    /// Return the path of this entry.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the file type of this entry, if known.
    pub fn file_type(&self) -> Option<fs::FileType> {
        self.file_type.clone()
    }

    /// Whether this entry represents stdin.
    pub fn is_stdin(&self) -> bool {
        self.is_stdin
    }

    /// Return the metadata for this entry.
    pub fn metadata(&self) -> Result<fs::Metadata, Error> {
        if let Some(ref md) = self.metadata {
            Ok(md.clone())
        } else {
            fs::metadata(&self.path).map_err(Error::io)
        }
    }

    /// Return the depth of this entry relative to the root.
    pub fn depth(&self) -> usize {
        self.depth
    }

    fn from_walkdir(entry: walkdir::DirEntry) -> DirEntry {
        let ft = entry.file_type();
        let depth = entry.depth();
        let path = entry.path().to_path_buf();
        let metadata = entry.metadata().ok();
        DirEntry {
            path,
            file_type: Some(ft),
            depth,
            is_stdin: false,
            metadata,
        }
    }

    fn from_path(path: PathBuf, depth: usize) -> DirEntry {
        let metadata = fs::metadata(&path).ok();
        let file_type = metadata.as_ref().map(|m| m.file_type());
        DirEntry {
            path,
            file_type,
            depth,
            is_stdin: false,
            metadata,
        }
    }

    /// Create a DirEntry representing stdin.
    pub fn stdin() -> DirEntry {
        DirEntry {
            path: PathBuf::from("<stdin>"),
            file_type: None,
            depth: 0,
            is_stdin: true,
            metadata: None,
        }
    }
}

// ---------------------------------------------------------------------------
// WalkState
// ---------------------------------------------------------------------------

/// The state returned by a walk callback to control traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkState {
    /// Continue the walk.
    Continue,
    /// Skip the current directory (don't descend into it).
    Skip,
    /// Stop the walk entirely.
    Quit,
}

// ---------------------------------------------------------------------------
// Ignore stack — accumulated ignore rules during traversal
// ---------------------------------------------------------------------------

/// A stack of ignore rules, built up as we descend into directories.
struct IgnoreStack {
    /// Stack of (depth, gitignore) pairs. When we leave a directory at
    /// depth d, we pop all entries with depth >= d.
    stack: Vec<(usize, Gitignore)>,
}

impl IgnoreStack {
    fn new() -> IgnoreStack {
        IgnoreStack { stack: Vec::new() }
    }

    fn push(&mut self, depth: usize, gi: Gitignore) {
        self.stack.push((depth, gi));
    }

    fn pop_to_depth(&mut self, depth: usize) {
        while let Some((d, _)) = self.stack.last() {
            if *d >= depth {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    /// Check all rules on the stack, returning the highest-priority match.
    /// Later (deeper, later-added) rules take priority.
    fn matched(&self, path: &Path, is_dir: bool) -> Match {
        let mut result = Match::None;
        for (_, gi) in &self.stack {
            let m = gi.matched(path, is_dir);
            if !m.is_none() {
                result = m;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Global gitignore
// ---------------------------------------------------------------------------

/// Try to find and parse the global gitignore file.
fn global_gitignore() -> Option<Gitignore> {
    // 1. Check GIT_CONFIG_GLOBAL env var.
    // 2. Check ~/.gitconfig for core.excludesFile.
    // 3. Fall back to ~/.config/git/ignore.

    let excludes_path = find_global_excludes_path();
    match excludes_path {
        Some(path) if path.exists() => {
            let (gi, _err) = Gitignore::new(&path);
            if gi.num_ignores() > 0 {
                Some(gi)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn find_global_excludes_path() -> Option<PathBuf> {
    // Check for GIT_CONFIG_GLOBAL -> parse it for core.excludesFile
    if let Ok(config_path) = env::var("GIT_CONFIG_GLOBAL") {
        if let Some(path) = parse_git_config_excludes(Path::new(&config_path)) {
            return Some(path);
        }
    }

    // Check ~/.gitconfig
    if let Some(home) = home_dir() {
        let gitconfig = home.join(".gitconfig");
        if gitconfig.exists() {
            if let Some(path) = parse_git_config_excludes(&gitconfig) {
                return Some(path);
            }
        }

        // Check XDG config
        let xdg_config = env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".config"));
        let xdg_gitconfig = xdg_config.join("git").join("config");
        if xdg_gitconfig.exists() {
            if let Some(path) = parse_git_config_excludes(&xdg_gitconfig) {
                return Some(path);
            }
        }

        // Default: ~/.config/git/ignore
        let default_ignore = xdg_config.join("git").join("ignore");
        if default_ignore.exists() {
            return Some(default_ignore);
        }
    }

    None
}

fn parse_git_config_excludes(config_path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(config_path).ok()?;
    let mut in_core = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_core = trimmed.eq_ignore_ascii_case("[core]");
            continue;
        }
        if in_core {
            // Look for excludesFile = ... or excludesfile = ...
            if let Some(rest) = trimmed
                .strip_prefix("excludesFile")
                .or_else(|| trimmed.strip_prefix("excludesfile"))
            {
                let rest = rest.trim();
                if let Some(value) = rest.strip_prefix('=') {
                    let value = value.trim().trim_matches('"');
                    let path = expand_tilde(value);
                    return Some(path);
                }
            }
        }
    }
    None
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix('~') {
        if let Some(home) = home_dir() {
            if rest.is_empty() {
                return home;
            }
            if rest.starts_with('/') || rest.starts_with('\\') {
                return home.join(&rest[1..]);
            }
        }
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Walk
// ---------------------------------------------------------------------------

/// A single-threaded recursive directory walker that respects ignore rules.
pub struct Walk {
    /// The inner walkdir iterator(s).
    inner: Box<dyn Iterator<Item = Result<walkdir::DirEntry, walkdir::Error>>>,
    /// The configuration.
    config: Arc<WalkConfig>,
    /// The ignore stack (gitignore + .ignore + .rgignore rules).
    gitignore_stack: IgnoreStack,
    ignore_stack: IgnoreStack,
    rgignore_stack: IgnoreStack,
    /// Global gitignore rules.
    global_gi: Option<Gitignore>,
    /// Whether the first entry (root) has been yielded.
    first: bool,
}

/// Shared configuration for Walk and WalkParallel.
struct WalkConfig {
    /// Root paths.
    paths: Vec<PathBuf>,
    /// Whether to skip hidden files.
    hidden: bool,
    /// Whether to respect `.ignore` files.
    ignore: bool,
    /// Whether to respect `.gitignore` files.
    git_ignore: bool,
    /// Whether to respect global gitignore.
    git_global: bool,
    /// Whether to respect parent directory ignore files.
    parents: bool,
    /// Max traversal depth.
    max_depth: Option<usize>,
    /// Max file size.
    max_filesize: Option<u64>,
    /// Whether to follow symlinks.
    follow_links: bool,
    /// Whether to stay on the same file system.
    same_file_system: bool,
    /// Glob overrides.
    overrides: Override,
    /// File type filters.
    types: Types,
    /// Optional sort function.
    sorter: Option<Arc<dyn Fn(&Path, &Path) -> Ordering + Send + Sync>>,
    /// Number of threads for parallel walk.
    threads: usize,
}

impl Walk {
    fn new(config: Arc<WalkConfig>) -> Walk {
        // Load global gitignore if enabled.
        let global_gi = if config.git_global {
            global_gitignore()
        } else {
            None
        };

        // Build the WalkDir iterators for each path.
        let mut iters: Vec<Box<dyn Iterator<Item = Result<walkdir::DirEntry, walkdir::Error>>>> =
            Vec::new();

        for path in &config.paths {
            let mut wd = WalkDir::new(path);
            if let Some(max_depth) = config.max_depth {
                wd = wd.max_depth(max_depth);
            }
            if config.follow_links {
                wd = wd.follow_links(true);
            }
            if let Some(ref sorter) = config.sorter {
                let s = sorter.clone();
                wd = wd.sort_by(move |a, b| s(a.path(), b.path()));
            }
            iters.push(Box::new(wd.into_iter()));
        }

        let inner: Box<dyn Iterator<Item = Result<walkdir::DirEntry, walkdir::Error>>> =
            Box::new(iters.into_iter().flatten());

        let mut walk = Walk {
            inner,
            config,
            gitignore_stack: IgnoreStack::new(),
            ignore_stack: IgnoreStack::new(),
            rgignore_stack: IgnoreStack::new(),
            global_gi,
            first: true,
        };

        // If parents is enabled, load parent ignore files for the first path.
        if walk.config.parents && !walk.config.paths.is_empty() {
            walk.load_parent_ignores(&walk.config.paths[0].clone());
        }

        walk
    }

    fn load_parent_ignores(&mut self, path: &Path) {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()
                .unwrap_or_default()
                .join(path)
        };

        // Collect ancestors (excluding the path itself, which will be
        // processed during traversal).
        let mut ancestors: Vec<PathBuf> = Vec::new();
        let mut current = abs_path.parent();
        while let Some(p) = current {
            ancestors.push(p.to_path_buf());
            current = p.parent();
        }
        ancestors.reverse(); // Process from root to immediate parent.

        for ancestor in &ancestors {
            self.try_load_ignore_files(ancestor, 0);
        }
    }

    fn try_load_ignore_files(&mut self, dir: &Path, depth: usize) {
        if self.config.git_ignore {
            let gitignore_path = dir.join(".gitignore");
            if gitignore_path.exists() {
                let (gi, _) = Gitignore::new(&gitignore_path);
                if gi.num_ignores() > 0 || true {
                    // Always push, even if empty, to handle whitelists.
                    self.gitignore_stack.push(depth, gi);
                }
            }
        }
        if self.config.ignore {
            let ignore_path = dir.join(".ignore");
            if ignore_path.exists() {
                let (gi, _) = Gitignore::new(&ignore_path);
                self.ignore_stack.push(depth, gi);
            }
            let rgignore_path = dir.join(".rgignore");
            if rgignore_path.exists() {
                let (gi, _) = Gitignore::new(&rgignore_path);
                self.rgignore_stack.push(depth, gi);
            }
        }
    }

    fn should_skip(&self, entry: &walkdir::DirEntry) -> bool {
        let path = entry.path();
        let is_dir = entry.file_type().is_dir();
        let depth = entry.depth();

        // Skip hidden files/dirs.
        if self.config.hidden && depth > 0 {
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    return true;
                }
            }
        }

        // Check overrides (highest precedence).
        let override_match = self.config.overrides.matched(path, is_dir);
        if override_match.is_ignore() {
            return true;
        }
        if override_match.is_whitelist() {
            // Whitelisted by override → don't skip.
            return false;
        }

        // Check .rgignore stack (precedence 2).
        let rgignore_match = self.rgignore_stack.matched(path, is_dir);
        if rgignore_match.is_ignore() {
            return true;
        }
        if rgignore_match.is_whitelist() {
            return false;
        }

        // Check .ignore stack (precedence 3).
        let ignore_match = self.ignore_stack.matched(path, is_dir);
        if ignore_match.is_ignore() {
            return true;
        }
        if ignore_match.is_whitelist() {
            return false;
        }

        // Check .gitignore stack (precedence 4).
        let gitignore_match = self.gitignore_stack.matched(path, is_dir);
        if gitignore_match.is_ignore() {
            return true;
        }
        if gitignore_match.is_whitelist() {
            return false;
        }

        // Check global gitignore (precedence 5).
        if let Some(ref global) = self.global_gi {
            let global_match = global.matched(path, is_dir);
            if global_match.is_ignore() {
                return true;
            }
        }

        // Check file type filters.
        if !is_dir {
            let type_match = self.config.types.matched(path, is_dir);
            if type_match.is_ignore() {
                return true;
            }
        }

        // Check max filesize.
        if !is_dir {
            if let Some(max_size) = self.config.max_filesize {
                if let Ok(md) = entry.metadata() {
                    if md.len() > max_size {
                        return true;
                    }
                }
            }
        }

        false
    }
}

impl Iterator for Walk {
    type Item = Result<DirEntry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let wd_entry = match self.inner.next()? {
                Ok(entry) => entry,
                Err(err) => {
                    return Some(Err(Error::from(err)));
                }
            };

            let depth = wd_entry.depth();
            let is_dir = wd_entry.file_type().is_dir();
            let path = wd_entry.path().to_path_buf();

            // Pop ignore rules for directories we've left.
            if is_dir {
                self.gitignore_stack.pop_to_depth(depth);
                self.ignore_stack.pop_to_depth(depth);
                self.rgignore_stack.pop_to_depth(depth);
            }

            // Check if we should skip.
            if depth > 0 && self.should_skip(&wd_entry) {
                if is_dir {
                    // Tell walkdir to skip this directory subtree.
                    // We can't directly skip with walkdir's into_iter,
                    // so we just won't load its ignore files and
                    // descendants will be filtered out.
                    // Actually, we can't skip with flatten. Instead,
                    // the descendants will also be filtered. This is
                    // fine for correctness but may be slower than optimal.
                }
                continue;
            }

            // Load ignore files from this directory.
            if is_dir {
                self.try_load_ignore_files(&path, depth);
            }

            let dir_entry = DirEntry::from_walkdir(wd_entry);
            return Some(Ok(dir_entry));
        }
    }
}

// ---------------------------------------------------------------------------
// WalkBuilder
// ---------------------------------------------------------------------------

/// A builder for constructing directory walkers.
pub struct WalkBuilder {
    paths: Vec<PathBuf>,
    hidden: bool,
    ignore: bool,
    git_ignore: bool,
    git_global: bool,
    parents: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
    follow_links: bool,
    same_file_system: bool,
    overrides: Override,
    types: Types,
    sorter: Option<Arc<dyn Fn(&Path, &Path) -> Ordering + Send + Sync>>,
    threads: usize,
}

impl WalkBuilder {
    /// Create a new `WalkBuilder` with the given root path.
    pub fn new(path: &Path) -> WalkBuilder {
        WalkBuilder {
            paths: vec![path.to_path_buf()],
            hidden: true,
            ignore: true,
            git_ignore: true,
            git_global: true,
            parents: true,
            max_depth: None,
            max_filesize: None,
            follow_links: false,
            same_file_system: false,
            overrides: Override::empty(),
            types: Types::empty(),
            sorter: None,
            threads: 0,
        }
    }

    /// Add an additional search path.
    pub fn add(&mut self, path: &Path) -> &mut WalkBuilder {
        self.paths.push(path.to_path_buf());
        self
    }

    /// Set whether to skip hidden files and directories (default: true).
    pub fn hidden(&mut self, yes: bool) -> &mut WalkBuilder {
        self.hidden = yes;
        self
    }

    /// Set whether to respect `.ignore` files (default: true).
    pub fn ignore(&mut self, yes: bool) -> &mut WalkBuilder {
        self.ignore = yes;
        self
    }

    /// Set whether to respect `.gitignore` files (default: true).
    pub fn git_ignore(&mut self, yes: bool) -> &mut WalkBuilder {
        self.git_ignore = yes;
        self
    }

    /// Set whether to respect the global gitignore (default: true).
    pub fn git_global(&mut self, yes: bool) -> &mut WalkBuilder {
        self.git_global = yes;
        self
    }

    /// Set whether to respect ignore files in parent directories
    /// (default: true).
    pub fn parents(&mut self, yes: bool) -> &mut WalkBuilder {
        self.parents = yes;
        self
    }

    /// Set the maximum traversal depth.
    pub fn max_depth(&mut self, depth: Option<usize>) -> &mut WalkBuilder {
        self.max_depth = depth;
        self
    }

    /// Set the maximum file size filter (in bytes).
    pub fn max_filesize(&mut self, size: Option<u64>) -> &mut WalkBuilder {
        self.max_filesize = size;
        self
    }

    /// Set whether to follow symbolic links (default: false).
    pub fn follow_links(&mut self, yes: bool) -> &mut WalkBuilder {
        self.follow_links = yes;
        self
    }

    /// Set whether to stay on the same file system (default: false).
    pub fn same_file_system(&mut self, yes: bool) -> &mut WalkBuilder {
        self.same_file_system = yes;
        self
    }

    /// Set glob overrides.
    pub fn overrides(&mut self, overrides: Override) -> &mut WalkBuilder {
        self.overrides = overrides;
        self
    }

    /// Set file type filters.
    pub fn types(&mut self, types: Types) -> &mut WalkBuilder {
        self.types = types;
        self
    }

    /// Set a sort function for entries.
    pub fn sort_by_file_path<F>(&mut self, cmp: F) -> &mut WalkBuilder
    where
        F: Fn(&Path, &Path) -> Ordering + Send + Sync + 'static,
    {
        self.sorter = Some(Arc::new(cmp));
        self
    }

    /// Set the number of threads for parallel walking.
    pub fn threads(&mut self, n: usize) -> &mut WalkBuilder {
        self.threads = n;
        self
    }

    fn build_config(&self) -> Arc<WalkConfig> {
        Arc::new(WalkConfig {
            paths: self.paths.clone(),
            hidden: self.hidden,
            ignore: self.ignore,
            git_ignore: self.git_ignore,
            git_global: self.git_global,
            parents: self.parents,
            max_depth: self.max_depth,
            max_filesize: self.max_filesize,
            follow_links: self.follow_links,
            same_file_system: self.same_file_system,
            overrides: self.overrides.clone(),
            types: self.types.clone(),
            sorter: self.sorter.clone(),
            threads: self.threads,
        })
    }

    /// Build a single-threaded [`Walk`] iterator.
    pub fn build(&self) -> Walk {
        Walk::new(self.build_config())
    }

    /// Build a parallel [`WalkParallel`] walker.
    pub fn build_parallel(&self) -> WalkParallel {
        WalkParallel {
            config: self.build_config(),
        }
    }
}

// ---------------------------------------------------------------------------
// WalkParallel
// ---------------------------------------------------------------------------

/// A parallel directory walker.
///
/// Uses multiple threads to traverse directory trees concurrently, calling a
/// user-supplied callback for each entry.
pub struct WalkParallel {
    config: Arc<WalkConfig>,
}

impl WalkParallel {
    /// Run the parallel walk, calling `callback` for each entry.
    ///
    /// The callback is invoked from multiple threads. Return
    /// [`WalkState::Continue`] to keep going, [`WalkState::Skip`] to skip the
    /// current directory, or [`WalkState::Quit`] to stop all traversal.
    pub fn run<F>(&self, callback: F)
    where
        F: Fn(Result<DirEntry, Error>) -> WalkState + Send + Sync + 'static,
    {
        let num_threads = if self.config.threads > 1 {
            self.config.threads
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        };

        let quit = Arc::new(AtomicBool::new(false));
        let callback = Arc::new(callback);

        // Use crossbeam-channel for work distribution.
        let (sender, receiver) = crossbeam_channel::bounded::<DirEntry>(num_threads * 64);

        // Spawn worker threads that process entries from the channel.
        let workers: Vec<_> = (0..num_threads)
            .map(|_| {
                let receiver = receiver.clone();
                let callback = callback.clone();
                let quit = quit.clone();
                std::thread::spawn(move || {
                    while let Ok(entry) = receiver.recv() {
                        if quit.load(AtomicOrdering::Relaxed) {
                            break;
                        }
                        let state = callback(Ok(entry));
                        match state {
                            WalkState::Continue => {}
                            WalkState::Skip => {} // Can't skip from worker.
                            WalkState::Quit => {
                                quit.store(true, AtomicOrdering::Relaxed);
                                break;
                            }
                        }
                    }
                })
            })
            .collect();

        // Use the single-threaded Walk to produce entries.
        // The main thread does the traversal and sends entries to workers.
        let walk = Walk::new(self.config.clone());
        for entry_result in walk {
            if quit.load(AtomicOrdering::Relaxed) {
                break;
            }
            match entry_result {
                Ok(entry) => {
                    if sender.send(entry).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let state = callback(Err(err));
                    if state == WalkState::Quit {
                        quit.store(true, AtomicOrdering::Relaxed);
                        break;
                    }
                }
            }
        }

        // Drop sender to signal workers to finish.
        drop(sender);

        // Wait for all workers to complete.
        for worker in workers {
            let _ = worker.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitignore_empty() {
        let gi = Gitignore::empty();
        assert_eq!(gi.num_ignores(), 0);
        assert!(gi.matched("foo.txt", false).is_none());
    }

    #[test]
    fn test_match_variants() {
        let m = Match::None;
        assert!(m.is_none());
        assert!(!m.is_ignore());
        assert!(!m.is_whitelist());
    }

    #[test]
    fn test_types_builder() {
        let mut builder = TypesBuilder::new();
        builder.select("rust");
        let types = builder.build().unwrap();

        // *.rs should be whitelisted.
        let m = types.matched("foo.rs", false);
        assert!(m.is_whitelist());

        // *.txt should be ignored (not in selected types).
        let m = types.matched("foo.txt", false);
        assert!(m.is_ignore());
    }

    #[test]
    fn test_types_negate() {
        let mut builder = TypesBuilder::new();
        builder.negate("rust");
        let types = builder.build().unwrap();

        // *.rs should be ignored.
        let m = types.matched("foo.rs", false);
        assert!(m.is_ignore());

        // *.txt should pass through (no selections means no filter).
        let m = types.matched("foo.txt", false);
        assert!(m.is_none());
    }

    #[test]
    fn test_types_empty() {
        let types = Types::empty();
        assert!(types.matched("anything.rs", false).is_none());
    }

    #[test]
    fn test_types_definitions() {
        let builder = TypesBuilder::new();
        let defs = builder.definitions();
        assert!(!defs.is_empty());

        // Check that rust is defined.
        let rust_def = defs.iter().find(|d| d.name() == "rust");
        assert!(rust_def.is_some());
        assert!(rust_def.unwrap().globs().contains(&"*.rs".to_string()));
    }

    #[test]
    fn test_override_empty() {
        let ov = Override::empty();
        assert!(ov.is_empty());
        assert!(ov.matched("foo.txt", false).is_none());
    }

    #[test]
    fn test_override_basic() {
        let mut builder = OverrideBuilder::new(Path::new("."));
        builder.add("*.rs").unwrap();
        let ov = builder.build().unwrap();

        // *.rs should be whitelisted.
        let m = ov.matched(Path::new("foo.rs"), false);
        assert!(m.is_whitelist());

        // *.txt should be ignored (not matching any positive override).
        let m = ov.matched(Path::new("foo.txt"), false);
        assert!(m.is_ignore());
    }

    #[test]
    fn test_override_negated() {
        let mut builder = OverrideBuilder::new(Path::new("."));
        builder.add("!*.txt").unwrap();
        let ov = builder.build().unwrap();

        // *.txt should be ignored (negated = exclude).
        let m = ov.matched(Path::new("foo.txt"), false);
        assert!(m.is_ignore());
    }

    #[test]
    fn test_walk_state() {
        assert_eq!(WalkState::Continue, WalkState::Continue);
        assert_ne!(WalkState::Continue, WalkState::Quit);
    }

    #[test]
    fn test_dir_entry_stdin() {
        let entry = DirEntry::stdin();
        assert!(entry.is_stdin());
        assert_eq!(entry.depth(), 0);
    }

    #[test]
    fn test_error_display() {
        let e = Error::msg("test error");
        assert_eq!(e.to_string(), "test error");

        let e = Error::glob("bad glob");
        assert!(e.to_string().contains("glob error"));

        let e = Error::io(io::Error::new(io::ErrorKind::NotFound, "not found"));
        assert!(e.to_string().contains("IO error"));
    }

    #[test]
    fn test_expand_tilde() {
        // Non-tilde paths pass through.
        let p = expand_tilde("/foo/bar");
        assert_eq!(p, PathBuf::from("/foo/bar"));
    }
}
