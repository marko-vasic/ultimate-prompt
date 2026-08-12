use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use grep_cli::is_tty_stdout;
use grep_matcher::LineTerminator;
use grep_printer::{ColorSpecs, HyperlinkFormat};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Encoding, MmapChoice, Searcher, SearcherBuilder};
use ignore::dir::IgnoreBuilder;
use ignore::overrides::OverrideBuilder;
use ignore::types::TypesBuilder;
use ignore::walk::{Walk, WalkParallel};

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Search,
    Files,
    Types,
    Version,
    Help,
    Generate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    pub fn to_termcolor(self) -> termcolor::ColorChoice {
        match self {
            ColorChoice::Auto => {
                if is_tty_stdout() {
                    termcolor::ColorChoice::Auto
                } else {
                    termcolor::ColorChoice::Never
                }
            }
            ColorChoice::Always => termcolor::ColorChoice::Always,
            ColorChoice::Never => termcolor::ColorChoice::Never,
        }
    }
}

#[derive(Debug)]
pub struct Args {
    pub mode: Mode,
    pub patterns: Vec<String>,
    pub paths: Vec<PathBuf>,

    pub case_insensitive: bool,
    pub case_sensitive: bool,
    pub smart_case: bool,
    pub fixed_strings: bool,
    pub word_regexp: bool,
    pub line_regexp: bool,
    pub invert_match: bool,
    pub multiline: bool,
    pub multiline_dotall: bool,

    pub count: bool,
    pub count_matches: bool,
    pub files_with_matches: bool,
    pub files_without_match: bool,
    pub only_matching: bool,
    pub replacement: Option<Vec<u8>>,
    pub line_number: Option<bool>,
    pub column: bool,
    pub with_filename: Option<bool>,
    pub byte_offset: bool,
    pub heading: Option<bool>,
    pub pretty: bool,
    pub vimgrep: bool,
    pub json: bool,
    pub quiet: bool,
    pub stats: bool,

    pub after_context: usize,
    pub before_context: usize,
    pub context_separator: Option<Vec<u8>>,
    pub no_context_separator: bool,

    pub globs: Vec<String>,
    pub iglobs: Vec<String>,
    pub type_filters: Vec<(bool, String)>,
    pub type_adds: Vec<String>,
    pub type_clears: Vec<String>,
    pub unrestricted: u8,
    pub hidden: bool,
    pub follow_links: bool,
    pub no_ignore: bool,
    pub no_ignore_vcs: bool,
    pub no_ignore_global: bool,
    pub no_ignore_parent: bool,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
    pub one_file_system: bool,

    pub binary: bool,
    pub text: bool,

    pub unicode: bool,
    pub crlf: bool,
    pub null_data: bool,
    pub regex_size_limit: Option<usize>,
    pub dfa_size_limit: Option<usize>,

    pub max_count: Option<u64>,
    pub max_columns: Option<u64>,
    pub max_columns_preview: bool,
    pub mmap: Option<bool>,
    pub threads: Option<usize>,
    pub sort: Option<String>,
    pub sort_reverse: Option<String>,
    pub search_zip: bool,
    pub pre: Option<String>,
    pub pre_globs: Vec<String>,
    pub stop_on_nonmatch: bool,
    pub passthru: bool,

    pub color: ColorChoice,
    pub color_specs: Vec<String>,
    pub hyperlink_format: Option<String>,
    pub path_separator: Option<u8>,
    pub null_path: bool,
    pub no_messages: bool,
    pub trim: bool,
    pub field_match_separator: Option<Vec<u8>>,
    pub field_context_separator: Option<Vec<u8>>,

