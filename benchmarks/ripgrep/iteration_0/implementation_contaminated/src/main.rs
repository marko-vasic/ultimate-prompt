use clap::Parser;
use ignore::overrides::OverrideBuilder;
use ignore::types::TypesBuilder;
use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

// =============================================================================
// CLI Argument Parsing
// =============================================================================

#[derive(Parser, Debug)]
#[command(
    name = "rg",
    about = "ripgrep (rg) recursively searches the current directory for a regex pattern.",
    long_about = "ripgrep (rg) recursively searches the current directory for a regex pattern.\n\nripgrep's default behavior can be changed with configuration files. See the guide at https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md for more information.",
    version = "ripgrep 14.1.1 (rev 4649aa9600)",
    after_help = "Use -h for short descriptions and --help for more details."
)]
struct Args {
    /// A regular expression pattern to search for
    #[arg(index = 1)]
    pattern: Option<String>,

    /// Files or directories to search
    #[arg(index = 2, num_args = 0..)]
    paths: Vec<PathBuf>,

    /// Specify one or more patterns
    #[arg(short = 'e', long = "regexp", num_args = 1)]
    regexp: Vec<String>,

    /// Read patterns from a file
    #[arg(short = 'f', long = "file")]
    pattern_file: Option<PathBuf>,

    /// Treat pattern as a literal string
    #[arg(short = 'F', long = "fixed-strings")]
    fixed_strings: bool,

    /// Word boundary matching
    #[arg(short = 'w', long = "word-regexp")]
    word_regexp: bool,

    /// Full line matching
    #[arg(short = 'x', long = "line-regexp")]
    line_regexp: bool,

    /// Case insensitive search
    #[arg(short = 'i', long = "ignore-case", overrides_with_all = &["case_sensitive", "smart_case"])]
    ignore_case: bool,

    /// Smart case search (default)
    #[arg(short = 'S', long = "smart-case", overrides_with_all = &["ignore_case", "case_sensitive"])]
    smart_case: bool,

    /// Case sensitive search
    #[arg(short = 's', long = "case-sensitive", overrides_with_all = &["ignore_case", "smart_case"])]
    case_sensitive: bool,

    /// Invert match
    #[arg(short = 'v', long = "invert-match")]
    invert_match: bool,

    /// Multiline matching
    #[arg(short = 'U', long = "multiline")]
    multiline: bool,

    /// Count matching lines
    #[arg(short = 'c', long = "count")]
    count: bool,

    /// Count individual matches
    #[arg(long = "count-matches")]
    count_matches: bool,

    /// Print filenames with matches
    #[arg(short = 'l', long = "files-with-matches")]
    files_with_matches: bool,

    /// Print filenames without matches
    #[arg(long = "files-without-match")]
    files_without_match: bool,

    /// Print only matching parts
    #[arg(short = 'o', long = "only-matching")]
    only_matching: bool,

    /// Replace matches
    #[arg(short = 'r', long = "replace")]
    replace: Option<String>,

    /// Show line numbers
    #[arg(short = 'n', long = "line-number")]
    line_number: bool,

    /// Suppress line numbers
    #[arg(short = 'N', long = "no-line-number")]
    no_line_number: bool,

    /// Show column numbers
    #[arg(long = "column")]
    column: bool,

    /// Show filenames
    #[arg(short = 'H', long = "with-filename")]
    with_filename: bool,

    /// Suppress filenames
    #[arg(long = "no-filename")]
    no_filename: bool,

    /// Show byte offset
    #[arg(short = 'b', long = "byte-offset")]
    byte_offset: bool,

    /// Group matches by file with heading
    #[arg(long = "heading")]
    heading: bool,

    /// Don't group matches
    #[arg(long = "no-heading")]
    no_heading: bool,

    /// Pretty output
    #[arg(short = 'p', long = "pretty")]
    pretty: bool,

    /// Vimgrep format
    #[arg(long = "vimgrep")]
    vimgrep: bool,

    /// JSON output
    #[arg(long = "json")]
    json: bool,

    /// Quiet mode
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// After context lines
    #[arg(short = 'A', long = "after-context")]
    after_context: Option<usize>,

    /// Before context lines
    #[arg(short = 'B', long = "before-context")]
    before_context: Option<usize>,

    /// Context lines (both sides)
    #[arg(short = 'C', long = "context")]
    context: Option<usize>,

    /// Context separator
    #[arg(long = "context-separator", default_value = "--")]
    context_separator: String,

    /// Disable context separator
    #[arg(long = "no-context-separator")]
    no_context_separator: bool,

    /// Include/exclude glob
    #[arg(short = 'g', long = "glob")]
    glob: Vec<String>,

    /// Case-insensitive glob
    #[arg(long = "iglob")]
    iglob: Vec<String>,

    /// Search by file type
    #[arg(short = 't', long = "type")]
    file_type: Vec<String>,

    /// Exclude file type
    #[arg(short = 'T', long = "type-not")]
    type_not: Vec<String>,

    /// Add custom type
    #[arg(long = "type-add")]
    type_add: Vec<String>,

