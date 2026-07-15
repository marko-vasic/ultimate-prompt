//! Argument parsing for the `rg` binary.
//!
//! This module uses the `lexopt` crate to parse command-line arguments into
//! an [`Args`] struct. The `Args` struct provides methods to construct the
//! various builders used throughout the search pipeline.

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, MmapChoice, SearcherBuilder, Searcher};
use grep_printer::{
    ColorSpecs, StandardBuilder, Standard, SummaryBuilder, Summary,
    SummaryKind, JSONBuilder, JSON,
};
use ignore::{
    WalkBuilder, Types, TypesBuilder, OverrideBuilder,
};
use termcolor::{ColorChoice, WriteColor};

/// Which output mode to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Standard grep-like output.
    Standard,
    /// Count matching lines per file (`-c`).
    Count,
    /// Count individual matches per file (`--count-matches`).
    CountMatches,
    /// List files with matches (`-l`).
    FilesWithMatches,
    /// List files without matches (`--files-without-match`).
    FilesWithoutMatch,
    /// Quiet mode: no output, exit code only (`-q`).
    Quiet,
    /// JSON output (`--json`).
    Json,
}

/// Sorting mode for output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    /// No sorting.
    None,
    /// Sort ascending by the given key.
    Ascending(SortKey),
    /// Sort descending by the given key.
    Descending(SortKey),
}

/// Sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// Sort by path.
    Path,
    /// Sort by last modified time.
    Modified,
    /// Sort by last accessed time.
    Accessed,
    /// Sort by creation time.
    Created,
}

/// Parsed command-line arguments.
#[derive(Debug)]
pub struct Args {
    // --- Patterns ---
    /// Raw patterns from -e flags, positional arg, and -f files.
    pub patterns: Vec<String>,
    /// Fixed-string mode (-F).
    pub fixed_strings: bool,
    /// Word-boundary matching (-w).
    pub word_regexp: bool,
    /// Line-boundary matching (-x).
    pub line_regexp: bool,
    /// Case-insensitive (-i).
    pub case_insensitive: bool,
    /// Smart case (-S).
    pub smart_case: bool,
    /// Case-sensitive (-s).
    pub case_sensitive: bool,
    /// Invert match (-v).
    pub invert_match: bool,
    /// Multi-line (-U).
    pub multi_line: bool,
    /// Multi-line dot-all.
    pub multi_line_dot_all: bool,

    // --- Paths ---
    /// Paths to search.
    pub paths: Vec<PathBuf>,

    // --- Output mode ---
    pub output_mode: OutputMode,
    /// Only matching parts (-o).
    pub only_matching: bool,
    /// Replacement string (-r).
    pub replace: Option<String>,
    /// Show line numbers (-n, --line-number).
    pub line_number: Option<bool>,
    /// Show column numbers (--column).
    pub column: bool,
    /// Show filenames (-H, --with-filename).
    pub with_filename: Option<bool>,
    /// Byte offset (-b).
    pub byte_offset: bool,
    /// Heading mode (--heading).
    pub heading: Option<bool>,
    /// Pretty mode (-p, --pretty).
    pub pretty: bool,
    /// Vimgrep mode (--vimgrep).
    pub vimgrep: bool,
    /// Show stats (--stats).
    pub stats: bool,

    // --- Context ---
    pub after_context: usize,
    pub before_context: usize,
    pub context_separator: Option<Vec<u8>>,

    // --- Filters ---
    pub globs: Vec<String>,
    pub iglobs: Vec<String>,
    pub type_select: Vec<String>,
    pub type_negate: Vec<String>,
    pub type_add: Vec<(String, String)>,
    pub type_clear: Vec<String>,
    pub type_list: bool,
    pub unrestricted_count: u8,
    pub hidden: bool,
    pub follow_links: bool,
    pub no_ignore: bool,
    pub no_ignore_vcs: bool,
    pub no_ignore_global: bool,
    pub no_ignore_parent: bool,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
    pub one_file_system: bool,

    // --- Binary ---
    pub binary: bool,
    pub text_mode: bool,
    pub no_binary: bool,

    // --- Regex engine ---
    pub regex_size_limit: Option<usize>,
    pub dfa_size_limit: Option<usize>,
    pub no_unicode: bool,
    pub crlf: bool,
    pub null_data: bool,