    pub debug: bool,
    pub trace: bool,
    pub encoding: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            mode: Mode::Search,
            patterns: Vec::new(),
            paths: Vec::new(),
            case_insensitive: false,
            case_sensitive: false,
            smart_case: true,
            fixed_strings: false,
            word_regexp: false,
            line_regexp: false,
            invert_match: false,
            multiline: false,
            multiline_dotall: false,
            count: false,
            count_matches: false,
            files_with_matches: false,
            files_without_match: false,
            only_matching: false,
            replacement: None,
            line_number: None,
            column: false,
            with_filename: None,
            byte_offset: false,
            heading: None,
            pretty: false,
            vimgrep: false,
            json: false,
            quiet: false,
            stats: false,
            after_context: 0,
            before_context: 0,
            context_separator: None,
            no_context_separator: false,
            globs: Vec::new(),
            iglobs: Vec::new(),
            type_filters: Vec::new(),
            type_adds: Vec::new(),
            type_clears: Vec::new(),
            unrestricted: 0,
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
            text: false,
            unicode: true,
            crlf: false,
            null_data: false,
            regex_size_limit: None,
            dfa_size_limit: None,
            max_count: None,
            max_columns: None,
            max_columns_preview: false,
            mmap: None,
            threads: None,
            sort: None,
            sort_reverse: None,
            search_zip: false,
            pre: None,
            pre_globs: Vec::new(),
            stop_on_nonmatch: false,
            passthru: false,
            color: ColorChoice::Auto,
            color_specs: Vec::new(),
            hyperlink_format: None,
            path_separator: None,
            null_path: false,
            no_messages: false,
            trim: false,
            field_match_separator: None,
            field_context_separator: None,
            debug: false,
            trace: false,
            encoding: None,
        }
    }
}

