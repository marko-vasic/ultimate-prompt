/*!
ripgrep (`rg`) — the main binary entry point.

This module implements the full CLI for ripgrep, including argument parsing,
config file handling, search pipeline orchestration, and output formatting.
*/

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{ArgAction, Parser, ValueEnum};
use grep_cli::{self, ColorSpecs};
use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{
    BinaryDetection, MemoryMap, Searcher, SearcherBuilder, Sink, SinkContext,
    SinkFinish, SinkMatch,
};
use ignore::{
    Override, OverrideBuilder, TypesBuilder, WalkBuilder, WalkState,
};
use termcolor::{BufferedStandardStream, ColorChoice, WriteColor};

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

const EXIT_MATCH: i32 = 0;
const EXIT_NO_MATCH: i32 = 1;
const EXIT_ERROR: i32 = 2;

// ---------------------------------------------------------------------------
// CLI argument definitions
// ---------------------------------------------------------------------------

/// ripgrep — recursively search for a pattern in files.
#[derive(Parser, Debug)]
#[command(
    name = "rg",
    version = "14.1.1",
    about = "ripgrep (rg) recursively searches the current directory for a regex pattern.",
    after_help = "Use -h for short descriptions and --help for more details.",
    disable_help_flag = false,
    disable_version_flag = false,
)]
struct Args {
    /// The regex pattern to search for.
    #[arg(index = 1)]
    pattern: Option<String>,

    /// Paths to search. If not given, searches current directory or stdin.
    #[arg(index = 2, num_args = 0..)]
    paths: Vec<PathBuf>,

    // -- Pattern specification --
    /// Specify one or more patterns. Lines matching any pattern are printed.
    #[arg(short = 'e', long = "regexp", num_args = 1, action = ArgAction::Append)]
    regexp: Vec<String>,

    /// Read patterns from a file, one per line. Use - for stdin.
    #[arg(short = 'f', long = "file", num_args = 1, action = ArgAction::Append)]
    pattern_file: Vec<PathBuf>,

    /// Treat patterns as literal strings, not regexes.
    #[arg(short = 'F', long = "fixed-strings")]
    fixed_strings: bool,

    /// Only match at word boundaries.
    #[arg(short = 'w', long = "word-regexp")]
    word_regexp: bool,

    /// Only match entire lines.
    #[arg(short = 'x', long = "line-regexp")]
    line_regexp: bool,

    /// Case-insensitive search.
    #[arg(short = 'i', long = "ignore-case")]
    ignore_case: bool,

    /// Smart case (default): case-insensitive if pattern is all lowercase.
    #[arg(short = 'S', long = "smart-case")]
    smart_case: bool,

    /// Case-sensitive search (override smart-case).
    #[arg(short = 's', long = "case-sensitive")]
    case_sensitive: bool,

    /// Print non-matching lines.
    #[arg(short = 'v', long = "invert-match")]
    invert_match: bool,

    /// Allow patterns to match across line boundaries.
    #[arg(short = 'U', long = "multiline")]
    multiline: bool,

    /// In multiline mode, make `.` match `\n`.
    #[arg(long = "multiline-dotall")]
    multiline_dotall: bool,

    // -- Output control --
    /// Print only count of matching lines per file.
    #[arg(short = 'c', long = "count")]
    count: bool,

    /// Print count of individual matches (not matching lines).
    #[arg(long = "count-matches")]
    count_matches: bool,

    /// Print only filenames containing matches.
    #[arg(short = 'l', long = "files-with-matches")]
    files_with_matches: bool,

    /// Print only filenames with no matches.
    #[arg(long = "files-without-match")]
    files_without_match: bool,

    /// Print only the matched parts of a line.
    #[arg(short = 'o', long = "only-matching")]
    only_matching: bool,

    /// Replace matches with the given string.
    #[arg(short = 'r', long = "replace")]
    replace: Option<String>,

    /// Show line numbers (default when output is a terminal).
    #[arg(short = 'n', long = "line-number")]
    line_number: bool,

    /// Suppress line numbers.
    #[arg(short = 'N', long = "no-line-number")]
    no_line_number: bool,

    /// Show the 1-based column number of the first match.
    #[arg(long = "column")]
    column: bool,

    /// Show filenames.
    #[arg(short = 'H', long = "with-filename")]
    with_filename: bool,

    /// Suppress filenames.
    #[arg(long = "no-filename")]
    no_filename: bool,

    /// Show the 0-based byte offset of each matching line.
    #[arg(short = 'b', long = "byte-offset")]
    byte_offset: bool,

    /// Group matches by file with filename as header (default for tty).
    #[arg(long = "heading")]
    heading: bool,

    /// Print filename prefix on every match line (default for pipe).
    #[arg(long = "no-heading")]
    no_heading: bool,

    /// Alias for --color=always --heading --line-number.
    #[arg(short = 'p', long = "pretty")]
    pretty: bool,

    /// Print results in vimgrep format.
    #[arg(long = "vimgrep")]
    vimgrep: bool,

    /// List files that would be searched, without searching.
    #[arg(long = "files")]
    list_files: bool,

    /// Print aggregate statistics after search.
    #[arg(long = "stats")]
    stats: bool,

    /// Output results in JSON Lines format.
    #[arg(long = "json")]
    json: bool,

    /// Suppress all output. Exit with 0 if match found, 1 otherwise.
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Print NUL byte after file paths (for xargs -0).
    #[arg(short = '0', long = "null")]
    null: bool,

    // -- Context --
    /// Show NUM lines after each match.
    #[arg(short = 'A', long = "after-context")]
    after_context: Option<usize>,

    /// Show NUM lines before each match.
    #[arg(short = 'B', long = "before-context")]
    before_context: Option<usize>,

    /// Show NUM lines before and after each match.
    #[arg(short = 'C', long = "context")]
    context: Option<usize>,

    /// Separator between context groups (default: --).
    #[arg(long = "context-separator")]
    context_separator: Option<String>,

    /// Disable context separators.
    #[arg(long = "no-context-separator")]
    no_context_separator: bool,

    // -- Filtering --
    /// Include/exclude files matching the glob.
    #[arg(short = 'g', long = "glob", num_args = 1, action = ArgAction::Append)]
    glob: Vec<String>,

    /// Like --glob, but case-insensitive.
    #[arg(long = "iglob", num_args = 1, action = ArgAction::Append)]
    iglob: Vec<String>,

    /// Only search files of the given type.
    #[arg(short = 't', long = "type", num_args = 1, action = ArgAction::Append)]
    type_filter: Vec<String>,

    /// Exclude files of the given type.
    #[arg(short = 'T', long = "type-not", num_args = 1, action = ArgAction::Append)]
    type_not: Vec<String>,

    /// Add a custom file type definition (e.g. "mytype:*.xyz").
    #[arg(long = "type-add", num_args = 1, action = ArgAction::Append)]
    type_add: Vec<String>,

    /// Clear all globs for the given file type.
    #[arg(long = "type-clear", num_args = 1, action = ArgAction::Append)]
    type_clear: Vec<String>,

    /// List all supported file types and exit.
    #[arg(long = "type-list")]
    type_list: bool,

    /// Reduce filtering. -u once disables ignores, -uu also searches hidden, -uuu also binary.
    #[arg(short = 'u', long = "unrestricted", action = ArgAction::Count)]
    unrestricted: u8,

    /// Search hidden files and directories.
    #[arg(long = "hidden")]
    hidden: bool,

    /// Don't search hidden files (default).
    #[arg(long = "no-hidden")]
    no_hidden: bool,

    /// Follow symbolic links during traversal.
    #[arg(short = 'L', long = "follow")]
    follow: bool,

    /// Don't respect ignore files.
    #[arg(long = "no-ignore")]
    no_ignore: bool,

    /// Don't respect VCS ignore files (.gitignore).
    #[arg(long = "no-ignore-vcs")]
    no_ignore_vcs: bool,

    /// Don't respect global gitignore.
    #[arg(long = "no-ignore-global")]
    no_ignore_global: bool,

    /// Don't respect ignore files in parent directories.
    #[arg(long = "no-ignore-parent")]
    no_ignore_parent: bool,

    /// Limit directory traversal depth.
    #[arg(short = 'd', long = "max-depth")]
    max_depth: Option<usize>,

    /// Skip files larger than SIZE (e.g. 1M, 500K).
    #[arg(long = "max-filesize")]
    max_filesize: Option<String>,

    /// Don't cross filesystem boundaries.
    #[arg(long = "one-file-system")]
    one_file_system: bool,

    // -- Binary --
    /// Search binary files (suppress binary output but don't warn).
    #[arg(long = "binary")]
    binary: bool,

    /// Treat binary files as text.
    #[arg(short = 'a', long = "text")]
    text: bool,

    /// Default binary behavior (skip/warn).
    #[arg(long = "no-binary")]
    no_binary: bool,

    // -- Regex config --
    /// Set max compiled regex size.
    #[arg(long = "regex-size-limit")]
    regex_size_limit: Option<String>,

    /// Set max DFA state size.
    #[arg(long = "dfa-size-limit")]
    dfa_size_limit: Option<String>,

    /// Disable Unicode mode.
    #[arg(long = "no-unicode")]
    no_unicode: bool,