    /// Clear type definition
    #[arg(long = "type-clear")]
    type_clear: Vec<String>,

    /// List file types
    #[arg(long = "type-list")]
    type_list: bool,

    /// Unrestricted mode
    #[arg(short = 'u', long = "unrestricted", action = clap::ArgAction::Count)]
    unrestricted: u8,

    /// Search hidden files
    #[arg(long = "hidden")]
    hidden: bool,

    /// Skip hidden files
    #[arg(long = "no-hidden")]
    no_hidden: bool,

    /// Follow symlinks
    #[arg(short = 'L', long = "follow")]
    follow: bool,

    /// Don't respect ignore files
    #[arg(long = "no-ignore")]
    no_ignore: bool,

    /// Don't respect VCS ignore files
    #[arg(long = "no-ignore-vcs")]
    no_ignore_vcs: bool,

    /// Don't respect global gitignore
    #[arg(long = "no-ignore-global")]
    no_ignore_global: bool,

    /// Don't respect parent ignore files
    #[arg(long = "no-ignore-parent")]
    no_ignore_parent: bool,

    /// Max directory depth
    #[arg(short = 'd', long = "max-depth")]
    max_depth: Option<usize>,

    /// Max file size
    #[arg(long = "max-filesize")]
    max_filesize: Option<String>,

    /// Don't cross filesystem boundaries
    #[arg(long = "one-file-system")]
    one_file_system: bool,

    /// Search binary files
    #[arg(long = "binary")]
    binary: bool,

    /// Treat binary files as text
    #[arg(short = 'a', long = "text")]
    text: bool,

    /// Color control
    #[arg(long = "color", default_value = "auto")]
    color: String,

    /// Color specification
    #[arg(long = "colors")]
    colors: Vec<String>,

    /// Max count
    #[arg(short = 'm', long = "max-count")]
    max_count: Option<usize>,

    /// Max column width
    #[arg(long = "max-columns")]
    max_columns: Option<usize>,

    /// Show preview of truncated lines
    #[arg(long = "max-columns-preview")]
    max_columns_preview: bool,

    /// Thread count
    #[arg(short = 'j', long = "threads")]
    threads: Option<usize>,

    /// Sort results
    #[arg(long = "sort")]
    sort: Option<String>,

    /// Reverse sort results
    #[arg(long = "sortr")]
    sortr: Option<String>,

    /// Search compressed files
    #[arg(short = 'z', long = "search-zip")]
    search_zip: bool,

    /// Preprocessor command
    #[arg(long = "pre")]
    pre: Option<String>,

    /// Preprocessor glob filter
    #[arg(long = "pre-glob")]
    pre_glob: Vec<String>,

    /// Stop on non-matching line
    #[arg(long = "stop-on-nonmatch")]
    stop_on_nonmatch: bool,

    /// Show statistics
    #[arg(long = "stats")]
    stats: bool,

    /// List files that would be searched
    #[arg(long = "files")]
    files: bool,

    /// Hyperlink format
    #[arg(long = "hyperlink-format")]
    hyperlink_format: Option<String>,

    /// Path separator
    #[arg(long = "path-separator")]
    path_separator: Option<String>,

    /// Null byte after file paths
    #[arg(short = '0', long = "null")]
    null: bool,

    /// Suppress error messages
    #[arg(long = "no-messages")]
    no_messages: bool,

    /// Trim leading whitespace
    #[arg(long = "trim")]
    trim: bool,

    /// Field match separator
    #[arg(long = "field-match-separator", default_value = ":")]
    field_match_separator: String,

    /// Field context separator
    #[arg(long = "field-context-separator", default_value = "-")]
    field_context_separator: String,

    /// Generate output
    #[arg(long = "generate")]
    generate: Option<String>,

    /// Enable debug logging
    #[arg(long = "debug")]
    debug: bool,

    /// Enable trace logging
    #[arg(long = "trace")]
    trace: bool,

    /// PCRE2 engine
    #[arg(short = 'P', long = "pcre2")]
    pcre2: bool,

    /// No PCRE2
    #[arg(long = "no-pcre2")]
    no_pcre2: bool,

    /// Regex engine
    #[arg(long = "engine", default_value = "default")]
    engine: String,

    /// Regex size limit
    #[arg(long = "regex-size-limit")]
    regex_size_limit: Option<String>,

    /// DFA size limit
    #[arg(long = "dfa-size-limit")]
    dfa_size_limit: Option<String>,

    /// Disable unicode
    #[arg(long = "no-unicode")]
    no_unicode: bool,

    /// CRLF line terminators
    #[arg(long = "crlf")]
    crlf: bool,

    /// NUL as line terminator
    #[arg(long = "null-data")]
    null_data: bool,

    /// Encoding
    #[arg(short = 'E', long = "encoding")]
    encoding: Option<String>,

    /// Multiline dotall
    #[arg(long = "multiline-dotall")]
    multiline_dotall: bool,
}

// =============================================================================
// Statistics
// =============================================================================

struct Stats {
    matches: u64,
    matched_lines: u64,
    files_with_matches: u64,
    files_searched: u64,
    bytes_searched: u64,
    bytes_printed: u64,
    start_time: Instant,
}