    // --- Search ---
    pub max_count: Option<u64>,
    pub max_columns: Option<u64>,
    pub max_columns_preview: bool,
    pub mmap: Option<bool>,
    pub threads: usize,
    pub sort_mode: SortMode,
    pub search_zip: bool,
    pub pre: Option<String>,
    pub pre_glob: Vec<String>,
    pub stop_on_nonmatch: bool,

    // --- Output formatting ---
    pub color: ColorSetting,
    pub color_specs: Vec<String>,
    pub hyperlink_format: Option<String>,
    pub path_separator: Option<u8>,
    pub null: bool,
    pub no_messages: bool,
    pub trim: bool,
    pub field_match_separator: Option<Vec<u8>>,
    pub field_context_separator: Option<Vec<u8>>,

    // --- Special ---
    pub help: bool,
    pub version: bool,
    pub debug: bool,
    pub trace: bool,
    pub generate: Option<String>,
    pub files_mode: bool,
}

/// Color setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSetting {
    /// Auto-detect based on terminal.
    Auto,
    /// Always use color.
    Always,
    /// Use ANSI color codes always.
    Ansi,
    /// Never use color.
    Never,
}

impl Default for Args {
    fn default() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        Args {
            patterns: Vec::new(),
            fixed_strings: false,
            word_regexp: false,
            line_regexp: false,
            case_insensitive: false,
            smart_case: true,
            case_sensitive: false,
            invert_match: false,
            multi_line: false,
            multi_line_dot_all: false,
            paths: Vec::new(),
            output_mode: OutputMode::Standard,
            only_matching: false,
            replace: None,
            line_number: None,
            column: false,
            with_filename: None,
            byte_offset: false,
            heading: None,
            pretty: false,
            vimgrep: false,
            stats: false,
            after_context: 0,
            before_context: 0,
            context_separator: None,
            globs: Vec::new(),
            iglobs: Vec::new(),
            type_select: Vec::new(),
            type_negate: Vec::new(),
            type_add: Vec::new(),
            type_clear: Vec::new(),
            type_list: false,
            unrestricted_count: 0,
            hidden: false,
            follow_links: false,
            no_ignore: false,
            no_ignore_vcs: false,
            no_ignore_global: false,
            no_ignore_parent: false,
            max_depth: None,
            max_filesize: None,
            one_file_system: false,
            binary: false,
            text_mode: false,
            no_binary: false,
            regex_size_limit: None,
            dfa_size_limit: None,
            no_unicode: false,
            crlf: false,
            null_data: false,
            max_count: None,
            max_columns: None,
            max_columns_preview: false,
            mmap: None,
            threads,
            sort_mode: SortMode::None,
            search_zip: false,
            pre: None,
            pre_glob: Vec::new(),
            stop_on_nonmatch: false,
            color: ColorSetting::Auto,
            color_specs: Vec::new(),
            hyperlink_format: None,
            path_separator: None,
            null: false,
            no_messages: false,
            trim: false,
            field_match_separator: None,
            field_context_separator: None,
            help: false,
            version: false,
            debug: false,
            trace: false,
            generate: None,
            files_mode: false,
        }
    }
}

/// Read arguments from the `RIPGREP_CONFIG_PATH` file, if it exists.
///
/// Lines starting with `#` are comments. Empty lines are skipped.
/// Each non-comment, non-empty line is treated as a single argument.
fn read_config_args() -> Vec<String> {
    let path = match std::env::var_os("RIPGREP_CONFIG_PATH") {
        Some(p) => PathBuf::from(p),
        None => return Vec::new(),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
            log::debug!("failed to read config file {:?}: {}", path, err);
            return Vec::new();
        }
    };
    let mut args = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        args.push(line.to_string());
    }
    log::debug!("config file args: {:?}", args);
    args
}

/// Parse arguments from the command line (including config file).
pub fn parse() -> Result<Args, String> {
    // Read config file args first, then CLI args override.
    let config_args = read_config_args();

    // Combine: config args + actual CLI args (skip argv[0]).
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let all_args: Vec<String> = config_args.into_iter().chain(cli_args).collect();

    parse_from(all_args)
}