    /// Treat CRLF as line terminator.
    #[arg(long = "crlf")]
    crlf: bool,

    /// Use NUL byte as line terminator.
    #[arg(long = "null-data")]
    null_data: bool,

    // -- Search behavior --
    /// Stop searching a file after NUM matching lines.
    #[arg(short = 'm', long = "max-count")]
    max_count: Option<u64>,

    /// Truncate/omit lines longer than NUM bytes.
    #[arg(short = 'M', long = "max-columns")]
    max_columns: Option<u64>,

    /// Show a preview of truncated lines.
    #[arg(long = "max-columns-preview")]
    max_columns_preview: bool,

    /// Force memory-mapped I/O.
    #[arg(long = "mmap")]
    mmap: bool,

    /// Disable memory-mapped I/O.
    #[arg(long = "no-mmap")]
    no_mmap: bool,

    /// Number of threads to use.
    #[arg(short = 'j', long = "threads")]
    threads: Option<usize>,

    /// Sort results ascending by criteria.
    #[arg(long = "sort")]
    sort: Option<SortCriteria>,

    /// Sort results descending by criteria.
    #[arg(long = "sortr")]
    sortr: Option<SortCriteria>,

    /// Search inside compressed files.
    #[arg(short = 'z', long = "search-zip")]
    search_zip: bool,

    /// Run a preprocessor command on each file.
    #[arg(long = "pre")]
    pre: Option<String>,

    /// Only run preprocessor on files matching glob.
    #[arg(long = "pre-glob", num_args = 1, action = ArgAction::Append)]
    pre_glob: Vec<String>,

    /// Stop searching a file after the first non-matching line following a match.
    #[arg(long = "stop-on-nonmatch")]
    stop_on_nonmatch: bool,

    // -- Output formatting --
    /// Control color output: never, always, auto.
    #[arg(long = "color", default_value = "auto")]
    color: ColorWhen,

    /// Configure specific colors.
    #[arg(long = "colors", num_args = 1, action = ArgAction::Append)]
    colors: Vec<String>,

    /// Set path separator character.
    #[arg(long = "path-separator")]
    path_separator: Option<String>,

    /// Suppress error messages.
    #[arg(long = "no-messages")]
    no_messages: bool,

    /// Trim leading ASCII whitespace from each line.
    #[arg(long = "trim")]
    trim: bool,

    /// Separator between fields in match lines (default: :).
    #[arg(long = "field-match-separator")]
    field_match_separator: Option<String>,

    /// Separator between fields in context lines (default: -).
    #[arg(long = "field-context-separator")]
    field_context_separator: Option<String>,

    // -- Generation / Debug --
    /// Generate man page or shell completions.
    #[arg(long = "generate")]
    generate: Option<GenerateMode>,

    /// Enable debug logging.
    #[arg(long = "debug")]
    debug: bool,

    /// Enable trace-level logging (more verbose than --debug).
    #[arg(long = "trace")]
    trace: bool,
}

#[derive(Clone, Debug, ValueEnum)]
enum ColorWhen {
    Never,
    Always,
    Auto,
}

#[derive(Clone, Debug, ValueEnum)]
enum SortCriteria {
    Path,
    Modified,
    Accessed,
    Created,
}

#[derive(Clone, Debug, ValueEnum)]
enum GenerateMode {
    Man,
    CompleteBash,
    CompleteZsh,
    CompleteFish,
    CompletePowershell,
}

// ---------------------------------------------------------------------------
// Resolved configuration
// ---------------------------------------------------------------------------

/// Fully resolved configuration derived from CLI args.
struct ResolvedConfig {
    patterns: Vec<String>,
    paths: Vec<PathBuf>,
    search_stdin: bool,

    // Pattern options
    fixed_strings: bool,
    word_regexp: bool,
    line_regexp: bool,
    case_insensitive: Option<bool>,
    case_smart: bool,
    invert_match: bool,
    multiline: bool,
    multiline_dotall: bool,

    // Output modes
    count: bool,
    count_matches: bool,
    files_with_matches: bool,
    files_without_match: bool,
    only_matching: bool,
    replace: Option<String>,
    show_line_number: bool,
    show_column: bool,
    show_filename: FilenameMode,
    byte_offset: bool,
    heading: bool,
    vimgrep: bool,
    stats: bool,
    json: bool,
    quiet: bool,
    null_sep: bool,
    trim: bool,

    // Context
    before_context: usize,
    after_context: usize,
    context_separator: Vec<u8>,

    // Filtering
    hidden: bool,
    no_ignore: bool,
    no_ignore_vcs: bool,
    no_ignore_global: bool,
    no_ignore_parent: bool,
    follow_links: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
    one_file_system: bool,

    // Binary
    binary_detection: BinaryBehavior,

    // Regex config
    regex_size_limit: Option<usize>,
    dfa_size_limit: Option<usize>,
    no_unicode: bool,
    crlf: bool,
    null_data: bool,

    // Search behavior
    max_count: Option<u64>,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    memory_map: MemoryMap,
    threads: usize,
    sort: Option<(SortCriteria, bool)>, // (criteria, reversed)
    search_zip: bool,
    pre: Option<String>,
    pre_globs: Vec<String>,
    stop_on_nonmatch: bool,

    // Output formatting
    color_choice: ColorChoice,
    color_specs: ColorSpecs,
    path_separator: Option<String>,
    no_messages: bool,
    field_match_separator: String,
    field_context_separator: String,

    // Type filters
    glob_overrides: Override,
    types_filter: ignore::Types,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FilenameMode {
    Always,
    Never,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BinaryBehavior {
    /// Default: quit on NUL, warn if matches found
    Quit,
    /// --text/-a: treat binary as text
    AsText,
    /// --binary: search binary, suppress output but no skip
    SearchBinary,
}

// ---------------------------------------------------------------------------
// Statistics tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SearchStats {
    total_matches: AtomicU64,
    total_matched_lines: AtomicU64,
    files_with_matches: AtomicU64,
    files_searched: AtomicU64,
    bytes_searched: AtomicU64,
    bytes_printed: AtomicU64,
}

// Per-file stats collected in sink
#[derive(Debug, Default, Clone)]
struct FileStats {
    matches: u64,
    matched_lines: u64,
    bytes_searched: u64,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    // Install a custom panic handler that sets exit code 2 on panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        process::exit(EXIT_ERROR);
    }));

    let code = match run() {
        Ok(code) => code,
        Err(err) => {
            // Check for broken pipe
            if is_broken_pipe(&err) {
                EXIT_MATCH
            } else {
                eprintln!("rg: {}", err);
                EXIT_ERROR
            }
        }
    };
    process::exit(code);
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    if let Some(io_err) = err.downcast_ref::<io::Error>() {
        return io_err.kind() == io::ErrorKind::BrokenPipe;
    }
    false
}

fn run() -> anyhow::Result<i32> {
    // Read config file args first.
    let config_args = read_config_file_args();
    let actual_args: Vec<OsString> = std::env::args_os().collect();

    // Merge: [program_name] + config_args + actual_args[1..]
    let mut merged = vec![actual_args[0].clone()];
    merged.extend(config_args);
    if actual_args.len() > 1 {
        merged.extend_from_slice(&actual_args[1..]);
    }

    let args = match Args::try_parse_from(&merged) {
        Ok(a) => a,
        Err(e) => {
            // clap will print help/version and exit with 0 for those,
            // or print error and exit with 2 for parse errors.
            e.exit();
        }
    };

    // Initialize logging.
    if args.trace {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Trace)
            .init();
    } else if args.debug {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    // Handle --generate
    if let Some(ref mode) = args.generate {
        return handle_generate(mode);
    }

    // Handle --type-list
    if args.type_list {
        return handle_type_list(&args);
    }

    // Resolve the full config from args.
    let config = resolve_config(&args)?;

    // Handle --files mode
    if args.list_files {
        return handle_list_files(&config);
    }

    // Need a pattern for search.
    if config.patterns.is_empty() {
        anyhow::bail!(
            "No pattern was given. Use -e to specify patterns, or provide a positional PATTERN."
        );
    }

    // Build the regex matcher.
    let matcher = build_matcher(&config)?;

    // Check for impossible match (optimization).
    if matcher.is_match_impossible() {
        return Ok(EXIT_NO_MATCH);
    }

    // Run the search.
    let exit_code = if config.search_stdin {
        search_stdin(&config, &matcher)?
    } else {
        search_paths(&config, &matcher)?
    };

    Ok(exit_code)
}

// ---------------------------------------------------------------------------
// Config file reading
// ---------------------------------------------------------------------------

fn read_config_file_args() -> Vec<OsString> {
    let path = match std::env::var_os("RIPGREP_CONFIG_PATH") {
        Some(p) => PathBuf::from(p),
        None => return Vec::new(),
    };

    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut args = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        args.push(OsString::from(trimmed));
    }
    args
}

// ---------------------------------------------------------------------------
// Resolve args → config
// ---------------------------------------------------------------------------