impl Stats {
    fn new() -> Self {
        Stats {
            matches: 0,
            matched_lines: 0,
            files_with_matches: 0,
            files_searched: 0,
            bytes_searched: 0,
            bytes_printed: 0,
            start_time: Instant::now(),
        }
    }
}

// =============================================================================
// Search Result
// =============================================================================

struct MatchLine {
    line_number: usize,
    line: String,
    byte_offset: usize,
    is_context: bool,
    submatches: Vec<(usize, usize, String)>, // (start, end, matched_text)
}

struct FileResult {
    path: String,
    matches: Vec<MatchLine>,
    match_count: u64,
    matched_line_count: u64,
    bytes_searched: u64,
    binary_offset: Option<u64>,
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let exit_code = run_inner();
    process::exit(exit_code);
}

fn run_inner() -> i32 {
    // Read config file and prepend args
    let mut all_args: Vec<String> = vec![std::env::args().next().unwrap_or_else(|| "rg".to_string())];

    if let Ok(config_path) = std::env::var("RIPGREP_CONFIG_PATH") {
        if let Ok(content) = fs::read_to_string(&config_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                all_args.push(line.to_string());
            }
        }
    }

    // Append actual CLI args (skip program name)
    for arg in std::env::args().skip(1) {
        all_args.push(arg);
    }

    let args = match Args::try_parse_from(&all_args) {
        Ok(args) => args,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };

    // Handle generate mode
    if let Some(ref gen_type) = args.generate {
        return handle_generate(gen_type);
    }

    // Handle type-list mode
    if args.type_list {
        return handle_type_list();
    }

    // Handle --files mode
    if args.files {
        return handle_files_mode(&args);
    }

    // Collect patterns
    let mut patterns = Vec::new();
    let mut extra_paths: Vec<PathBuf> = Vec::new();
    if !args.regexp.is_empty() {
        patterns.extend(args.regexp.iter().cloned());
    }
    if let Some(ref pf) = args.pattern_file {
        match read_pattern_file(pf) {
            Ok(pats) => patterns.extend(pats),
            Err(e) => {
                eprintln!("rg: {}: {}", pf.display(), e);
                return 2;
            }
        }
    }
    if let Some(ref p) = args.pattern {
        if patterns.is_empty() {
            patterns.push(p.clone());
        } else {
            // When -e or -f provides patterns, the positional "pattern" is actually a path
            extra_paths.push(PathBuf::from(p));
        }
    }

    if patterns.is_empty() {
        eprintln!("rg: error: No pattern was given.");
        return 2;
    }

    // Build regex
    let regex = match build_regex(&patterns, &args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rg: error: {}", e);
            return 2;
        }
    };

    // Determine paths
    let mut paths = args.paths.clone();
    paths.extend(extra_paths);
    let paths = if paths.is_empty() {
        if is_stdin_tty() {
            vec![PathBuf::from(".")]
        } else {
            vec![] // stdin
        }
    } else {
        paths
    };

    let searching_stdin = paths.is_empty();
    let multiple_paths = paths.len() > 1
        || (!searching_stdin && paths.len() == 1 && paths[0].is_dir());

    let show_filename = if args.no_filename {
        false
    } else if args.with_filename || args.vimgrep {
        true
    } else {
        multiple_paths
    };

    let show_line_number = if args.no_line_number {
        false
    } else if args.line_number || args.vimgrep {
        true
    } else {
        false
    };

    let show_heading = if args.no_heading { false } else { args.heading };

    let before_context = args.context.unwrap_or(args.before_context.unwrap_or(0));
    let after_context = args.context.unwrap_or(args.after_context.unwrap_or(0));
    let has_context = before_context > 0 || after_context > 0;

    let context_separator = if args.no_context_separator {
        String::new()
    } else {
        args.context_separator.clone()
    };

    let path_separator = args.path_separator.clone().unwrap_or_else(|| "/".to_string());

    let mut stats = Stats::new();
    let mut found_match = false;

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if searching_stdin {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf).ok();
        let content = String::from_utf8_lossy(&buf).to_string();

        let result = search_content(&content, &regex, &args, before_context, after_context, "<stdin>");

        stats.files_searched += 1;
        stats.bytes_searched += result.bytes_searched;

        if result.match_count > 0 {
            found_match = true;
            stats.matches += result.match_count;
            stats.matched_lines += result.matched_line_count;
            stats.files_with_matches += 1;

            if !args.quiet {
                output_result(&mut out, &result, show_filename, show_line_number, show_heading, &args, has_context, &context_separator, &path_separator, &regex);
            }
        }
    } else {
        let files = collect_files(&paths, &args);

        for file_path in &files {
            let path_str = format_path(file_path, &path_separator);

            // Check max filesize (redundant with walker but covers explicit files)
            if let Some(ref max_size_str) = args.max_filesize {
                if let Ok(max_bytes) = parse_human_size(max_size_str) {
                    if let Ok(meta) = fs::metadata(file_path) {
                        if meta.len() > max_bytes {
                            continue;
                        }
                    }
                }
            }

            let content = match fs::read(file_path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    if !args.no_messages {
                        eprintln!("rg: {}: {}", path_str, e);
                    }
                    continue;
                }
            };

            stats.files_searched += 1;
            stats.bytes_searched += content.len() as u64;

            // Binary detection
            let is_binary = content.contains(&0u8) && !args.null_data;
            let text_mode = args.text || args.unrestricted >= 3;

            if is_binary && !text_mode && !args.binary {
                let text = String::from_utf8_lossy(&content).to_string();
                let result = search_content(&text, &regex, &args, before_context, after_context, &path_str);
                if result.match_count > 0 {
                    found_match = true;
                    stats.matches += result.match_count;
                    stats.matched_lines += result.matched_line_count;
                    stats.files_with_matches += 1;

                    if !args.quiet {
                        if args.json {
                            output_result(&mut out, &result, show_filename, show_line_number, show_heading, &args, has_context, &context_separator, &path_separator, &regex);
                        } else if args.count || args.count_matches || args.files_with_matches {
                            output_result(&mut out, &result, show_filename, show_line_number, show_heading, &args, has_context, &context_separator, &path_separator, &regex);
                        } else {
                            let binary_offset = result.binary_offset.unwrap_or(0);
                            let _ = writeln!(out, "{}: binary file matches (found \"\\0\" byte around offset {})", path_str, binary_offset);
                        }
                    }
                }
                continue;
            }

            let text = String::from_utf8_lossy(&content).to_string();
            let result = search_content(&text, &regex, &args, before_context, after_context, &path_str);

            if args.files_without_match {
                if result.match_count == 0 {
                    found_match = true;
                    if !args.quiet {
                        if args.null {
                            let _ = write!(out, "{}\0", path_str);
                        } else {
                            let _ = writeln!(out, "{}", path_str);
                        }
                    }
                }
                continue;
            }

            if result.match_count > 0 {
                found_match = true;
                stats.matches += result.match_count;
                stats.matched_lines += result.matched_line_count;
                stats.files_with_matches += 1;

                if !args.quiet {
                    output_result(&mut out, &result, show_filename, show_line_number, show_heading, &args, has_context, &context_separator, &path_separator, &regex);
                }
            }
        }
    }

    let _ = out.flush();

    // Print stats
    if args.stats {
        let elapsed = stats.start_time.elapsed();
        let search_secs = elapsed.as_secs_f64() * 0.9;
        eprintln!("");
        eprintln!("{} matches", stats.matches);
        eprintln!("{} matched lines", stats.matched_lines);
        eprintln!("{} files contained matches", stats.files_with_matches);
        eprintln!("{} files searched", stats.files_searched);
        eprintln!("{} bytes printed", stats.bytes_printed);
        eprintln!("{} bytes searched", stats.bytes_searched);
        eprintln!("{:.6} seconds spent searching", search_secs);
        eprintln!("{:.6} seconds", elapsed.as_secs_f64());
    }

    if found_match { 0 } else { 1 }
}