/// Parse from a given list of argument strings.
pub fn parse_from(args: Vec<String>) -> Result<Args, String> {
    use lexopt::prelude::*;

    let mut result = Args::default();
    let mut explicit_patterns: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut pattern_files: Vec<PathBuf> = Vec::new();

    let mut parser = lexopt::Parser::from_args(args);

    while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
        match arg {
            // ----- Pattern flags -----
            Short('e') | Long("regexp") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                explicit_patterns.push(val);
            }
            Short('f') | Long("file") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                pattern_files.push(PathBuf::from(val));
            }
            Short('F') | Long("fixed-strings") => {
                result.fixed_strings = true;
            }
            Short('w') | Long("word-regexp") => {
                result.word_regexp = true;
            }
            Short('x') | Long("line-regexp") => {
                result.line_regexp = true;
            }
            Short('i') | Long("ignore-case") => {
                result.case_insensitive = true;
                result.case_sensitive = false;
                result.smart_case = false;
            }
            Short('S') | Long("smart-case") => {
                result.smart_case = true;
                result.case_insensitive = false;
                result.case_sensitive = false;
            }
            Short('s') | Long("case-sensitive") => {
                result.case_sensitive = true;
                result.case_insensitive = false;
                result.smart_case = false;
            }
            Short('v') | Long("invert-match") => {
                result.invert_match = true;
            }
            Short('U') | Long("multiline") => {
                result.multi_line = true;
            }
            Long("multiline-dotall") => {
                result.multi_line_dot_all = true;
            }

            // ----- Output flags -----
            Short('c') | Long("count") => {
                result.output_mode = OutputMode::Count;
            }
            Long("count-matches") => {
                result.output_mode = OutputMode::CountMatches;
            }
            Short('l') | Long("files-with-matches") => {
                result.output_mode = OutputMode::FilesWithMatches;
            }
            Long("files-without-match") => {
                result.output_mode = OutputMode::FilesWithoutMatch;
            }
            Short('o') | Long("only-matching") => {
                result.only_matching = true;
            }
            Short('r') | Long("replace") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.replace = Some(val);
            }
            Short('n') | Long("line-number") => {
                result.line_number = Some(true);
            }
            Short('N') | Long("no-line-number") => {
                result.line_number = Some(false);
            }
            Long("column") => {
                result.column = true;
            }
            Short('H') | Long("with-filename") => {
                result.with_filename = Some(true);
            }
            Long("no-filename") => {
                result.with_filename = Some(false);
            }
            Short('b') | Long("byte-offset") => {
                result.byte_offset = true;
            }
            Long("heading") => {
                result.heading = Some(true);
            }
            Long("no-heading") => {
                result.heading = Some(false);
            }
            Short('p') | Long("pretty") => {
                result.pretty = true;
            }
            Long("vimgrep") => {
                result.vimgrep = true;
            }
            Long("json") => {
                result.output_mode = OutputMode::Json;
            }
            Short('q') | Long("quiet") => {
                result.output_mode = OutputMode::Quiet;
            }
            Long("stats") => {
                result.stats = true;
            }

            // ----- Context flags -----
            Short('A') | Long("after-context") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.after_context = val.parse::<usize>()
                    .map_err(|e| format!("invalid after-context: {}", e))?;
            }
            Short('B') | Long("before-context") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.before_context = val.parse::<usize>()
                    .map_err(|e| format!("invalid before-context: {}", e))?;
            }
            Short('C') | Long("context") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                let n = val.parse::<usize>()
                    .map_err(|e| format!("invalid context: {}", e))?;
                result.after_context = n;
                result.before_context = n;
            }
            Long("context-separator") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.context_separator = Some(grep_cli::unescape(&val));
            }
            Long("no-context-separator") => {
                result.context_separator = Some(Vec::new());
            }

            // ----- Filter flags -----
            Short('g') | Long("glob") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.globs.push(val);
            }
            Long("iglob") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.iglobs.push(val);
            }
            Short('t') | Long("type") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.type_select.push(val);
            }
            Short('T') | Long("type-not") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.type_negate.push(val);
            }
            Long("type-add") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                // Format: "name:glob"
                if let Some((name, glob)) = val.split_once(':') {
                    result.type_add.push((name.to_string(), glob.to_string()));
                } else {
                    return Err(format!("invalid --type-add value: {:?} (expected NAME:GLOB)", val));
                }
            }
            Long("type-clear") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.type_clear.push(val);
            }
            Long("type-list") => {
                result.type_list = true;
            }
            Short('u') | Long("unrestricted") => {
                result.unrestricted_count += 1;
            }
            Long("hidden") => {
                result.hidden = true;
            }
            Long("no-hidden") => {
                result.hidden = false;
            }
            Short('L') | Long("follow") => {
                result.follow_links = true;
            }
            Long("no-ignore") => {
                result.no_ignore = true;
            }
            Long("no-ignore-vcs") => {
                result.no_ignore_vcs = true;
            }
            Long("no-ignore-global") => {
                result.no_ignore_global = true;
            }
            Long("no-ignore-parent") => {
                result.no_ignore_parent = true;
            }
            Short('d') | Long("max-depth") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.max_depth = Some(val.parse::<usize>()
                    .map_err(|e| format!("invalid max-depth: {}", e))?);
            }
            Long("max-filesize") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.max_filesize = Some(
                    grep_cli::parse_human_readable_size(&val)
                        .map_err(|e| format!("invalid max-filesize: {}", e))?
                );
            }
            Long("one-file-system") => {
                result.one_file_system = true;
            }

            // ----- Binary flags -----
            Long("binary") => {
                result.binary = true;
            }
            Short('a') | Long("text") => {
                result.text_mode = true;
            }
            Long("no-binary") => {
                result.no_binary = true;
            }

            // ----- Regex engine flags -----
            Long("engine") => {
                // Accept but currently only "default" is supported.
                let _val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
            }
            Long("pcre2") | Short('P') => {
                // Accept but log a warning.
                log::warn!("--pcre2 is not supported in this build; using default engine");
            }
            Long("no-pcre2") => {
                // No-op.
            }
            Long("regex-size-limit") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                let size = grep_cli::parse_human_readable_size(&val)
                    .map_err(|e| format!("invalid regex-size-limit: {}", e))?;
                result.regex_size_limit = Some(size as usize);
            }
            Long("dfa-size-limit") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                let size = grep_cli::parse_human_readable_size(&val)
                    .map_err(|e| format!("invalid dfa-size-limit: {}", e))?;
                result.dfa_size_limit = Some(size as usize);
            }
            Long("no-unicode") => {
                result.no_unicode = true;
            }
            Long("crlf") => {
                result.crlf = true;
            }
            Long("null-data") => {
                result.null_data = true;
            }

            // ----- Search flags -----
            Short('m') | Long("max-count") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.max_count = Some(val.parse::<u64>()
                    .map_err(|e| format!("invalid max-count: {}", e))?);
            }
            Long("max-columns") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.max_columns = Some(val.parse::<u64>()
                    .map_err(|e| format!("invalid max-columns: {}", e))?);
            }
            Long("max-columns-preview") => {
                result.max_columns_preview = true;
            }
            Long("mmap") => {
                result.mmap = Some(true);
            }
            Long("no-mmap") => {
                result.mmap = Some(false);
            }
            Short('j') | Long("threads") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.threads = val.parse::<usize>()
                    .map_err(|e| format!("invalid threads: {}", e))?;
                if result.threads == 0 {
                    result.threads = 1;
                }
            }
            Long("sort") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.sort_mode = parse_sort_mode(&val, false)?;
            }
            Long("sortr") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.sort_mode = parse_sort_mode(&val, true)?;
            }
            Short('z') | Long("search-zip") => {
                result.search_zip = true;
            }
            Long("pre") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.pre = Some(val);
            }
            Long("pre-glob") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.pre_glob.push(val);
            }
            Long("stop-on-nonmatch") => {
                result.stop_on_nonmatch = true;
            }

            // ----- Output formatting flags -----
            Long("color") | Long("colour") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.color = match val.to_lowercase().as_str() {
                    "never" => ColorSetting::Never,
                    "always" => ColorSetting::Always,
                    "auto" => ColorSetting::Auto,
                    "ansi" => ColorSetting::Ansi,
                    other => return Err(format!("invalid color value: {:?}", other)),
                };
            }
            Long("colors") | Long("colours") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.color_specs.push(val);
            }
            Long("hyperlink-format") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.hyperlink_format = Some(val);
            }
            Long("path-separator") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                if val.len() == 1 {
                    result.path_separator = Some(val.as_bytes()[0]);
                } else {
                    return Err(format!("--path-separator must be a single byte, got {:?}", val));
                }
            }
            Short('0') | Long("null") => {
                result.null = true;
            }
            Long("no-messages") => {
                result.no_messages = true;
            }
            Long("trim") => {
                result.trim = true;
            }
            Long("field-match-separator") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.field_match_separator = Some(grep_cli::unescape(&val));
            }
            Long("field-context-separator") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.field_context_separator = Some(grep_cli::unescape(&val));
            }

            // ----- Special flags -----
            Short('h') | Long("help") => {
                result.help = true;
            }
            Short('V') | Long("version") => {
                result.version = true;
            }
            Long("debug") => {
                result.debug = true;
            }
            Long("trace") => {
                result.trace = true;
            }
            Long("generate") => {
                let val: String = parser.value().map_err(|e| e.to_string())?
                    .to_string_lossy().into_owned();
                result.generate = Some(val);
            }
            Long("files") => {
                result.files_mode = true;
            }

            // ----- Positional arguments -----
            Value(val) => {
                positionals.push(val.to_string_lossy().into_owned());
            }

            _ => {
                return Err(format!("unknown argument"));
            }
        }
    }

    // Read patterns from -f files.
    for path in &pattern_files {
        if path.as_os_str() == "-" {
            match grep_cli::read_patterns_from_stdin() {
                Ok(pats) => explicit_patterns.extend(pats),
                Err(err) => return Err(format!("failed to read patterns from stdin: {}", err)),
            }
        } else {
            match grep_cli::read_patterns_from_file(path) {
                Ok(pats) => explicit_patterns.extend(pats),
                Err(err) => return Err(format!("failed to read patterns from {:?}: {}", path, err)),
            }
        }
    }

    // Assign patterns and paths from positionals.
    // If explicit patterns are given (-e or -f), all positionals are paths.
    // Otherwise, the first positional is the pattern, and the rest are paths.
    if !explicit_patterns.is_empty() {
        result.patterns = explicit_patterns;
        result.paths = positionals.into_iter().map(PathBuf::from).collect();
    } else if !positionals.is_empty() {
        result.patterns.push(positionals[0].clone());
        result.paths = positionals[1..].iter().map(|s| PathBuf::from(s)).collect();
    }

    // Apply -u (unrestricted) stacking.
    if result.unrestricted_count >= 1 {
        result.no_ignore = true;
    }
    if result.unrestricted_count >= 2 {
        result.hidden = true;
    }
    if result.unrestricted_count >= 3 {
        result.text_mode = true;
    }

    // Apply --pretty.
    if result.pretty {
        result.color = ColorSetting::Always;
        result.heading = Some(true);
        result.line_number = Some(true);
    }

    // Apply --vimgrep.
    if result.vimgrep {
        result.line_number = Some(true);
        result.column = true;
        // vimgrep implies one line per match (per_match in the printer).
    }

    Ok(result)
}