fn resolve_config(args: &Args) -> anyhow::Result<ResolvedConfig> {
    // Collect patterns.
    let mut patterns = Vec::new();

    // -e / --regexp patterns
    for p in &args.regexp {
        patterns.push(p.clone());
    }

    // -f / --file patterns
    for f in &args.pattern_file {
        let file_patterns = if f == Path::new("-") {
            grep_cli::patterns_from_stdin()?
        } else {
            grep_cli::patterns_from_path(f)?
        };
        patterns.extend(file_patterns);
    }

    let mut paths = args.paths.clone();

    // Positional PATTERN (only used if no -e or -f was given)
    // If -e or -f was given, any positional argument in args.pattern is actually the first path.
    if patterns.is_empty() {
        if let Some(ref p) = args.pattern {
            patterns.push(p.clone());
        }
    } else if let Some(ref p) = args.pattern {
        paths.insert(0, PathBuf::from(p));
    }

    let search_stdin = if paths.is_empty() {
        // No explicit paths
        if grep_cli::is_readable_stdin() {
            // stdin is a pipe or redirected file → search stdin
            true
        } else {
            // stdin is tty or character device → search current directory
            paths.push(PathBuf::from("."));
            false
        }
    } else if paths.len() == 1 && paths[0] == Path::new("-") {
        true
    } else {
        false
    };

    // Determine if searching a single file.
    let is_single_file = !search_stdin
        && paths.len() == 1
        && paths[0].is_file();

    // Handle -p / --pretty
    let pretty = args.pretty;

    // Resolve color choice.
    let color_choice = if args.json {
        ColorChoice::Never
    } else if pretty {
        ColorChoice::Always
    } else {
        match args.color {
            ColorWhen::Never => ColorChoice::Never,
            ColorWhen::Always => ColorChoice::Always,
            ColorWhen::Auto => {
                if std::io::stdout().is_terminal() {
                    ColorChoice::Auto
                } else {
                    ColorChoice::Never
                }
            }
        }
    };

    // Resolve color specs.
    let color_specs = if args.colors.is_empty() {
        ColorSpecs::default()
    } else {
        grep_cli::parse_color_specs(&args.colors)
            .map_err(|e| anyhow::anyhow!("{}", e))?
    };

    // Line numbers.
    let show_line_number = if args.no_line_number {
        false
    } else if args.line_number || args.column || pretty || args.vimgrep {
        true
    } else {
        // Default: show line numbers when output is a tty.
        std::io::stdout().is_terminal()
    };

    // Heading mode.
    let heading = if args.no_heading || args.vimgrep {
        false
    } else if args.heading || pretty {
        true
    } else {
        std::io::stdout().is_terminal()
    };

    // Filename display.
    let show_filename = if args.no_filename {
        FilenameMode::Never
    } else if args.with_filename || args.vimgrep {
        FilenameMode::Always
    } else {
        FilenameMode::Auto
    };

    // Unrestricted levels.
    let unrestricted = args.unrestricted;

    // Hidden files.
    let hidden = args.hidden || !args.no_hidden && unrestricted >= 2;

    // Ignore handling.
    let no_ignore = args.no_ignore || unrestricted >= 1;
    let no_ignore_vcs = args.no_ignore_vcs || no_ignore;
    let no_ignore_global = args.no_ignore_global || no_ignore;
    let no_ignore_parent = args.no_ignore_parent;

    // Binary behavior.
    let binary_detection = if args.text || unrestricted >= 3 {
        BinaryBehavior::AsText
    } else if args.binary {
        BinaryBehavior::SearchBinary
    } else {
        BinaryBehavior::Quit
    };

    // Case sensitivity.
    let (case_insensitive, case_smart) = if args.case_sensitive {
        (Some(false), false)
    } else if args.ignore_case {
        (Some(true), false)
    } else if args.smart_case {
        (None, true)
    } else {
        // Default: smart-case
        (None, true)
    };

    // Context.
    let before_context = args.context.unwrap_or(args.before_context.unwrap_or(0));
    let after_context = args.context.unwrap_or(args.after_context.unwrap_or(0));

    // Context separator.
    let context_separator = if args.no_context_separator {
        Vec::new()
    } else if let Some(ref sep) = args.context_separator {
        grep_cli::unescape(sep)
    } else {
        b"--".to_vec()
    };

    // Max filesize.
    let max_filesize = if let Some(ref s) = args.max_filesize {
        Some(
            grep_cli::parse_human_readable_size(s)
                .map_err(|e| anyhow::anyhow!("{}", e))?,
        )
    } else {
        None
    };

    // Regex size limits.
    let regex_size_limit = if let Some(ref s) = args.regex_size_limit {
        Some(
            grep_cli::parse_human_readable_size(s)
                .map_err(|e| anyhow::anyhow!("{}", e))? as usize,
        )
    } else {
        None
    };

    let dfa_size_limit = if let Some(ref s) = args.dfa_size_limit {
        Some(
            grep_cli::parse_human_readable_size(s)
                .map_err(|e| anyhow::anyhow!("{}", e))? as usize,
        )
    } else {
        None
    };

    // Memory mapping.
    let memory_map = if args.mmap {
        MemoryMap::Always
    } else if args.no_mmap {
        MemoryMap::Never
    } else {
        MemoryMap::Auto
    };

    // Threads.
    let sort_option = if let Some(ref s) = args.sort {
        Some((s.clone(), false))
    } else if let Some(ref s) = args.sortr {
        Some((s.clone(), true))
    } else {
        None
    };

    // Sorting forces single-threaded.
    let threads = if sort_option.is_some() {
        1
    } else if let Some(t) = args.threads {
        if t == 0 { 1 } else { t }
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    };

    // Build glob overrides.
    let glob_overrides = build_glob_overrides(&args.glob, &args.iglob, &paths)?;

    // Build type filters.
    let types_filter = build_type_filter(
        &args.type_filter,
        &args.type_not,
        &args.type_add,
        &args.type_clear,
    )?;

    // Field separators.
    let field_match_separator = args
        .field_match_separator
        .clone()
        .unwrap_or_else(|| ":".to_string());
    let field_context_separator = args
        .field_context_separator
        .clone()
        .unwrap_or_else(|| "-".to_string());

    Ok(ResolvedConfig {
        patterns,
        paths,
        search_stdin,
        fixed_strings: args.fixed_strings,
        word_regexp: args.word_regexp,
        line_regexp: args.line_regexp,
        case_insensitive,
        case_smart,
        invert_match: args.invert_match,
        multiline: args.multiline,
        multiline_dotall: args.multiline_dotall,
        count: args.count,
        count_matches: args.count_matches,
        files_with_matches: args.files_with_matches,
        files_without_match: args.files_without_match,
        only_matching: args.only_matching,
        replace: args.replace.clone(),
        show_line_number,
        show_column: args.column || args.vimgrep,
        show_filename,
        byte_offset: args.byte_offset,
        heading,
        vimgrep: args.vimgrep,
        stats: args.stats,
        json: args.json,
        quiet: args.quiet,
        null_sep: args.null,
        trim: args.trim,
        before_context,
        after_context,
        context_separator,
        hidden,
        no_ignore,
        no_ignore_vcs,
        no_ignore_global,
        no_ignore_parent,
        follow_links: args.follow,
        max_depth: args.max_depth,
        max_filesize,
        one_file_system: args.one_file_system,
        binary_detection,
        regex_size_limit,
        dfa_size_limit,
        no_unicode: args.no_unicode,
        crlf: args.crlf,
        null_data: args.null_data,
        max_count: args.max_count,
        max_columns: args.max_columns,
        max_columns_preview: args.max_columns_preview,
        memory_map,
        threads,
        sort: sort_option,
        search_zip: args.search_zip,
        pre: args.pre.clone(),
        pre_globs: args.pre_glob.clone(),
        stop_on_nonmatch: args.stop_on_nonmatch,
        color_choice,
        color_specs,
        path_separator: args.path_separator.clone(),
        no_messages: args.no_messages,
        field_match_separator,
        field_context_separator,
        glob_overrides,
        types_filter,
    })
}

// ---------------------------------------------------------------------------
// Build helpers
// ---------------------------------------------------------------------------

fn build_glob_overrides(
    globs: &[String],
    iglobs: &[String],
    paths: &[PathBuf],
) -> anyhow::Result<Override> {
    if globs.is_empty() && iglobs.is_empty() {
        return Ok(Override::empty());
    }
    let root = if paths.is_empty() {
        PathBuf::from(".")
    } else {
        paths[0].clone()
    };
    let mut builder = OverrideBuilder::new(&root);
    for g in globs {
        builder.add(g).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    if !iglobs.is_empty() {
        builder.case_insensitive(true);
        for g in iglobs {
            builder.add(g).map_err(|e| anyhow::anyhow!("{}", e))?;
        }
    }
    Ok(builder.build().map_err(|e| anyhow::anyhow!("{}", e))?)
}

fn build_type_filter(
    selected: &[String],
    negated: &[String],
    added: &[String],
    cleared: &[String],
) -> anyhow::Result<ignore::Types> {
    let mut builder = TypesBuilder::new();

    // Handle --type-add
    for spec in added {
        // Format: "name:glob" or "name:glob1,glob2,..."
        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("invalid --type-add value '{}': expected 'name:glob'", spec);
        }
        let name = parts[0];
        let glob_str = parts[1];
        // Support comma-separated globs.
        for g in glob_str.split(',') {
            let g = g.trim();
            if !g.is_empty() {
                builder.add(name, g).map_err(|e| anyhow::anyhow!("{}", e))?;
            }
        }
    }

    // Handle --type-clear
    for name in cleared {
        builder.clear(name);
    }

    // Handle -t / --type (select)
    for name in selected {
        builder.select(name);
    }

    // Handle -T / --type-not (negate)
    for name in negated {
        builder.negate(name);
    }

    Ok(builder.build().map_err(|e| anyhow::anyhow!("{}", e))?)
}