impl Args {
    pub fn parse() -> Result<Self> {
        let mut args = Args::default();

        let mut all_args: Vec<OsString> = Vec::new();
        if let Ok(config_path) = env::var("RIPGREP_CONFIG_PATH") {
            if let Ok(config_args) = read_config_file(&config_path) {
                all_args.extend(config_args.into_iter().map(OsString::from));
            }
        }

        all_args.extend(env::args_os().skip(1));

        let mut parser = lexopt::Parser::from_args(all_args);

        while let Some(arg) = parser.next()? {
            use lexopt::prelude::*;
            match arg {
                Short('h') | Long("help") => {
                    args.mode = Mode::Help;
                    return Ok(args);
                }
                Short('V') | Long("version") => {
                    args.mode = Mode::Version;
                    return Ok(args);
                }
                Long("type-list") => {
                    args.mode = Mode::Types;
                    return Ok(args);
                }
                Long("generate") => {
                    let val: String = parser.value()?.parse()?;
                    args.mode = Mode::Generate(val);
                    return Ok(args);
                }
                Long("files") => {
                    args.mode = Mode::Files;
                }
                Short('e') | Long("regexp") => {
                    let val: String = parser.value()?.parse()?;
                    args.patterns.push(val);
                }
                Short('f') | Long("file") => {
                    let path_str: String = parser.value()?.parse()?;
                    if path_str == "-" {
                        let pats = grep_cli::patterns_from_stdin()?;
                        args.patterns.extend(pats);
                    } else {
                        let pats = grep_cli::patterns_from_path(Path::new(&path_str))?;
                        args.patterns.extend(pats);
                    }
                }
                Short('i') | Long("ignore-case") => {
                    args.case_insensitive = true;
                    args.case_sensitive = false;
                    args.smart_case = false;
                }
                Short('s') | Long("case-sensitive") => {
                    args.case_sensitive = true;
                    args.case_insensitive = false;
                    args.smart_case = false;
                }
                Short('S') | Long("smart-case") => {
                    args.smart_case = true;
                    args.case_insensitive = false;
                    args.case_sensitive = false;
                }
                Short('F') | Long("fixed-strings") => {
                    args.fixed_strings = true;
                }
                Short('w') | Long("word-regexp") => {
                    args.word_regexp = true;
                }
                Short('x') | Long("line-regexp") => {
                    args.line_regexp = true;
                }
                Short('v') | Long("invert-match") => {
                    args.invert_match = true;
                }
                Short('c') | Long("count") => {
                    args.count = true;
                }
                Long("count-matches") => {
                    args.count_matches = true;
                }
                Short('l') | Long("files-with-matches") => {
                    args.files_with_matches = true;
                }
                Long("files-without-match") => {
                    args.files_without_match = true;
                }
                Short('o') | Long("only-matching") => {
                    args.only_matching = true;
                }
                Short('r') | Long("replace") => {
                    let val: String = parser.value()?.parse()?;
                    args.replacement = Some(val.into_bytes());
                }
                Short('n') | Long("line-number") => {
                    args.line_number = Some(true);
                }
                Short('N') | Long("no-line-number") => {
                    args.line_number = Some(false);
                }
                Long("column") => {
                    args.column = true;
                }
                Short('H') | Long("with-filename") => {
                    args.with_filename = Some(true);
                }
                Long("no-filename") => {
                    args.with_filename = Some(false);
                }
                Short('b') | Long("byte-offset") => {
                    args.byte_offset = true;
                }
                Long("heading") => {
                    args.heading = Some(true);
                }
                Long("no-heading") => {
                    args.heading = Some(false);
                }
                Short('p') | Long("pretty") => {
                    args.pretty = true;
                    args.color = ColorChoice::Always;
                    args.heading = Some(true);
                    args.line_number = Some(true);
                }
                Long("vimgrep") => {
                    args.vimgrep = true;
                    args.line_number = Some(true);
                    args.column = true;
                    args.with_filename = Some(true);
                    args.heading = Some(false);
                }
                Long("json") => {
                    args.json = true;
                }
                Short('q') | Long("quiet") => {
                    args.quiet = true;
                }
                Long("stats") => {
                    args.stats = true;
                }
                Short('A') | Long("after-context") => {
                    let val: usize = parser.value()?.parse()?;
                    args.after_context = val;
                }
                Short('B') | Long("before-context") => {
                    let val: usize = parser.value()?.parse()?;
                    args.before_context = val;
                }
                Short('C') | Long("context") => {
                    let val: usize = parser.value()?.parse()?;
                    args.after_context = val;
                    args.before_context = val;
                }
                Long("context-separator") => {
                    let val: String = parser.value()?.parse()?;
                    args.context_separator = Some(val.into_bytes());
                }
                Long("no-context-separator") => {
                    args.no_context_separator = true;
                }
                Short('g') | Long("glob") => {
                    let val: String = parser.value()?.parse()?;
                    args.globs.push(val);
                }
                Long("iglob") => {
                    let val: String = parser.value()?.parse()?;
                    args.iglobs.push(val);
                }
                Short('t') | Long("type") => {
                    let val: String = parser.value()?.parse()?;
                    args.type_filters.push((true, val));
                }
                Short('T') | Long("type-not") => {
                    let val: String = parser.value()?.parse()?;
                    args.type_filters.push((false, val));
                }
                Long("type-add") => {
                    let val: String = parser.value()?.parse()?;
                    args.type_adds.push(val);
                }
                Long("type-clear") => {
                    let val: String = parser.value()?.parse()?;
                    args.type_clears.push(val);
                }
                Short('u') | Long("unrestricted") => {
                    args.unrestricted += 1;
                }
                Long("hidden") => {
                    args.hidden = true;
                }
                Long("no-hidden") => {
                    args.hidden = false;
                }
                Short('L') | Long("follow") => {
                    args.follow_links = true;
                }
                Long("no-ignore") => {
                    args.no_ignore = true;
                }
                Long("no-ignore-vcs") => {
                    args.no_ignore_vcs = true;
                }
                Long("no-ignore-global") => {
                    args.no_ignore_global = true;
                }
                Long("no-ignore-parent") => {
                    args.no_ignore_parent = true;
                }
                Short('d') | Long("max-depth") => {
                    let val: usize = parser.value()?.parse()?;
                    args.max_depth = Some(val);
                }
                Long("max-filesize") => {
                    let val: String = parser.value()?.parse()?;
                    let bytes = grep_cli::parse_human_readable_size(&val)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    args.max_filesize = Some(bytes);
                }
                Long("one-file-system") => {
                    args.one_file_system = true;
                }
                Long("binary") => {
                    args.binary = true;
                }
                Short('a') | Long("text") => {
                    args.text = true;
                }
                Long("no-unicode") => {
                    args.unicode = false;
                }
                Long("crlf") => {
                    args.crlf = true;
                }
                Long("null-data") => {
                    args.null_data = true;
                }
                Long("regex-size-limit") => {
                    let val: String = parser.value()?.parse()?;
                    let bytes = grep_cli::parse_human_readable_size(&val)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    args.regex_size_limit = Some(bytes as usize);
                }
                Long("dfa-size-limit") => {
                    let val: String = parser.value()?.parse()?;
                    let bytes = grep_cli::parse_human_readable_size(&val)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    args.dfa_size_limit = Some(bytes as usize);
                }
                Short('m') | Long("max-count") => {
                    let val: u64 = parser.value()?.parse()?;
                    args.max_count = Some(val);
                }
                Long("max-columns") => {
                    let val: u64 = parser.value()?.parse()?;
                    args.max_columns = Some(val);
                }
                Long("max-columns-preview") => {
                    args.max_columns_preview = true;
                }
                Long("mmap") => {
                    args.mmap = Some(true);
                }
                Long("no-mmap") => {
                    args.mmap = Some(false);
                }
                Short('j') | Long("threads") => {
                    let val: usize = parser.value()?.parse()?;
                    args.threads = Some(val);
                }
                Long("sort") => {
                    let val: String = parser.value()?.parse()?;
                    args.sort = Some(val);
                }
                Long("sortr") => {
                    let val: String = parser.value()?.parse()?;
                    args.sort_reverse = Some(val);
                }
                Short('z') | Long("search-zip") => {
                    args.search_zip = true;
                }
                Long("pre") => {
                    let val: String = parser.value()?.parse()?;
                    args.pre = Some(val);
                }
                Long("pre-glob") => {
                    let val: String = parser.value()?.parse()?;
                    args.pre_globs.push(val);
                }
                Long("stop-on-nonmatch") => {
                    args.stop_on_nonmatch = true;
                }
                Long("passthru") => {
                    args.passthru = true;
                }
                Long("color") => {
                    let val: String = parser.value()?.parse()?;
                    match val.to_lowercase().as_str() {
                        "always" => args.color = ColorChoice::Always,
                        "never" => args.color = ColorChoice::Never,
                        "auto" => args.color = ColorChoice::Auto,
                        _ => bail!("invalid color choice: {}", val),
                    }
                }
                Long("colors") => {
                    let val: String = parser.value()?.parse()?;
                    args.color_specs.push(val);
                }
                Long("hyperlink-format") => {
                    let val: String = parser.value()?.parse()?;
                    args.hyperlink_format = Some(val);
                }
                Long("path-separator") => {
                    let val: String = parser.value()?.parse()?;
                    if let Some(ch) = val.bytes().next() {
                        args.path_separator = Some(ch);
                    }
                }
                Short('0') | Long("null") => {
                    args.null_path = true;
                }
                Long("no-messages") => {
                    args.no_messages = true;
                }
                Long("trim") => {
                    args.trim = true;
                }
                Long("field-match-separator") => {
                    let val: String = parser.value()?.parse()?;
                    args.field_match_separator = Some(val.into_bytes());
                }
                Long("field-context-separator") => {
                    let val: String = parser.value()?.parse()?;
                    args.field_context_separator = Some(val.into_bytes());
                }
                Long("debug") => {
                    args.debug = true;
                }
                Long("trace") => {
                    args.trace = true;
                }
                Short('E') | Long("encoding") => {
                    let val: String = parser.value()?.parse()?;
                    args.encoding = Some(val);
                }
                Short('U') | Long("multiline") => {
                    args.multiline = true;
                }
                Long("multiline-dotall") => {
                    args.multiline_dotall = true;
                }
                Value(val) => {
                    let val_str = val.to_string_lossy().into_owned();
                    if args.mode == Mode::Search && args.patterns.is_empty() {
                        args.patterns.push(val_str);
                    } else {
                        args.paths.push(PathBuf::from(val_str));
                    }
                }
                _ => bail!("unrecognized argument"),
            }
        }

        if args.mode == Mode::Search && args.patterns.is_empty() {
            bail!("No pattern specified. Run with --help for usage.");
        }

        if args.paths.is_empty() {
            args.paths.push(PathBuf::from("."));
        }

        Ok(args)
    }