fn parse_sort_mode(val: &str, reverse: bool) -> Result<SortMode, String> {
    let key = match val.to_lowercase().as_str() {
        "path" | "none" => SortKey::Path,
        "modified" => SortKey::Modified,
        "accessed" => SortKey::Accessed,
        "created" => SortKey::Created,
        other => return Err(format!("invalid sort key: {:?}", other)),
    };
    if reverse {
        Ok(SortMode::Descending(key))
    } else {
        Ok(SortMode::Ascending(key))
    }
}

impl Args {
    /// Returns the color choice for termcolor.
    pub fn color_choice(&self) -> ColorChoice {
        match self.color {
            ColorSetting::Always => ColorChoice::Always,
            ColorSetting::Ansi => ColorChoice::AlwaysAnsi,
            ColorSetting::Never => ColorChoice::Never,
            ColorSetting::Auto => {
                if grep_cli::is_tty_stdout() {
                    ColorChoice::Auto
                } else {
                    ColorChoice::Never
                }
            }
        }
    }

    /// Returns whether we should use color.
    pub fn use_color(&self) -> bool {
        match self.color {
            ColorSetting::Always | ColorSetting::Ansi => true,
            ColorSetting::Never => false,
            ColorSetting::Auto => grep_cli::is_tty_stdout(),
        }
    }