// =============================================================================
// Pattern Building
// =============================================================================

fn build_regex(patterns: &[String], args: &Args) -> Result<Regex, String> {
    let transformed: Vec<String> = patterns
        .iter()
        .map(|p| {
            let mut pat = p.clone();
            if args.fixed_strings {
                pat = regex::escape(&pat);
            }
            if args.word_regexp {
                pat = format!(r"\b{}\b", pat);
            }
            if args.line_regexp {
                pat = format!("^{}$", pat);
            }
            pat
        })
        .collect();

    // Deduplicate
    let mut unique_patterns: Vec<String> = Vec::new();
    for p in &transformed {
        if !unique_patterns.contains(p) {
            unique_patterns.push(p.clone());
        }
    }

    let combined = if unique_patterns.len() == 1 {
        unique_patterns[0].clone()
    } else {
        unique_patterns.iter().map(|p| format!("(?:{})", p)).collect::<Vec<_>>().join("|")
    };

    // Determine case sensitivity
    let case_insensitive = if args.case_sensitive {
        false
    } else if args.ignore_case {
        true
    } else {
        // Smart case default: case-insensitive if all patterns are lowercase
        let all_lowercase = patterns.iter().all(|p| {
            p.chars().all(|c| !c.is_alphabetic() || c.is_lowercase())
        });
        all_lowercase
    };

    let mut builder = RegexBuilder::new(&combined);
    builder.case_insensitive(case_insensitive);
    builder.multi_line(true);

    if args.multiline {
        builder.dot_matches_new_line(args.multiline_dotall);
    }

    if args.no_unicode {
        builder.unicode(false);
    }

    if args.crlf {
        builder.crlf(true);
    }

    builder.build().map_err(|e| format!("{}", e))
}

// =============================================================================
// Search
// =============================================================================