    pub fn matcher(&self) -> Result<RegexMatcher> {
        let mut builder = RegexMatcherBuilder::new();
        builder
            .case_smart(self.smart_case)
            .fixed_strings(self.fixed_strings)
            .word(self.word_regexp)
            .whole_line(self.line_regexp)
            .multi_line(self.multiline)
            .dot_all(self.multiline_dotall)
            .unicode(self.unicode)
            .crlf(self.crlf);

        if self.case_insensitive {
            builder.case_insensitive(true);
        } else if self.case_sensitive {
            builder.case_insensitive(false);
        }

        if self.null_data {
            builder.line_terminator(Some(LineTerminator::Byte(b'\0')));
        }

        if let Some(limit) = self.regex_size_limit {
            builder.regex_size_limit(limit);
        }
        if let Some(limit) = self.dfa_size_limit {
            builder.dfa_size_limit(limit);
        }

        let refs: Vec<&str> = self.patterns.iter().map(|s| s.as_str()).collect();
        let matcher = builder
            .build_many(&refs)
            .context("failed to compile regex pattern")?;
        Ok(matcher)
    }

    pub fn searcher(&self) -> Searcher {
        let mut builder = SearcherBuilder::new();
        builder
            .invert_match(self.invert_match)
            .before_context(self.before_context)
            .after_context(self.after_context)
            .passthru(self.passthru)
            .stop_on_nonmatch(self.stop_on_nonmatch);

        if self.null_data {
            builder.line_terminator(LineTerminator::Byte(b'\0'));
        }

        if let Some(n) = self.line_number {
            builder.line_number(n);
        }

        if self.text {
            builder.binary_detection(BinaryDetection::None);
        } else if self.binary {
            builder.binary_detection(BinaryDetection::Convert);
        } else {
            builder.binary_detection(BinaryDetection::Quit);
        }

        if let Some(mmap_flag) = self.mmap {
            if mmap_flag {
                builder.memory_map(MmapChoice::Always);
            } else {
                builder.memory_map(MmapChoice::Never);
            }
        } else {
            builder.memory_map(MmapChoice::Auto);
        }

        builder.build()
    }