fn build_matcher(config: &ResolvedConfig) -> anyhow::Result<RegexMatcher> {
    let mut builder = RegexMatcherBuilder::new();

    builder
        .fixed_strings(config.fixed_strings)
        .word(config.word_regexp)
        .line(config.line_regexp)
        .unicode(!config.no_unicode)
        .crlf(config.crlf)
        .multi_line(config.multiline)
        .dot_matches_new_line(config.multiline_dotall);

    if let Some(ci) = config.case_insensitive {
        builder.case_insensitive(ci);
        builder.case_smart(false);
    } else {
        builder.case_smart(config.case_smart);
    }

    if let Some(limit) = config.regex_size_limit {
        builder.size_limit(limit);
    }
    if let Some(limit) = config.dfa_size_limit {
        builder.dfa_size_limit(limit);
    }

    if config.null_data {
        builder.line_terminator(Some(grep_matcher::LineTerminator::Byte(b'\0')));
    }

    let patterns: Vec<String> = config.patterns.clone();
    let matcher = builder
        .build_many(&patterns)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(matcher)
}

fn build_searcher(config: &ResolvedConfig) -> Searcher {
    let mut builder = SearcherBuilder::new();

    builder
        .invert_match(config.invert_match)
        .line_number(config.show_line_number)
        .multi_line(config.multiline)
        .before_context(config.before_context)
        .after_context(config.after_context)
        .stop_on_nonmatch(config.stop_on_nonmatch)
        .memory_map(config.memory_map);

    if let Some(max) = config.max_count {
        builder.max_count(Some(max));
    }

    // Line terminator
    if config.null_data {
        builder.line_terminator(grep_matcher::LineTerminator::Byte(b'\0'));
        // null-data disables binary detection
        builder.binary_detection(BinaryDetection::none());
    } else if config.crlf {
        builder.line_terminator(grep_matcher::LineTerminator::CRLF);
        // Set binary detection based on config
        builder.binary_detection(binary_detection_from_config(config));
    } else {
        builder.binary_detection(binary_detection_from_config(config));
    }

    builder.build()
}

fn binary_detection_from_config(config: &ResolvedConfig) -> BinaryDetection {
    match config.binary_detection {
        BinaryBehavior::AsText => BinaryDetection::none(),
        BinaryBehavior::SearchBinary => BinaryDetection::none(),
        BinaryBehavior::Quit => BinaryDetection::quit(),
    }
}

fn build_walker(config: &ResolvedConfig) -> WalkBuilder {
    let first_path = config.paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let mut builder = WalkBuilder::new(&first_path);

    for path in config.paths.iter().skip(1) {
        builder.add(path);
    }

    builder
        .hidden(!config.hidden)
        .ignore(!config.no_ignore)
        .git_ignore(!config.no_ignore_vcs)
        .git_global(!config.no_ignore_global)
        .parents(!config.no_ignore_parent)
        .max_depth(config.max_depth)
        .max_filesize(config.max_filesize)
        .follow_links(config.follow_links)
        .same_file_system(config.one_file_system)
        .overrides(config.glob_overrides.clone())
        .types(config.types_filter.clone())
        .threads(config.threads);

    // Handle sorting.
    if let Some((ref criteria, reversed)) = config.sort {
        match criteria {
            SortCriteria::Path => {
                if reversed {
                    builder.sort_by_file_path(|a, b| b.cmp(a));
                } else {
                    builder.sort_by_file_path(|a, b| a.cmp(b));
                }
            }
            SortCriteria::Modified => {
                let rev = reversed;
                builder.sort_by_file_path(move |a, b| {
                    let ma = fs::metadata(a).and_then(|m| m.modified()).ok();
                    let mb = fs::metadata(b).and_then(|m| m.modified()).ok();
                    let ord = ma.cmp(&mb);
                    if rev { ord.reverse() } else { ord }
                });
            }
            SortCriteria::Accessed => {
                let rev = reversed;
                builder.sort_by_file_path(move |a, b| {
                    let ma = fs::metadata(a).and_then(|m| m.accessed()).ok();
                    let mb = fs::metadata(b).and_then(|m| m.accessed()).ok();
                    let ord = ma.cmp(&mb);
                    if rev { ord.reverse() } else { ord }
                });
            }
            SortCriteria::Created => {
                let rev = reversed;
                builder.sort_by_file_path(move |a, b| {
                    let ma = fs::metadata(a).and_then(|m| m.created()).ok();
                    let mb = fs::metadata(b).and_then(|m| m.created()).ok();
                    let ord = ma.cmp(&mb);
                    if rev { ord.reverse() } else { ord }
                });
            }
        }
    }

    builder
}

// ---------------------------------------------------------------------------
// Special modes
// ---------------------------------------------------------------------------

fn handle_generate(mode: &GenerateMode) -> anyhow::Result<i32> {
    use clap::CommandFactory;
    let mut cmd = Args::command();

    match mode {
        GenerateMode::Man => {
            let man = clap_mangen::Man::new(cmd);
            let mut buf = Vec::new();
            man.render(&mut buf)?;
            io::stdout().write_all(&buf)?;
        }
        GenerateMode::CompleteBash => {
            clap_complete::generate(
                clap_complete::shells::Bash,
                &mut cmd,
                "rg",
                &mut io::stdout(),
            );
        }
        GenerateMode::CompleteZsh => {
            clap_complete::generate(
                clap_complete::shells::Zsh,
                &mut cmd,
                "rg",
                &mut io::stdout(),
            );
        }
        GenerateMode::CompleteFish => {
            clap_complete::generate(
                clap_complete::shells::Fish,
                &mut cmd,
                "rg",
                &mut io::stdout(),
            );
        }
        GenerateMode::CompletePowershell => {
            clap_complete::generate(
                clap_complete::shells::PowerShell,
                &mut cmd,
                "rg",
                &mut io::stdout(),
            );
        }
    }

    Ok(EXIT_MATCH)
}

fn handle_type_list(args: &Args) -> anyhow::Result<i32> {
    let mut builder = TypesBuilder::new();

    // Apply --type-add
    for spec in &args.type_add {
        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        if parts.len() == 2 {
            for g in parts[1].split(',') {
                let g = g.trim();
                if !g.is_empty() {
                    let _ = builder.add(parts[0], g);
                }
            }
        }
    }

    // Apply --type-clear
    for name in &args.type_clear {
        builder.clear(name);
    }

    let defs = builder.definitions();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for def in &defs {
        write!(out, "{}: ", def.name())?;
        let globs = def.globs();
        for (i, g) in globs.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "{}", g)?;
        }
        writeln!(out)?;
    }
    Ok(EXIT_MATCH)
}

fn handle_list_files(config: &ResolvedConfig) -> anyhow::Result<i32> {
    let walker = build_walker(config);
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let path_terminator = if config.null_sep { "\0" } else { "\n" };

    for entry in walker.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                if !config.no_messages {
                    eprintln!("rg: {}", err);
                }
                continue;
            }
        };

        // Skip directories.
        if entry.file_type().map_or(false, |ft| ft.is_dir()) {
            continue;
        }

        let path = entry.path();
        let display_path = format_path(path, &config.path_separator);
        write!(out, "{}{}", display_path, path_terminator)?;
    }

    Ok(EXIT_MATCH)
}

// ---------------------------------------------------------------------------
// Search: stdin
// ---------------------------------------------------------------------------

fn search_stdin(
    config: &ResolvedConfig,
    matcher: &RegexMatcher,
) -> anyhow::Result<i32> {
    let mut searcher = build_searcher(config);
    let search_start = Instant::now();

    let should_show_filename = match config.show_filename {
        FilenameMode::Always => true,
        FilenameMode::Never => false,
        FilenameMode::Auto => false, // stdin: no filename by default
    };

    let path_label: Option<&str> = if should_show_filename {
        Some("<stdin>")
    } else {
        None
    };

    let stats = Arc::new(SearchStats::default());
    let color_choice = config.color_choice;
    let mut stdout = BufferedStandardStream::stdout(color_choice);

    let mut sink = PrinterSink::new(
        config,
        Some(matcher.clone()),
        path_label.map(|s| s.to_string()),
        Some("<stdin>".to_string()),
        &mut stdout,
        stats.clone(),
    );

    let stdin = io::stdin();
    let stdin_lock = stdin.lock();
    searcher.search_reader(matcher, stdin_lock, &mut sink)?;

    let had_match = sink.had_match;
    let binary_match_path = sink.binary_match_path.clone();
    drop(sink);
    stdout.flush()?;

    if let Some(path) = binary_match_path {
        eprintln!("Binary file {} matches", path);
    }

    if config.stats {
        let elapsed = search_start.elapsed();
        print_stats(&stats, elapsed, &mut stdout)?;
        stdout.flush()?;
    }

    Ok(if had_match { EXIT_MATCH } else { EXIT_NO_MATCH })
}