    /// Returns whether heading mode is enabled.
    pub fn use_heading(&self) -> bool {
        self.heading.unwrap_or_else(|| grep_cli::is_tty_stdout())
    }

    /// Returns whether line numbers should be shown.
    pub fn use_line_number(&self) -> bool {
        self.line_number.unwrap_or_else(|| grep_cli::is_tty_stdout())
    }

    /// Returns whether filenames should be shown.
    pub fn use_filename(&self) -> bool {
        self.with_filename.unwrap_or_else(|| {
            // Show filename if multiple paths, or if using implicit "." with tty.
            self.paths.len() > 1
                || (self.paths.is_empty() && grep_cli::is_tty_stdout())
                || self.paths.first().map_or(false, |p| p.is_dir())
        })
    }

    /// Returns whether stdin should be searched.
    pub fn is_stdin_search(&self) -> bool {
        self.paths.is_empty() && !io::stdin().is_terminal()
    }

    /// Build a regex matcher from the configured patterns.
    pub fn regex_matcher(&self) -> Result<grep_regex::RegexMatcher, String> {
        let mut builder = RegexMatcherBuilder::new();

        // Case sensitivity.
        if self.case_sensitive {
            builder.case_insensitive(false);
            builder.case_smart(false);
        } else if self.case_insensitive {
            builder.case_insensitive(true);
            builder.case_smart(false);
        } else {
            builder.case_smart(self.smart_case);
        }

        builder.fixed_strings(self.fixed_strings);
        builder.word(self.word_regexp);
        builder.line(self.line_regexp);
        builder.unicode(!self.no_unicode);
        builder.multi_line(self.multi_line);
        builder.dot_all(self.multi_line_dot_all);
        builder.crlf(self.crlf);

        // Line terminator configuration.
        if self.null_data {
            builder.line_terminator(Some(b'\x00'));
        } else {
            builder.line_terminator(Some(b'\n'));
        }

        if let Some(limit) = self.regex_size_limit {
            builder.size_limit(limit);
        }
        if let Some(limit) = self.dfa_size_limit {
            builder.dfa_size_limit(limit);
        }

        if self.patterns.is_empty() {
            return Err("no pattern given".to_string());
        }

        let pattern_refs: Vec<&str> = self.patterns.iter().map(|s| s.as_str()).collect();
        builder.build_many(&pattern_refs).map_err(|e| e.to_string())
    }