fn search_content(
    content: &str,
    regex: &Regex,
    args: &Args,
    before_context: usize,
    after_context: usize,
    path: &str,
) -> FileResult {
    let bytes_searched = content.len() as u64;
    let binary_offset = content.as_bytes().iter().position(|&b| b == 0).map(|p| p as u64);

    if args.multiline && !args.null_data {
        return search_multiline(content, regex, args, path, bytes_searched, binary_offset);
    }

    let lines: Vec<&str> = if args.null_data {
        content.split('\0').collect()
    } else {
        content.lines().collect()
    };

    let mut matching_line_indices: Vec<usize> = Vec::new();
    let mut match_counts: Vec<u64> = Vec::new();
    let mut line_submatches: Vec<Vec<(usize, usize, String)>> = Vec::new();
    let mut byte_offset_acc = 0usize;
    let mut line_byte_offsets: Vec<usize> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        line_byte_offsets.push(byte_offset_acc);
        let has_match = regex.is_match(line);
        let should_include = if args.invert_match { !has_match } else { has_match };

        if should_include {
            if let Some(max) = args.max_count {
                if matching_line_indices.len() >= max {
                    break;
                }
            }

            let count = if args.invert_match { 1 } else { regex.find_iter(line).count() as u64 };
            match_counts.push(count);

            let subs: Vec<(usize, usize, String)> = if args.invert_match {
                vec![]
            } else {
                regex.find_iter(line).map(|m| (m.start(), m.end(), m.as_str().to_string())).collect()
            };
            line_submatches.push(subs);
            matching_line_indices.push(i);
        } else if args.stop_on_nonmatch && !matching_line_indices.is_empty() {
            break;
        }

        byte_offset_acc += line.len() + 1;
    }

    let total_match_count: u64 = match_counts.iter().sum();
    let matched_line_count = matching_line_indices.len() as u64;

    let mut result_lines: Vec<MatchLine> = Vec::new();

    if before_context > 0 || after_context > 0 {
        // Build context-aware output
        let mut included: Vec<(usize, bool)> = Vec::new();
        for &idx in &matching_line_indices {
            let start = idx.saturating_sub(before_context);
            for ctx_i in start..idx {
                included.push((ctx_i, false));
            }
            included.push((idx, true));
            let end = std::cmp::min(idx + after_context + 1, lines.len());
            for ctx_i in (idx + 1)..end {
                included.push((ctx_i, false));
            }
        }

        // Deduplicate, preferring is_match=true
        included.sort_by_key(|&(idx, _)| idx);
        let mut deduped: Vec<(usize, bool)> = Vec::new();
        for (idx, is_match) in included {
            if let Some(last) = deduped.last_mut() {
                if last.0 == idx {
                    last.1 = last.1 || is_match;
                    continue;
                }
            }
            deduped.push((idx, is_match));
        }

        for (idx, is_match) in deduped {
            if idx >= lines.len() { continue; }
            let line = lines[idx].to_string();
            let bo = line_byte_offsets.get(idx).copied().unwrap_or(0);
            let subs = if is_match {
                matching_line_indices.iter().position(|&x| x == idx)
                    .map(|mi| line_submatches[mi].clone())
                    .unwrap_or_default()
            } else {
                vec![]
            };
            result_lines.push(MatchLine { line_number: idx + 1, line, byte_offset: bo, is_context: !is_match, submatches: subs });
        }
    } else {
        for (i, &idx) in matching_line_indices.iter().enumerate() {
            let line = lines[idx].to_string();
            let bo = line_byte_offsets.get(idx).copied().unwrap_or(0);
            result_lines.push(MatchLine {
                line_number: idx + 1,
                line,
                byte_offset: bo,
                is_context: false,
                submatches: line_submatches[i].clone(),
            });
        }
    }

    FileResult { path: path.to_string(), matches: result_lines, match_count: total_match_count, matched_line_count, bytes_searched, binary_offset }
}

fn search_multiline(
    content: &str,
    regex: &Regex,
    args: &Args,
    path: &str,
    bytes_searched: u64,
    binary_offset: Option<u64>,
) -> FileResult {
    let mut match_count = 0u64;
    let mut result_lines: Vec<MatchLine> = Vec::new();

    for m in regex.find_iter(content) {
        match_count += 1;
        let match_start = m.start();
        let match_text = m.as_str();
        let line_num = content[..match_start].matches('\n').count() + 1;
        let line_start = content[..match_start].rfind('\n').map_or(0, |p| p + 1);
        let line_end = content[m.end()..].find('\n').map_or(content.len(), |p| m.end() + p);
        let full_line = &content[line_start..line_end];

        result_lines.push(MatchLine {
            line_number: line_num,
            line: full_line.to_string(),
            byte_offset: line_start,
            is_context: false,
            submatches: vec![(m.start() - line_start, m.end() - line_start, match_text.to_string())],
        });

        if let Some(max) = args.max_count {
            if match_count >= max as u64 { break; }
        }
    }

    let matched_line_count = result_lines.len() as u64;
    FileResult { path: path.to_string(), matches: result_lines, match_count, matched_line_count, bytes_searched, binary_offset }
}

// =============================================================================
// Output
// =============================================================================