// ---------------------------------------------------------------------------
// Search: file paths
// ---------------------------------------------------------------------------

fn search_paths(
    config: &ResolvedConfig,
    matcher: &RegexMatcher,
) -> anyhow::Result<i32> {
    let walker = build_walker(config);
    let search_start = Instant::now();
    let stats = Arc::new(SearchStats::default());
    let had_match = Arc::new(AtomicBool::new(false));
    let had_error = Arc::new(AtomicBool::new(false));

    // Determine default filename display for this search.
    let is_single_file = config.paths.len() == 1
        && config.paths[0].is_file();
    let default_show_filename = !is_single_file;

    let should_show_filename = match config.show_filename {
        FilenameMode::Always => true,
        FilenameMode::Never => false,
        FilenameMode::Auto => default_show_filename,
    };

    if config.threads <= 1 || config.sort.is_some() {
        // Single-threaded search.
        let color_choice = config.color_choice;
        let mut stdout = BufferedStandardStream::stdout(color_choice);

        for entry in walker.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    if !config.no_messages {
                        eprintln!("rg: {}", err);
                    }
                    had_error.store(true, AtomicOrdering::Relaxed);
                    continue;
                }
            };

            // Skip directories.
            if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                continue;
            }

            let path = entry.path().to_path_buf();
            let result = search_one_file(
                config,
                matcher,
                &path,
                should_show_filename,
                &stats,
                &mut stdout,
            );

            match result {
                Ok(matched) => {
                    if matched {
                        had_match.store(true, AtomicOrdering::Relaxed);
                        if config.quiet {
                            break;
                        }
                    }
                }
                Err(err) => {
                    if !config.no_messages {
                        eprintln!("rg: {}: {}", path.display(), err);
                    }
                    had_error.store(true, AtomicOrdering::Relaxed);
                }
            }
        }

        if config.stats {
            let elapsed = search_start.elapsed();
            print_stats(&stats, elapsed, &mut stdout)?;
        }
        stdout.flush()?;
    } else {
        let config_arc = Arc::new(config.clone());
        let matcher_arc = Arc::new(matcher.clone());
        let stats_ref = stats.clone();
        let had_match_ref = had_match.clone();
        let had_error_ref = had_error.clone();

        // Use a mutex around stdout for atomic file output.
        let stdout_mutex = Arc::new(Mutex::new(
            BufferedStandardStream::stdout(config.color_choice),
        ));

        let parallel_walker = walker.build_parallel();
        parallel_walker.run(move |entry_result| {
            if config_arc.quiet && had_match_ref.load(AtomicOrdering::Relaxed) {
                return WalkState::Quit;
            }

            let entry = match entry_result {
                Ok(e) => e,
                Err(err) => {
                    if !config_arc.no_messages {
                        eprintln!("rg: {}", err);
                    }
                    had_error_ref.store(true, AtomicOrdering::Relaxed);
                    return WalkState::Continue;
                }
            };

            // Skip directories.
            if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                return WalkState::Continue;
            }

            let path = entry.path().to_path_buf();

            // Buffer output for this file to avoid interleaving.
            let mut buffer = Vec::new();
            let result = search_one_file_to_buffer(
                &config_arc,
                &matcher_arc,
                &path,
                should_show_filename,
                &stats_ref,
                &mut buffer,
            );

            match result {
                Ok((matched, binary_path)) => {
                    if matched {
                        had_match_ref.store(true, AtomicOrdering::Relaxed);
                    }
                    if !buffer.is_empty() {
                        if let Ok(mut stdout) = stdout_mutex.lock() {
                            let _ = stdout.write_all(&buffer);
                            let _ = stdout.flush();
                        }
                    }
                    if let Some(bpath) = binary_path {
                        eprintln!("Binary file {} matches", bpath);
                    }
                    if config_arc.quiet && matched {
                        return WalkState::Quit;
                    }
                }
                Err(err) => {
                    if !config_arc.no_messages {
                        eprintln!("rg: {}: {}", path.display(), err);
                    }
                    had_error_ref.store(true, AtomicOrdering::Relaxed);
                }
            }

            WalkState::Continue
        });

        if config.stats {
            let elapsed = search_start.elapsed();
            let mut stdout = BufferedStandardStream::stdout(config.color_choice);
            print_stats(&stats, elapsed, &mut stdout)?;
            stdout.flush()?;
        }
    }

    // Check if nothing was searched (warning).
    if stats.files_searched.load(AtomicOrdering::Relaxed) == 0
        && !had_match.load(AtomicOrdering::Relaxed)
        && !config.search_stdin
    {
        log::debug!(
            "No files were searched. Use --debug for more information."
        );
    }

    Ok(if had_match.load(AtomicOrdering::Relaxed) {
        EXIT_MATCH
    } else {
        EXIT_NO_MATCH
    })
}

// ---------------------------------------------------------------------------
// Search a single file (direct output to stdout)
// ---------------------------------------------------------------------------

fn search_one_file(
    config: &ResolvedConfig,
    matcher: &RegexMatcher,
    path: &Path,
    show_filename: bool,
    stats: &Arc<SearchStats>,
    stdout: &mut BufferedStandardStream,
) -> anyhow::Result<bool> {
    let mut searcher = build_searcher(config);
    let display_path = format_path(path, &config.path_separator);
    let path_label = if show_filename {
        Some(display_path.clone())
    } else {
        None
    };

    stats.files_searched.fetch_add(1, AtomicOrdering::Relaxed);

    // Handle preprocessor / decompression.
    if config.search_zip {
        if let Ok(reader) = grep_cli::DecompressionReaderBuilder::new().build(path) {
            let mut sink = PrinterSink::new(config, Some(matcher.clone()), path_label, Some(display_path.clone()), stdout, stats.clone());
            searcher.search_reader(matcher, reader, &mut sink)?;
            let had_match = sink.had_match;
            if let Some(ref bp) = sink.binary_match_path {
                eprintln!("Binary file {} matches", bp);
            }
            return Ok(had_match);
        }
    }

    if let Some(ref pre_cmd) = config.pre {
        let should_preprocess = if config.pre_globs.is_empty() {
            true
        } else {
            config.pre_globs.iter().any(|g| {
                globset::Glob::new(g)
                    .ok()
                    .map(|glob| glob.compile_matcher().is_match(path))
                    .unwrap_or(false)
            })
        };

        if should_preprocess {
            if let Ok(reader) = grep_cli::PreprocessorReader::new(pre_cmd, path) {
                let mut sink = PrinterSink::new(config, Some(matcher.clone()), path_label, Some(display_path.clone()), stdout, stats.clone());
                searcher.search_reader(matcher, reader, &mut sink)?;
                let had_match = sink.had_match;
                if let Some(ref bp) = sink.binary_match_path {
                    eprintln!("Binary file {} matches", bp);
                }
                return Ok(had_match);
            }
        }
    }

    let mut sink = PrinterSink::new(config, Some(matcher.clone()), path_label, Some(display_path), stdout, stats.clone());
    searcher.search_path(matcher, path, &mut sink)?;
    let had_match = sink.had_match;
    if let Some(ref bp) = sink.binary_match_path {
        eprintln!("Binary file {} matches", bp);
    }
    Ok(had_match)
}

// ---------------------------------------------------------------------------
// Search a single file (buffered output for parallel mode)
// ---------------------------------------------------------------------------

fn search_one_file_to_buffer(
    config: &ResolvedConfig,
    matcher: &RegexMatcher,
    path: &Path,
    show_filename: bool,
    stats: &Arc<SearchStats>,
    buffer: &mut Vec<u8>,
) -> anyhow::Result<(bool, Option<String>)> {
    let mut searcher = build_searcher(config);
    let display_path = format_path(path, &config.path_separator);
    let path_label = if show_filename {
        Some(display_path.clone())
    } else {
        None
    };

    stats.files_searched.fetch_add(1, AtomicOrdering::Relaxed);

    // For buffered mode, we write to a termcolor::Buffer.
    let mut color_buf = termcolor::Buffer::ansi();
    if config.color_choice == ColorChoice::Never {
        color_buf = termcolor::Buffer::no_color();
    }

    // Handle preprocessor / decompression.
    if config.search_zip {
        if let Ok(reader) = grep_cli::DecompressionReaderBuilder::new().build(path) {
            let mut sink =
                PrinterSink::new_buffered(config, Some(matcher.clone()), path_label.clone(), Some(display_path.clone()), &mut color_buf, stats.clone());
            searcher.search_reader(matcher, reader, &mut sink)?;
            let had_match = sink.had_match;
            let binary_path = sink.binary_match_path.clone();
            drop(sink);
            *buffer = color_buf.into_inner();
            return Ok((had_match, binary_path));
        }
    }

    if let Some(ref pre_cmd) = config.pre {
        let should_preprocess = if config.pre_globs.is_empty() {
            true
        } else {
            config.pre_globs.iter().any(|g| {
                globset::Glob::new(g)
                    .ok()
                    .map(|glob| glob.compile_matcher().is_match(path))
                    .unwrap_or(false)
            })
        };

        if should_preprocess {
            if let Ok(reader) = grep_cli::PreprocessorReader::new(pre_cmd, path) {
                let mut sink =
                    PrinterSink::new_buffered(config, Some(matcher.clone()), path_label.clone(), Some(display_path.clone()), &mut color_buf, stats.clone());
                searcher.search_reader(matcher, reader, &mut sink)?;
                let had_match = sink.had_match;
                let binary_path = sink.binary_match_path.clone();
                drop(sink);
                *buffer = color_buf.into_inner();
                return Ok((had_match, binary_path));
            }
        }
    }

    let mut sink =
        PrinterSink::new_buffered(config, Some(matcher.clone()), path_label, Some(display_path), &mut color_buf, stats.clone());
    searcher.search_path(matcher, path, &mut sink)?;
    let had_match = sink.had_match;
    let binary_path = sink.binary_match_path.clone();
    drop(sink);
    *buffer = color_buf.into_inner();
    Ok((had_match, binary_path))
}