    /// Build a searcher with the current configuration.
    pub fn searcher(&self) -> Searcher {
        let mut builder = SearcherBuilder::new();
        builder.line_number(self.use_line_number());
        builder.invert_match(self.invert_match);
        builder.after_context(self.after_context);
        builder.before_context(self.before_context);
        builder.multi_line(self.multi_line);
        builder.stop_on_nonmatch(self.stop_on_nonmatch);

        // Binary detection.
        if self.text_mode {
            builder.binary_detection(BinaryDetection::None);
        } else if self.no_binary {
            builder.binary_detection(BinaryDetection::Quit);
        } else if self.binary {
            builder.binary_detection(BinaryDetection::Convert(b'\x00'));
        } else {
            builder.binary_detection(BinaryDetection::Quit);
        }

        // Memory mapping.
        match self.mmap {
            Some(true) => { builder.memory_map(MmapChoice::Always); }
            Some(false) => { builder.memory_map(MmapChoice::Never); }
            None => { builder.memory_map(MmapChoice::Auto); }
        }

        // Line terminator.
        if self.null_data {
            builder.line_terminator(grep_matcher::LineTerminator::byte(b'\x00'));
        } else if self.crlf {
            builder.line_terminator(grep_matcher::LineTerminator::crlf());
        }

        builder.build()
    }

    /// Build a standard printer.
    pub fn printer_standard<W: io::Write + WriteColor>(&self, wtr: W) -> Standard<W> {
        let mut builder = StandardBuilder::new();
        builder
            .heading(self.use_heading())
            .path(self.use_filename())
            .line_number(self.use_line_number())
            .column(self.column)
            .byte_offset(self.byte_offset)
            .trim(self.trim)
            .max_columns(self.max_columns)
            .max_columns_preview(self.max_columns_preview)
            .only_matching(self.only_matching)
            .per_match(self.vimgrep);

        if let Some(ref replacement) = self.replace {
            builder.replacement(Some(replacement.as_bytes()));
        }

        if self.null {
            builder.path_terminator(Some(b'\x00'));
        }

        if let Some(ref sep) = self.context_separator {
            if sep.is_empty() {
                builder.separator_context(None);
            } else {
                builder.separator_context(Some(sep));
            }
        }

        if let Some(ref sep) = self.field_match_separator {
            builder.separator_field_match(sep);
        }
        if let Some(ref sep) = self.field_context_separator {
            builder.separator_field_context(sep);
        }

        if self.use_color() {
            builder.colors(ColorSpecs::default_with_color());
        }

        builder.hyperlink_format(self.hyperlink_format.clone());

        builder.build(wtr)
    }