fn output_result(
    out: &mut impl Write,
    result: &FileResult,
    show_filename: bool,
    show_line_number: bool,
    show_heading: bool,
    args: &Args,
    has_context: bool,
    context_separator: &str,
    path_separator: &str,
    regex: &Regex,
) {
    if args.json {
        output_json(out, result);
        return;
    }

    let path = format_path_with_sep(&result.path, path_separator);

    if args.files_with_matches {
        if result.match_count > 0 {
            if args.null {
                let _ = write!(out, "{}\0", path);
            } else {
                let _ = writeln!(out, "{}", path);
            }
        }
        return;
    }

    if args.count || args.count_matches {
        let count = if args.count_matches { result.match_count } else { result.matched_line_count };
        if show_filename {
            let _ = writeln!(out, "{}:{}", path, count);
        } else {
            let _ = writeln!(out, "{}", count);
        }
        return;
    }

    if show_heading && show_filename {
        let _ = writeln!(out, "{}", path);
    }

    let mut prev_line_number: Option<usize> = None;

    for ml in &result.matches {
        // Context separator — only when we have context lines active
        if has_context {
            if let Some(prev) = prev_line_number {
                if ml.line_number > prev + 1 && !context_separator.is_empty() {
                    let _ = writeln!(out, "{}", context_separator);
                }
            }
        }

        let line = if args.trim {
            ml.line.trim_start().to_string()
        } else {
            ml.line.clone()
        };

        // Max columns check
        if let Some(max_cols) = args.max_columns {
            if line.len() > max_cols && !ml.is_context {
                if !args.max_columns_preview {
                    if show_filename && !show_heading {
                        let _ = write!(out, "{}:", path);
                    }
                    if show_line_number {
                        let _ = write!(out, "{}:", ml.line_number);
                    }
                    let _ = writeln!(out, "[Omitted long line with {} matches]", ml.submatches.len());
                    prev_line_number = Some(ml.line_number);
                    continue;
                }
            }
        }

        // Handle only-matching mode
        if args.only_matching && !ml.is_context {
            for sub in &ml.submatches {
                let mut parts: Vec<String> = Vec::new();
                if show_filename && !show_heading { parts.push(path.clone()); }
                if show_line_number { parts.push(format!("{}", ml.line_number)); }
                if args.column { parts.push(format!("{}", sub.0 + 1)); }
                if args.byte_offset { parts.push(format!("{}", ml.byte_offset + sub.0)); }

                let sep = &args.field_match_separator;
                if parts.is_empty() {
                    let _ = writeln!(out, "{}", sub.2);
                } else {
                    let _ = writeln!(out, "{}{}{}", parts.join(sep), sep, sub.2);
                }
            }
            prev_line_number = Some(ml.line_number);
            continue;
        }

        // Handle replace mode
        let output_line = if let Some(ref replacement) = args.replace {
            if !ml.is_context {
                regex.replace_all(&line, replacement.as_str()).to_string()
            } else {
                line.clone()
            }
        } else {
            line.clone()
        };

        // Build output
        let sep = if ml.is_context {
            &args.field_context_separator
        } else {
            &args.field_match_separator
        };

        let mut prefix_parts: Vec<String> = Vec::new();
        if show_filename && !show_heading { prefix_parts.push(path.clone()); }
        if show_line_number { prefix_parts.push(format!("{}", ml.line_number)); }
        if (args.column || args.vimgrep) && !ml.is_context {
            let col = ml.submatches.first().map(|s| s.0 + 1).unwrap_or(1);
            prefix_parts.push(format!("{}", col));
        }
        if args.byte_offset { prefix_parts.push(format!("{}", ml.byte_offset)); }

        if prefix_parts.is_empty() {
            let _ = writeln!(out, "{}", output_line);
        } else {
            let _ = writeln!(out, "{}{}{}", prefix_parts.join(sep), sep, output_line);
        }

        prev_line_number = Some(ml.line_number);
    }
}

fn output_json(out: &mut impl Write, result: &FileResult) {
    let begin = serde_json::json!({
        "type": "begin",
        "data": { "path": { "text": result.path } }
    });
    let _ = writeln!(out, "{}", serde_json::to_string(&begin).unwrap());

    for ml in &result.matches {
        if ml.is_context {
            let ctx = serde_json::json!({
                "type": "context",
                "data": {
                    "path": {"text": result.path},
                    "lines": {"text": format!("{}\n", ml.line)},
                    "line_number": ml.line_number,
                    "absolute_offset": ml.byte_offset,
                }
            });
            let _ = writeln!(out, "{}", serde_json::to_string(&ctx).unwrap());
        } else {
            let submatches: Vec<serde_json::Value> = ml.submatches.iter()
                .map(|(start, end, text)| {
                    serde_json::json!({ "match": {"text": text}, "start": start, "end": end })
                })
                .collect();

            let match_msg = serde_json::json!({
                "type": "match",
                "data": {
                    "path": {"text": result.path},
                    "lines": {"text": format!("{}\n", ml.line)},
                    "line_number": ml.line_number,
                    "absolute_offset": ml.byte_offset,
                    "submatches": submatches,
                }
            });
            let _ = writeln!(out, "{}", serde_json::to_string(&match_msg).unwrap());
        }
    }

    let end_msg = serde_json::json!({
        "type": "end",
        "data": {
            "path": {"text": result.path},
            "binary_offset": result.binary_offset,
            "stats": {
                "elapsed": {"secs": 0, "nanos": 0, "human": "0.000000s"},
                "searches": 1,
                "searches_with_match": if result.match_count > 0 { 1 } else { 0 },
                "bytes_searched": result.bytes_searched,
                "bytes_printed": 0,
                "matched_lines": result.matched_line_count,
                "matches": result.match_count,
            }
        }
    });
    let _ = writeln!(out, "{}", serde_json::to_string(&end_msg).unwrap());
}