// ---------------------------------------------------------------------------
// Path formatting
// ---------------------------------------------------------------------------

fn format_path(path: &Path, path_separator: &Option<String>) -> String {
    let mut s = path.to_string_lossy().to_string();
    if let Some(ref sep) = path_separator {
        s = s.replace(std::path::MAIN_SEPARATOR, sep);
    }
    s
}

// ---------------------------------------------------------------------------
// Statistics printing
// ---------------------------------------------------------------------------

fn print_stats(
    stats: &SearchStats,
    elapsed: std::time::Duration,
    out: &mut dyn Write,
) -> io::Result<()> {
    writeln!(out)?;
    writeln!(
        out,
        "{} matches",
        stats.total_matches.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(
        out,
        "{} matched lines",
        stats.total_matched_lines.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(
        out,
        "{} files contained matches",
        stats.files_with_matches.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(
        out,
        "{} files searched",
        stats.files_searched.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(
        out,
        "{} bytes printed",
        stats.bytes_printed.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(
        out,
        "{} bytes searched",
        stats.bytes_searched.load(AtomicOrdering::Relaxed)
    )?;
    writeln!(out, "{:.6} seconds spent searching", elapsed.as_secs_f64())?;
    writeln!(out, "{:.6} seconds total", elapsed.as_secs_f64())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// PrinterSink — implements Sink for all output formatting
// ---------------------------------------------------------------------------

/// The core output implementation. This struct implements `grep_searcher::Sink`
/// and handles all output formatting (standard, count, JSON, files-with-matches,
/// only-matching, replace, context, heading, colors, etc.)
struct PrinterSink<'a, W: WriteColor> {
    // Configuration reference (borrowed from caller)
    config: SinkConfig,
    /// The path label to display (None = no filename prefix).
    path: Option<String>,
    /// The file path for binary match reporting and JSON.
    file_path: Option<String>,
    /// The output writer.
    out: &'a mut W,
    /// Whether any match was found during this search.
    had_match: bool,
    /// Match count for this file.
    match_count: u64,
    /// Matched line count for this file.
    matched_line_count: u64,
    /// If binary was detected and we had a match, store path here.
    binary_match_path: Option<String>,
    /// Whether we've printed the heading for this file.
    heading_printed: bool,
    /// Whether we need a context separator before the next output.
    needs_context_separator: bool,
    /// Whether this is the first match group (to suppress leading separator).
    first_group: bool,
    /// Global stats reference.
    stats: Arc<SearchStats>,
    /// Bytes searched for this file.
    file_bytes: u64,
    /// Matcher for finding submatches (needed for JSON, only-matching, etc.)
    matcher: Option<RegexMatcher>,
}

/// Owned copy of configuration needed by the sink.
#[derive(Clone)]
struct SinkConfig {
    count: bool,
    count_matches: bool,
    files_with_matches: bool,
    files_without_match: bool,
    only_matching: bool,
    replace: Option<String>,
    show_line_number: bool,
    show_column: bool,
    byte_offset: bool,
    heading: bool,
    vimgrep: bool,
    json: bool,
    quiet: bool,
    null_sep: bool,
    trim: bool,
    stats: bool,
    max_columns: Option<u64>,
    max_columns_preview: bool,
    color_specs: ColorSpecs,
    use_color: bool,
    field_match_separator: String,
    field_context_separator: String,
    context_separator: Vec<u8>,
    before_context: usize,
    after_context: usize,
    binary_behavior: BinaryBehavior,
    invert_match: bool,
}

impl<'a, W: WriteColor> PrinterSink<'a, W> {
    fn new(
        config: &ResolvedConfig,
        matcher: Option<RegexMatcher>,
        path: Option<String>,
        file_path: Option<String>,
        out: &'a mut W,
        stats: Arc<SearchStats>,
    ) -> Self {
        let use_color = config.color_choice != ColorChoice::Never;
        PrinterSink {
            config: SinkConfig {
                count: config.count,
                count_matches: config.count_matches,
                files_with_matches: config.files_with_matches,
                files_without_match: config.files_without_match,
                only_matching: config.only_matching,
                replace: config.replace.clone(),
                show_line_number: config.show_line_number,
                show_column: config.show_column,
                byte_offset: config.byte_offset,
                heading: config.heading,
                vimgrep: config.vimgrep,
                json: config.json,
                quiet: config.quiet,
                null_sep: config.null_sep,
                trim: config.trim,
                stats: config.stats,
                max_columns: config.max_columns,
                max_columns_preview: config.max_columns_preview,
                color_specs: config.color_specs.clone(),
                use_color,
                field_match_separator: config.field_match_separator.clone(),
                field_context_separator: config.field_context_separator.clone(),
                context_separator: config.context_separator.clone(),
                before_context: config.before_context,
                after_context: config.after_context,
                binary_behavior: config.binary_detection,
                invert_match: config.invert_match,
            },
            path,
            file_path,
            out,
            had_match: false,
            match_count: 0,
            matched_line_count: 0,
            binary_match_path: None,
            heading_printed: false,
            needs_context_separator: false,
            first_group: true,
            stats,
            file_bytes: 0,
            matcher,
        }
    }

    fn new_buffered(
        config: &ResolvedConfig,
        matcher: Option<RegexMatcher>,
        path: Option<String>,
        file_path: Option<String>,
        out: &'a mut W,
        stats: Arc<SearchStats>,
    ) -> Self {
        Self::new(config, matcher, path, file_path, out, stats)
    }

    /// Write the path with color.
    fn write_path(&mut self) -> io::Result<()> {
        if let Some(ref path) = self.path {
            if self.config.use_color {
                self.out.set_color(&self.config.color_specs.get(grep_cli::OutType::Path))?;
            }
            write!(self.out, "{}", path)?;
            if self.config.use_color {
                self.out.reset()?;
            }
        }
        Ok(())
    }

    /// Write a line number with color.
    fn write_line_number(&mut self, line_number: u64) -> io::Result<()> {
        if self.config.use_color {
            self.out.set_color(&self.config.color_specs.get(grep_cli::OutType::Line))?;
        }
        write!(self.out, "{}", line_number)?;
        if self.config.use_color {
            self.out.reset()?;
        }
        Ok(())
    }

    /// Write a column number with color.
    fn write_column_number(&mut self, col: u64) -> io::Result<()> {
        if self.config.use_color {
            self.out.set_color(&self.config.color_specs.get(grep_cli::OutType::Column))?;
        }
        write!(self.out, "{}", col)?;
        if self.config.use_color {
            self.out.reset()?;
        }
        Ok(())
    }

    /// Write the matched text with highlighting.
    fn write_matched_line(
        &mut self,
        line: &[u8],
        matcher: &RegexMatcher,
    ) -> io::Result<()> {
        if !self.config.use_color {
            self.out.write_all(line)?;
            return Ok(());
        }

        // Find all matches on this line and highlight them.
        let mut last_end = 0;
        let match_spec = self.config.color_specs.get(grep_cli::OutType::Match);

        let result: Result<(), _> = matcher.find_iter(line, |m| {
            // Write non-match portion.
            let _ = self.out.write_all(&line[last_end..m.start()]);
            // Write match with color.
            let _ = self.out.set_color(&match_spec);
            let _ = self.out.write_all(&line[m.start()..m.end()]);
            let _ = self.out.reset();
            last_end = m.end();
            true
        });

        // Ignore matcher errors for display purposes.
        let _ = result;

        // Write remaining portion.
        if last_end < line.len() {
            self.out.write_all(&line[last_end..])?;
        }

        Ok(())
    }

    /// Print a heading line for a file.
    fn print_heading(&mut self) -> io::Result<()> {
        if self.heading_printed {
            return Ok(());
        }
        self.heading_printed = true;
        if let Some(ref path) = self.path {
            if self.config.use_color {
                self.out.set_color(&self.config.color_specs.get(grep_cli::OutType::Path))?;
            }
            write!(self.out, "{}", path)?;
            if self.config.use_color {
                self.out.reset()?;
            }
            writeln!(self.out)?;
        }
        Ok(())
    }

    /// Print the context separator.
    fn print_context_separator(&mut self) -> io::Result<()> {
        if !self.config.context_separator.is_empty() {
            self.out.write_all(&self.config.context_separator)?;
            writeln!(self.out)?;
        }
        Ok(())
    }

    fn trim_line<'b>(&self, line: &'b [u8]) -> &'b [u8] {
        if self.config.trim {
            let start = line.iter().position(|&b| !b.is_ascii_whitespace())
                .unwrap_or(line.len());
            &line[start..]
        } else {
            line
        }
    }
}

impl<'a, W: WriteColor> Sink for PrinterSink<'a, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.had_match = true;
        self.match_count += 1;
        self.matched_line_count += 1;

        // For quiet mode, signal that we found a match and stop.
        if self.config.quiet {
            return Ok(false);
        }

        // If binary quit mode and line contains NUL byte, do not print raw binary line.
        if self.config.binary_behavior == BinaryBehavior::Quit
            && memchr::memchr(0x00, mat.bytes()).is_some()
        {
            return Ok(false);
        }

        // For count modes, just count and continue.
        if self.config.count || self.config.count_matches {
            return Ok(true);
        }

        // For files-with-matches, just record and stop searching this file.
        if self.config.files_with_matches {
            return Ok(false);
        }

        // For files-without-match, keep searching (we need to know if it matches).
        if self.config.files_without_match {
            return Ok(false);
        }

        // JSON output mode.
        if self.config.json {
            return self.write_json_match(mat);
        }

        // Context separator.
        if self.needs_context_separator && !self.first_group {
            self.print_context_separator()?;
        }
        self.needs_context_separator = false;
        self.first_group = false;

        let line = mat.bytes();
        let line = self.trim_line(line);

        // Heading mode: print path header once.
        if self.config.heading && self.path.is_some() {
            self.print_heading()?;
        }

        // Handle --only-matching or --vimgrep.
        if self.config.only_matching || self.config.vimgrep {
            return self.write_only_matching(mat, searcher);
        }

        // Handle --replace.
        if self.config.replace.is_some() {
            return self.write_replaced_line(mat, searcher);
        }

        // Check max-columns.
        if let Some(max_cols) = self.config.max_columns {
            let line_no_term = strip_line_terminator(line, searcher.line_terminator());
            if line_no_term.len() as u64 > max_cols {
                if self.config.max_columns_preview {
                    // Truncate.
                    self.write_prefix(mat, &self.config.field_match_separator.clone())?;
                    let truncated = &line_no_term[..max_cols as usize];
                    self.out.write_all(truncated)?;
                    write!(self.out, " [... {} more bytes]", line_no_term.len() as u64 - max_cols)?;
                    writeln!(self.out)?;
                } else {
                    // Omit the line entirely but show a notice.
                    self.write_prefix(mat, &self.config.field_match_separator.clone())?;
                    write!(
                        self.out,
                        "[Omitted long matching line with {} bytes]",
                        line_no_term.len()
                    )?;
                    writeln!(self.out)?;
                }
                return Ok(true);
            }
        }

        // Standard output.
        let sep = self.config.field_match_separator.clone();
        self.write_prefix(mat, &sep)?;

        // Build a temporary matcher for color highlighting if needed.
        // Re-using the outer matcher would be ideal, but we don't have
        // access to it in the Sink trait. Instead, we highlight based
        // on the line content. For simplicity in this implementation,
        // we just write the line as-is (with line terminator).
        self.out.write_all(line)?;

        // Ensure line ends with newline.
        if !line.is_empty() && !line.ends_with(b"\n") {
            writeln!(self.out)?;
        }

        Ok(true)
    }

    fn context(
        &mut self,
        searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        if self.config.quiet || self.config.count || self.config.count_matches
            || self.config.files_with_matches || self.config.files_without_match
        {
            return Ok(true);
        }

        if self.config.json {
            return self.write_json_context(ctx);
        }

        // Heading mode.
        if self.config.heading && self.path.is_some() {
            self.print_heading()?;
        }

        let line = ctx.bytes();
        let line = self.trim_line(line);

        let sep = self.config.field_context_separator.clone();
        self.write_context_prefix(ctx, &sep)?;
        self.out.write_all(line)?;

        if !line.is_empty() && !line.ends_with(b"\n") {
            writeln!(self.out)?;
        }

        Ok(true)
    }

    fn context_break(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        self.needs_context_separator = true;
        Ok(true)
    }

    fn begin(
        &mut self,
        _searcher: &Searcher,
    ) -> Result<bool, io::Error> {
        if self.config.json {
            self.write_json_begin()?;
        }
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &Searcher,
        summary: &SinkFinish,
    ) -> Result<(), io::Error> {
        self.file_bytes = summary.byte_count;

        // Update global stats.
        self.stats.bytes_searched.fetch_add(
            summary.byte_count,
            AtomicOrdering::Relaxed,
        );

        if self.had_match {
            self.stats.files_with_matches.fetch_add(1, AtomicOrdering::Relaxed);
            self.stats.total_matches.fetch_add(
                self.match_count,
                AtomicOrdering::Relaxed,
            );
            self.stats.total_matched_lines.fetch_add(
                self.matched_line_count,
                AtomicOrdering::Relaxed,
            );
        }

        // Handle binary detection result.
        if summary.binary_byte_offset.is_some()
            && self.had_match
            && self.config.binary_behavior == BinaryBehavior::Quit
            && !self.config.quiet
        {
            if let Some(path) = self.path.as_ref().or(self.file_path.as_ref()) {
                self.binary_match_path = Some(path.clone());
            }
        }

        // Handle deferred output modes.
        if self.config.quiet {
            // No output needed.
        } else if self.config.count || self.config.count_matches {
            self.write_count()?;
        } else if self.config.files_with_matches {
            if self.had_match {
                self.write_filename()?;
            }
        } else if self.config.files_without_match {
            if !self.had_match {
                self.write_filename()?;
            }
        }

        if self.config.json {
            self.write_json_end(summary)?;
        }

        Ok(())
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, io::Error> {
        // Stop searching on binary data.
        Ok(false)
    }
}

// Additional methods on PrinterSink for various output formats.
impl<'a, W: WriteColor> PrinterSink<'a, W> {
    /// Write the prefix (path, line number, column, byte offset).
    fn write_prefix(
        &mut self,
        mat: &SinkMatch<'_>,
        separator: &str,
    ) -> io::Result<()> {
        if !self.config.heading {
            if let Some(ref _path) = self.path {
                self.write_path()?;
                write!(self.out, "{}", separator)?;
            }
        }

        if self.config.show_line_number {
            if let Some(ln) = mat.line_number() {
                self.write_line_number(ln)?;
                write!(self.out, "{}", separator)?;
            }
        }

        if self.config.show_column {
            // Compute column from first match on the line.
            let col = self.compute_column(mat.bytes());
            self.write_column_number(col)?;
            write!(self.out, "{}", separator)?;
        }

        if self.config.byte_offset {
            write!(self.out, "{}{}", mat.absolute_byte_offset(), separator)?;
        }

        Ok(())
    }

    /// Write the prefix for context lines.
    fn write_context_prefix(
        &mut self,
        ctx: &SinkContext<'_>,
        separator: &str,
    ) -> io::Result<()> {
        if !self.config.heading {
            if let Some(ref _path) = self.path {
                self.write_path()?;
                write!(self.out, "{}", separator)?;
            }
        }

        if self.config.show_line_number {
            if let Some(ln) = ctx.line_number() {
                self.write_line_number(ln)?;
                write!(self.out, "{}", separator)?;
            }
        }

        if self.config.byte_offset {
            write!(self.out, "{}{}", ctx.absolute_byte_offset(), separator)?;
        }

        Ok(())
    }

    /// Compute the 1-based column of the first match in the line.
    fn compute_column(&self, line: &[u8]) -> u64 {
        if let Some(ref m) = self.matcher {
            if let Ok(Some(mat)) = m.find(line) {
                return (mat.start() as u64) + 1;
            }
        }
        1
    }

    /// Write the count for count modes.
    fn write_count(&mut self) -> io::Result<()> {
        let count = if self.config.count_matches {
            self.match_count
        } else {
            self.matched_line_count
        };

        if let Some(ref _path) = self.path {
            self.write_path()?;
            write!(self.out, ":")?;
        }
        writeln!(self.out, "{}", count)?;
        Ok(())
    }

    /// Write the filename for files-with-matches / files-without-match.
    fn write_filename(&mut self) -> io::Result<()> {
        if let Some(ref path) = self.path {
            if self.config.use_color {
                self.out.set_color(&self.config.color_specs.get(grep_cli::OutType::Path))?;
            }
            write!(self.out, "{}", path)?;
            if self.config.use_color {
                self.out.reset()?;
            }
        }

        if self.config.null_sep {
            write!(self.out, "\0")?;
        } else {
            writeln!(self.out)?;
        }
        Ok(())
    }

    /// Write only-matching output (one match per line).
    fn write_only_matching(
        &mut self,
        mat: &SinkMatch<'_>,
        _searcher: &Searcher,
    ) -> io::Result<bool> {
        let line = mat.bytes();
        let sep = self.config.field_match_separator.clone();
        let mut match_ranges = Vec::new();
        if let Some(ref m) = self.matcher {
            let mut at = 0;
            while at < line.len() {
                if let Ok(Some(match_pos)) = m.find_at(line, at) {
                    if match_pos.start() == match_pos.end() {
                        at += 1;
                        continue;
                    }
                    match_ranges.push(match_pos.start()..match_pos.end());
                    at = match_pos.end();
                } else {
                    break;
                }
            }
        }
        if match_ranges.is_empty() {
            let line = strip_line_terminator(line, grep_matcher::LineTerminator::default());
            self.write_prefix(mat, &sep)?;
            self.out.write_all(line)?;
            writeln!(self.out)?;
        } else {
            for range in match_ranges {
                let matched_bytes = &line[range];
                self.write_prefix(mat, &sep)?;
                self.out.write_all(matched_bytes)?;
                writeln!(self.out)?;
            }
        }
        Ok(true)
    }

    /// Write a line with replacements applied.
    fn write_replaced_line(
        &mut self,
        mat: &SinkMatch<'_>,
        _searcher: &Searcher,
    ) -> io::Result<bool> {
        let line = mat.bytes();
        let sep = self.config.field_match_separator.clone();
        self.write_prefix(mat, &sep)?;

        if let (Some(ref m), Some(ref rep)) = (&self.matcher, &self.config.replace) {
            let mut dst = Vec::new();
            let rep_bytes = rep.as_bytes();
            let res = m.replace(line, &mut dst, |_m, d| {
                d.extend_from_slice(rep_bytes);
                true
            });
            if res.is_ok() {
                self.out.write_all(&dst)?;
            } else {
                self.out.write_all(line)?;
            }
        } else {
            self.out.write_all(line)?;
        }

        if !line.is_empty() && !line.ends_with(b"\n") {
            writeln!(self.out)?;
        }
        Ok(true)
    }

    // -- JSON output methods --

    fn write_json_begin(&mut self) -> io::Result<()> {
        let path_text = self.path.as_deref().unwrap_or("");
        let json = serde_json::json!({
            "type": "begin",
            "data": {
                "path": {"text": path_text}
            }
        });
        writeln!(self.out, "{}", json)?;
        Ok(())
    }

    fn write_json_match(
        &mut self,
        mat: &SinkMatch<'_>,
    ) -> io::Result<bool> {
        let line_bytes = mat.bytes();
        let line_text = String::from_utf8_lossy(line_bytes).to_string();
        let path_text = self.path.as_deref().unwrap_or("");

        let mut submatches = Vec::new();
        if let Some(ref m) = self.matcher {
            let mut at = 0;
            while at < line_bytes.len() {
                if let Ok(Some(mp)) = m.find_at(line_bytes, at) {
                    if mp.start() == mp.end() {
                        at += 1;
                        continue;
                    }
                    let m_text = String::from_utf8_lossy(&line_bytes[mp.start()..mp.end()]).to_string();
                    submatches.push(serde_json::json!({
                        "match": {"text": m_text},
                        "start": mp.start(),
                        "end": mp.end()
                    }));
                    at = mp.end();
                } else {
                    break;
                }
            }
        }
        if submatches.is_empty() {
            submatches.push(serde_json::json!({
                "match": {"text": line_text.trim_end_matches('\n')},
                "start": 0,
                "end": line_bytes.len()
            }));
        }

        let json = serde_json::json!({
            "type": "match",
            "data": {
                "path": {"text": path_text},
                "lines": {"text": line_text},
                "line_number": mat.line_number(),
                "absolute_offset": mat.absolute_byte_offset(),
                "submatches": submatches
            }
        });
        writeln!(self.out, "{}", json)?;
        Ok(true)
    }

    fn write_json_context(
        &mut self,
        ctx: &SinkContext<'_>,
    ) -> io::Result<bool> {
        let line_text = String::from_utf8_lossy(ctx.bytes()).to_string();
        let path_text = self.path.as_deref().unwrap_or("");

        let json = serde_json::json!({
            "type": "context",
            "data": {
                "path": {"text": path_text},
                "lines": {"text": line_text},
                "line_number": ctx.line_number(),
                "absolute_offset": ctx.absolute_byte_offset(),
                "submatches": []
            }
        });
        writeln!(self.out, "{}", json)?;
        Ok(true)
    }

    fn write_json_end(&mut self, summary: &SinkFinish) -> io::Result<()> {
        let path_text = self.path.as_deref().unwrap_or("");

        let json = serde_json::json!({
            "type": "end",
            "data": {
                "path": {"text": path_text},
                "binary_offset": summary.binary_byte_offset,
                "stats": {
                    "matched_lines": self.matched_line_count,
                    "matches": self.match_count,
                    "bytes_searched": summary.byte_count
                }
            }
        });
        writeln!(self.out, "{}", json)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

fn strip_line_terminator(line: &[u8], lt: grep_matcher::LineTerminator) -> &[u8] {
    let mut end = line.len();
    let term = lt.as_byte();
    if end > 0 && line[end - 1] == term {
        end -= 1;
    }
    if lt.is_crlf() && end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    &line[..end]
}

// ---------------------------------------------------------------------------
// Clone implementations needed for parallel mode
// ---------------------------------------------------------------------------

// ResolvedConfig needs Clone for Arc wrapping in parallel mode.
impl Clone for ResolvedConfig {
    fn clone(&self) -> Self {
        ResolvedConfig {
            patterns: self.patterns.clone(),
            paths: self.paths.clone(),
            search_stdin: self.search_stdin,
            fixed_strings: self.fixed_strings,
            word_regexp: self.word_regexp,
            line_regexp: self.line_regexp,
            case_insensitive: self.case_insensitive,
            case_smart: self.case_smart,
            invert_match: self.invert_match,
            multiline: self.multiline,
            multiline_dotall: self.multiline_dotall,
            count: self.count,
            count_matches: self.count_matches,
            files_with_matches: self.files_with_matches,
            files_without_match: self.files_without_match,
            only_matching: self.only_matching,
            replace: self.replace.clone(),
            show_line_number: self.show_line_number,
            show_column: self.show_column,
            show_filename: self.show_filename,
            byte_offset: self.byte_offset,
            heading: self.heading,
            vimgrep: self.vimgrep,
            stats: self.stats,
            json: self.json,
            quiet: self.quiet,
            null_sep: self.null_sep,
            trim: self.trim,
            before_context: self.before_context,
            after_context: self.after_context,
            context_separator: self.context_separator.clone(),
            hidden: self.hidden,
            no_ignore: self.no_ignore,
            no_ignore_vcs: self.no_ignore_vcs,
            no_ignore_global: self.no_ignore_global,
            no_ignore_parent: self.no_ignore_parent,
            follow_links: self.follow_links,
            max_depth: self.max_depth,
            max_filesize: self.max_filesize,
            one_file_system: self.one_file_system,
            binary_detection: self.binary_detection,
            regex_size_limit: self.regex_size_limit,
            dfa_size_limit: self.dfa_size_limit,
            no_unicode: self.no_unicode,
            crlf: self.crlf,
            null_data: self.null_data,
            max_count: self.max_count,
            max_columns: self.max_columns,
            max_columns_preview: self.max_columns_preview,
            memory_map: self.memory_map,
            threads: self.threads,
            sort: self.sort.clone(),
            search_zip: self.search_zip,
            pre: self.pre.clone(),
            pre_globs: self.pre_globs.clone(),
            stop_on_nonmatch: self.stop_on_nonmatch,
            color_choice: self.color_choice,
            color_specs: self.color_specs.clone(),
            path_separator: self.path_separator.clone(),
            no_messages: self.no_messages,
            field_match_separator: self.field_match_separator.clone(),
            field_context_separator: self.field_context_separator.clone(),
            glob_overrides: self.glob_overrides.clone(),
            types_filter: self.types_filter.clone(),
        }
    }
}

// RegexMatcher needs Clone for Arc wrapping.
// grep_regex::RegexMatcher already derives Debug but let's check if it has Clone.
// If not, we wrap it differently. Since RegexMatcher contains a Regex which is Clone,
// we can work around this. For now, let's build a new matcher per thread.

// We'll use a factory function instead of cloning the matcher.

// ---------------------------------------------------------------------------
// Parallel search with matcher factory
// ---------------------------------------------------------------------------

// The parallel search in search_paths uses Arc<RegexMatcher>. We need to verify
// that RegexMatcher implements Send + Sync (Regex does). If not, we build per-thread.

// RegexMatcher contains Regex which is Send+Sync, and Option<ByteSet> and
// Option<LineTerminator> which are also Send+Sync. So wrapping in Arc should work.

// However, RegexMatcher doesn't derive Clone. Let's work around by rebuilding
// the matcher in each thread or by sharing via Arc (which only needs Send+Sync).

// Since Matcher trait methods take &self, we can share via Arc without issue.

// The Arc::new(matcher.clone()) in search_paths won't compile if RegexMatcher
// doesn't implement Clone. Let's fix this by not cloning — just wrapping:

// Actually looking at the code again, search_paths does:
//   let matcher = Arc::new(matcher.clone());
// but the caller passes &RegexMatcher. We need to restructure this.
// Let's fix the search_paths function to accept owned RegexMatcher for parallel.