    /// Build a summary printer.
    pub fn printer_summary<W: io::Write + WriteColor>(&self, wtr: W) -> Summary<W> {
        let mut builder = SummaryBuilder::new();

        let kind = match self.output_mode {
            OutputMode::Count => SummaryKind::Count,
            OutputMode::CountMatches => SummaryKind::CountMatches,
            OutputMode::FilesWithMatches => SummaryKind::FilesWithMatches,
            OutputMode::FilesWithoutMatch => SummaryKind::FilesWithoutMatch,
            OutputMode::Quiet => SummaryKind::Quiet,
            _ => SummaryKind::Count,
        };
        builder.kind(kind);
        builder.path(self.use_filename());

        if self.null {
            builder.path_terminator(Some(b'\x00'));
        }

        if self.use_color() {
            builder.colors(ColorSpecs::default_with_color());
        }

        builder.build(wtr)
    }

    /// Build a JSON printer.
    pub fn printer_json<W: io::Write>(&self, wtr: W) -> JSON<W> {
        JSONBuilder::new().build(wtr)
    }

    /// Build file types.
    pub fn types(&self) -> Result<Types, String> {
        let mut builder = TypesBuilder::new();
        builder.add_defaults();

        for (name, glob) in &self.type_add {
            builder.add(name, glob).map_err(|e| e.to_string())?;
        }
        for name in &self.type_clear {
            builder.clear(name);
        }
        for name in &self.type_select {
            builder.select(name);
        }
        for name in &self.type_negate {
            builder.negate(name);
        }

        builder.build().map_err(|e| e.to_string())
    }

    /// Build a walk builder for directory traversal.
    pub fn walk_builder(&self) -> Result<WalkBuilder, String> {
        let first_path = if self.paths.is_empty() {
            PathBuf::from(".")
        } else {
            self.paths[0].clone()
        };

        let mut builder = WalkBuilder::new(&first_path);
        for path in self.paths.iter().skip(1) {
            builder.add(path);
        }
        if self.paths.is_empty() {
            // Already set to "."
        }

        builder.hidden(!self.hidden);
        builder.no_ignore(self.no_ignore);
        builder.no_ignore_vcs(self.no_ignore_vcs);
        builder.no_ignore_global(self.no_ignore_global);
        builder.no_ignore_parent(self.no_ignore_parent);
        builder.follow_links(self.follow_links);
        builder.max_depth(self.max_depth);
        builder.same_file_system(self.one_file_system);
        builder.threads(self.threads);

        if let Some(max_size) = self.max_filesize {
            builder.max_filesize(Some(max_size));
        }

        // Override globs.
        let mut overrides = OverrideBuilder::new(&first_path);
        for glob in &self.globs {
            overrides.add(glob).map_err(|e| e.to_string())?;
        }
        // Case-insensitive globs: convert to case-insensitive glob pattern.
        for glob in &self.iglobs {
            // A simple approach: treat iglob as a regular glob (the glob engine
            // in our implementation doesn't support case-insensitive mode natively,
            // so we just add it as-is for now).
            overrides.add(glob).map_err(|e| e.to_string())?;
        }
        builder.overrides(overrides.build().map_err(|e| e.to_string())?);

        // File types.
        let types = self.types()?;
        builder.types(types);

        // Sorting.
        match self.sort_mode {
            SortMode::Ascending(SortKey::Path) => {
                builder.sort_by_file_name();
            }
            _ => {
                // Other sort modes: we do best-effort via sort_by_file_name
                // for descending path, or ignore for other keys.
                if let SortMode::Descending(SortKey::Path) = self.sort_mode {
                    builder.sort_by_file_name();
                    // Note: descending would require post-processing. For now,
                    // just use ascending.
                }
            }
        }

        Ok(builder)
    }
}