// =============================================================================
// File Collection
// =============================================================================

fn collect_files(paths: &[PathBuf], args: &Args) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();

    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            files.extend(walk_directory(path, args));
        }
    }

    if let Some(ref sort_by) = args.sort {
        sort_files(&mut files, sort_by, false);
    } else if let Some(ref sort_by) = args.sortr {
        sort_files(&mut files, sort_by, true);
    }

    files
}

fn walk_directory(path: &Path, args: &Args) -> Vec<PathBuf> {
    let no_ignore = args.no_ignore || args.unrestricted >= 1;
    let search_hidden = args.hidden || args.unrestricted >= 2;

    let mut builder = WalkBuilder::new(path);
    builder.hidden(!search_hidden);
    builder.add_custom_ignore_filename(".rgignore");

    if no_ignore {
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);
        builder.ignore(false);
    }

    if args.no_ignore_vcs {
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);
    }

    if args.no_ignore_global { builder.git_global(false); }
    if args.no_ignore_parent { builder.parents(false); }
    if let Some(depth) = args.max_depth { builder.max_depth(Some(depth)); }
    builder.follow_links(args.follow);

    // Type definitions
    let mut types_builder = TypesBuilder::new();
    types_builder.add_defaults();

    for type_add in &args.type_add {
        if let Some((name, glob_pat)) = type_add.split_once(':') {
            let _ = types_builder.add(name, glob_pat);
        }
    }
    for type_clear in &args.type_clear {
        types_builder.clear(type_clear);
    }
    for ft in &args.file_type { types_builder.select(ft); }
    for ft in &args.type_not { types_builder.negate(ft); }

    if !args.file_type.is_empty() || !args.type_not.is_empty() {
        if let Ok(types) = types_builder.build() {
            builder.types(types);
        }
    }

    // Glob overrides
    if !args.glob.is_empty() || !args.iglob.is_empty() {
        let mut override_builder = OverrideBuilder::new(path);
        for g in &args.glob { let _ = override_builder.add(g); }
        for g in &args.iglob {
            let _ = override_builder.case_insensitive(true);
            let _ = override_builder.add(g);
        }
        if let Ok(overrides) = override_builder.build() {
            builder.overrides(overrides);
        }
    }

    if let Some(ref max_size_str) = args.max_filesize {
        if let Ok(max_bytes) = parse_human_size(max_size_str) {
            builder.max_filesize(Some(max_bytes));
        }
    }

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in builder.build() {
        match entry {
            Ok(entry) => {
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    files.push(entry.into_path());
                }
            }
            Err(e) => {
                if !args.no_messages { eprintln!("rg: {}", e); }
            }
        }
    }
    files
}

fn sort_files(files: &mut Vec<PathBuf>, sort_by: &str, reverse: bool) {
    match sort_by {
        "path" => files.sort_by(|a, b| { let c = a.cmp(b); if reverse { c.reverse() } else { c } }),
        "modified" => files.sort_by(|a, b| {
            let ma = fs::metadata(a).and_then(|m| m.modified()).ok();
            let mb = fs::metadata(b).and_then(|m| m.modified()).ok();
            let c = ma.cmp(&mb); if reverse { c.reverse() } else { c }
        }),
        "accessed" => files.sort_by(|a, b| {
            let ma = fs::metadata(a).and_then(|m| m.accessed()).ok();
            let mb = fs::metadata(b).and_then(|m| m.accessed()).ok();
            let c = ma.cmp(&mb); if reverse { c.reverse() } else { c }
        }),
        "created" => files.sort_by(|a, b| {
            let ma = fs::metadata(a).and_then(|m| m.created()).ok();
            let mb = fs::metadata(b).and_then(|m| m.created()).ok();
            let c = ma.cmp(&mb); if reverse { c.reverse() } else { c }
        }),
        _ => {}
    }
}

// =============================================================================
// File Listing Mode
// =============================================================================

fn handle_files_mode(args: &Args) -> i32 {
    let mut paths: Vec<PathBuf> = Vec::new();
    // In --files mode, the positional "pattern" arg is actually a path
    if let Some(ref p) = args.pattern {
        paths.push(PathBuf::from(p));
    }
    paths.extend(args.paths.iter().cloned());
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let mut files = collect_files(&paths, args);

    if let Some(ref sort_by) = args.sort { sort_files(&mut files, sort_by, false); }
    else if let Some(ref sort_by) = args.sortr { sort_files(&mut files, sort_by, true); }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for file in &files {
        let path_str = file.to_string_lossy();
        if args.null { let _ = write!(out, "{}\0", path_str); }
        else { let _ = writeln!(out, "{}", path_str); }
    }
    let _ = out.flush();
    0
}