    pub fn walker(&self) -> Walk {
        self.ignore_builder().build()
    }

    pub fn walker_parallel(&self) -> WalkParallel {
        let mut builder = self.ignore_builder();
        if let Some(threads) = self.threads {
            builder.threads(threads);
        }
        builder.build_parallel()
    }

    fn ignore_builder(&self) -> IgnoreBuilder {
        let root = &self.paths[0];
        let mut builder = IgnoreBuilder::new();

        if self.unrestricted >= 1 || self.no_ignore {
            builder.git_ignore(false);
            builder.ignore_file(false);
            builder.rg_ignore_file(false);
        }
        if self.unrestricted >= 2 || self.hidden {
            builder.hidden(false);
        }
        if self.no_ignore_vcs {
            builder.git_ignore(false);
        }
        if self.no_ignore_global {
            builder.git_global(false);
        }
        if self.no_ignore_parent {
            builder.parents(false);
        }

        if self.follow_links {
            builder.follow_links(true);
        }
        if self.one_file_system {
            builder.same_file_system(true);
        }
        if let Some(depth) = self.max_depth {
            builder.max_depth(Some(depth));
        }
        if let Some(size) = self.max_filesize {
            builder.max_filesize(Some(size));
        }

        let mut override_builder = OverrideBuilder::new(root);
        for g in &self.globs {
            let _ = override_builder.add(g);
        }
        for g in &self.iglobs {
            let _ = override_builder.case_insensitive(true).add(g);
        }
        if let Ok(ov) = override_builder.build() {
            builder.overrides(ov);
        }

        let mut types_builder = TypesBuilder::new();
        types_builder.add_defaults();
        for spec in &self.type_adds {
            if let Some((name, glob)) = spec.split_once(':') {
                let _ = types_builder.add(name, glob);
            }
        }
        for name in &self.type_clears {
            types_builder.clear(name);
        }
        for (select, name) in &self.type_filters {
            if *select {
                types_builder.select(name);
            } else {
                types_builder.negate(name);
            }
        }
        if let Ok(types) = types_builder.build() {
            builder.types(types);
        }

        for path in &self.paths {
            builder.add_path(path);
        }

        builder
    }
}

fn read_config_file(path: &str) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut args = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        args.push(trimmed.to_string());
    }

    Ok(args)
}