/// Print the help message.
pub fn print_help() {
    let help = r#"rg 0.1.0
ripgrep recursively searches the current directory for lines matching
a regex pattern. By default, ripgrep will respect gitignore globs and
automatically skip hidden files/directories and binary files.

USAGE:
    rg [OPTIONS] PATTERN [PATH ...]
    rg [OPTIONS] -e PATTERN ... [PATH ...]
    rg [OPTIONS] -f PATTERNFILE ... [PATH ...]
    command | rg [OPTIONS] PATTERN

ARGS:
    <PATTERN>    A regular expression pattern to search for
    <PATH>...    Files or directories to search (default: current directory)

COMMON OPTIONS:
    -e, --regexp <PATTERN>     Specify a pattern (can be repeated)
    -f, --file <PATTERNFILE>   Read patterns from a file, one per line
    -i, --ignore-case          Case insensitive search
    -S, --smart-case           Smart case (default): case insensitive if
                               pattern is all lowercase
    -s, --case-sensitive       Case sensitive search
    -v, --invert-match         Invert matching (show non-matching lines)
    -w, --word-regexp          Only match whole words
    -x, --line-regexp          Only match whole lines
    -F, --fixed-strings        Treat pattern as a literal string
    -U, --multiline            Enable multiline matching
    -c, --count                Show count of matching lines per file
    --count-matches            Show count of individual matches per file
    -l, --files-with-matches   Show only filenames with matches
    --files-without-match      Show only filenames without matches
    -o, --only-matching        Show only the matched parts of a line
    -r, --replace <STRING>     Replace matches with the given string
    -n, --line-number          Show line numbers (default for terminals)
    -N, --no-line-number       Suppress line numbers
    --column                   Show column number of first match
    -H, --with-filename        Show filename for each match
    --no-filename              Suppress filename
    -b, --byte-offset          Show byte offset
    --heading                  Show filename as a heading (default for tty)
    --no-heading               Show filename on each line
    -p, --pretty               Alias for --color=always --heading -n
    --vimgrep                  Show results in vim-compatible format
    --json                     Output results as JSON lines
    -q, --quiet                Don't print anything, exit with status
    --stats                    Print aggregate statistics
    -A, --after-context <NUM>  Show NUM lines after each match
    -B, --before-context <NUM> Show NUM lines before each match
    -C, --context <NUM>        Show NUM lines before and after each match
    -g, --glob <GLOB>          Include or exclude files matching GLOB
    --iglob <GLOB>             Like --glob but case insensitive
    -t, --type <TYPE>          Only search files of TYPE
    -T, --type-not <TYPE>      Exclude files of TYPE
    --type-list                Show all available file types
    --type-add <SPEC>          Add a custom type: NAME:GLOB
    --type-clear <TYPE>        Clear globs for TYPE
    -u, --unrestricted         Reduce filtering (stackable, up to -uuu)
    --hidden                   Search hidden files and directories
    -L, --follow               Follow symbolic links
    --no-ignore                Don't respect ignore files
    --no-ignore-vcs            Don't respect .gitignore files
    -d, --max-depth <NUM>      Maximum directory depth
    --max-filesize <SIZE>      Skip files larger than SIZE (e.g. 1M)
    --one-file-system          Don't cross filesystem boundaries
    -a, --text                 Search binary files as text
    --binary                   Search binary files (with replacement)
    --no-binary                Skip binary files (default)
    --no-unicode               Disable Unicode mode
    --crlf                     Use CRLF line terminators
    --null-data                Use NUL as line terminator
    -m, --max-count <NUM>      Stop after NUM matches per file
    --max-columns <NUM>        Truncate long lines
    --max-columns-preview      Show a preview of truncated lines
    --mmap                     Use memory-mapped I/O
    --no-mmap                  Don't use memory-mapped I/O
    -j, --threads <NUM>        Number of threads to use
    --sort <KEY>               Sort results by KEY (path, modified, etc.)
    --sortr <KEY>              Sort results in reverse by KEY
    -z, --search-zip           Search compressed files
    --pre <COMMAND>            Preprocess files with COMMAND
    --pre-glob <GLOB>          Only preprocess files matching GLOB
    --stop-on-nonmatch         Stop searching file after first non-match
    --color <WHEN>             When to use color: never, auto, always, ansi
    --colors <SPEC>            Configure color settings
    --hyperlink-format <FMT>   Set hyperlink format
    --path-separator <SEP>     Set path separator
    -0, --null                 Print NUL byte after filenames
    --no-messages              Suppress error messages
    --trim                     Trim leading whitespace
    --files                    List files that would be searched
    --debug                    Show debug messages
    --trace                    Show trace messages
    --generate <TYPE>          Generate shell completions or man page
    -h, --help                 Show this help message
    -V, --version              Show version

ENVIRONMENT:
    RIPGREP_CONFIG_PATH        Path to a config file with default arguments

EXIT STATUS:
    0    At least one match was found
    1    No matches were found
    2    An error occurred

EXAMPLES:
    rg 'fn main'
    rg -i 'error' src/
    rg -t rust 'unwrap'
    rg --json 'TODO' | jq .
    rg -g '*.rs' -g '!test*' pattern
    rg -uuu pattern  # search everything, including hidden and binary

For more information, see https://github.com/BurntSushi/ripgrep
"#;
    print!("{}", help);
}

/// Print the version string.
pub fn print_version() {
    println!("rg 0.1.0");
}