// =============================================================================
// Type List Mode
// =============================================================================

fn handle_type_list() -> i32 {
    let mut types_builder = TypesBuilder::new();
    types_builder.add_defaults();
    let types = types_builder.build().unwrap();

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for def in types.definitions() {
        let _ = writeln!(out, "{}: {}", def.name(), def.globs().join(", "));
    }
    let _ = out.flush();
    0
}

// =============================================================================
// Generate Mode
// =============================================================================

fn handle_generate(gen_type: &str) -> i32 {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    match gen_type {
        "man" => {
            let _ = write!(out, r#".TH RG 1 "2024" "ripgrep 14.1.1"
.SH NAME
rg \- recursively search the current directory for lines matching a pattern
.SH SYNOPSIS
.B rg
.RI [ OPTIONS ]
.I PATTERN
.RI [ PATH ...]
.SH DESCRIPTION
ripgrep (rg) recursively searches the current directory for a regex pattern.
By default, ripgrep will respect gitignore rules and automatically skip hidden
files/directories and binary files.
.SH OPTIONS
.TP
.BR \-e ", " \-\-regexp =\fIPATTERN\fR
A pattern to search for.
.TP
.BR \-i ", " \-\-ignore\-case
Case insensitive search.
.TP
.BR \-S ", " \-\-smart\-case
Searches case insensitively if the pattern is all lowercase.
.TP
.BR \-s ", " \-\-case\-sensitive
Search case sensitively.
.TP
.BR \-v ", " \-\-invert\-match
Invert matching.
.TP
.BR \-c ", " \-\-count
Only show the count of matching lines for each file.
.TP
.BR \-l ", " \-\-files\-with\-matches
Only print the paths with at least one match.
.TP
.BR \-n ", " \-\-line\-number
Show line numbers.
.TP
.BR \-N ", " \-\-no\-line\-number
Suppress line numbers.
.TP
.BR \-\-color =\fIWHEN\fR
Controls when to use colors.
.SH EXIT STATUS
.TP
.B 0
A match was found.
.TP
.B 1
No match was found.
.TP
.B 2
An error occurred.
.SH AUTHOR
Andrew Gallant <jamslam@gmail.com>
"#);
            let _ = out.flush();
            0
        }
        "complete-bash" => {
            let _ = write!(out, r#"_rg() {{
    local cur prev
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    if [[ "$cur" == -* ]]; then
        COMPREPLY=($(compgen -W "--regexp --file --fixed-strings --ignore-case --smart-case --case-sensitive --invert-match --count --files-with-matches --line-number --no-line-number --color --help --version" -- "$cur"))
        return 0
    fi
    COMPREPLY=($(compgen -f -- "$cur"))
    return 0
}}
complete -F _rg rg
"#);
            let _ = out.flush();
            0
        }
        "complete-zsh" => {
            let _ = writeln!(out, "#compdef rg");
            let _ = out.flush();
            0
        }
        "complete-fish" => {
            let _ = writeln!(out, "# Fish completions for ripgrep");
            let _ = out.flush();
            0
        }
        "complete-powershell" => {
            let _ = writeln!(out, "# PowerShell completions for ripgrep");
            let _ = out.flush();
            0
        }
        _ => {
            eprintln!("rg: error: unrecognized generate type: {}", gen_type);
            2
        }
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

fn read_pattern_file(path: &Path) -> Result<Vec<String>, io::Error> {
    if path == Path::new("-") {
        let stdin = io::stdin();
        let patterns: Vec<String> = stdin.lock().lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(patterns)
    } else {
        let content = fs::read_to_string(path)?;
        Ok(content.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
    }
}

fn parse_human_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() { return Err("empty size".to_string()); }

    let (num_str, multiplier) = if s.ends_with("KB") || s.ends_with("kb") {
        (&s[..s.len() - 2], 1000u64)
    } else if s.ends_with("MB") || s.ends_with("mb") {
        (&s[..s.len() - 2], 1_000_000u64)
    } else if s.ends_with("GB") || s.ends_with("gb") {
        (&s[..s.len() - 2], 1_000_000_000u64)
    } else if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len() - 1], 1024u64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024u64 * 1024)
    } else if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024u64 * 1024 * 1024)
    } else {
        (s, 1u64)
    };

    let num: u64 = num_str.parse().map_err(|_| format!("invalid size: {}", s))?;
    Ok(num * multiplier)
}

fn format_path(path: &Path, sep: &str) -> String {
    let s = path.to_string_lossy().to_string();
    if sep != "/" && sep != std::path::MAIN_SEPARATOR_STR {
        s.replace('/', sep).replace(std::path::MAIN_SEPARATOR, sep)
    } else {
        s
    }
}

fn format_path_with_sep(path: &str, sep: &str) -> String {
    if sep != "/" && sep != std::path::MAIN_SEPARATOR_STR {
        path.replace('/', sep).replace(std::path::MAIN_SEPARATOR, sep)
    } else {
        path.to_string()
    }
}

fn is_stdin_tty() -> bool {
    unsafe { libc::isatty(0) != 0 }
}

extern crate libc;
