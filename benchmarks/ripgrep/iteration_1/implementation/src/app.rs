pub struct FlagDef {
    pub short: Option<char>,
    pub long: &'static str,
    pub doc: &'static str,
    pub takes_value: bool,
}

pub static ALL_FLAGS: &[FlagDef] = &[
    FlagDef { short: Some('e'), long: "regexp", doc: "A pattern to search for.", takes_value: true },
    FlagDef { short: Some('f'), long: "file", doc: "Search for patterns from the given file.", takes_value: true },
    FlagDef { short: Some('i'), long: "ignore-case", doc: "Case insensitive search.", takes_value: false },
    FlagDef { short: Some('s'), long: "case-sensitive", doc: "Case sensitive search.", takes_value: false },
    FlagDef { short: Some('S'), long: "smart-case", doc: "Smart case search (default).", takes_value: false },
    FlagDef { short: Some('F'), long: "fixed-strings", doc: "Treat the pattern as a literal string.", takes_value: false },
    FlagDef { short: Some('w'), long: "word-regexp", doc: "Only show matches surrounded by word boundaries.", takes_value: false },
    FlagDef { short: Some('x'), long: "line-regexp", doc: "Only show matches surrounded by line boundaries.", takes_value: false },
    FlagDef { short: Some('v'), long: "invert-match", doc: "Invert matching.", takes_value: false },
    FlagDef { short: Some('c'), long: "count", doc: "Show count of matching lines for each file.", takes_value: false },
    FlagDef { short: None, long: "count-matches", doc: "Show count of individual matches for each file.", takes_value: false },
    FlagDef { short: Some('l'), long: "files-with-matches", doc: "Print the paths with at least one match.", takes_value: false },
    FlagDef { short: None, long: "files-without-match", doc: "Print the paths that contain zero matches.", takes_value: false },
    FlagDef { short: Some('o'), long: "only-matching", doc: "Print only matched parts of a matching line.", takes_value: false },
    FlagDef { short: Some('r'), long: "replace", doc: "Replace every match with the text given.", takes_value: true },
    FlagDef { short: Some('n'), long: "line-number", doc: "Show line numbers (default for terminal).", takes_value: false },
    FlagDef { short: Some('N'), long: "no-line-number", doc: "Suppress line numbers.", takes_value: false },
    FlagDef { short: None, long: "column", doc: "Show column numbers.", takes_value: false },
    FlagDef { short: Some('H'), long: "with-filename", doc: "Print the file path with output.", takes_value: false },
    FlagDef { short: None, long: "no-filename", doc: "Never print the file path.", takes_value: false },
    FlagDef { short: Some('b'), long: "byte-offset", doc: "Print the 0-based byte offset for each matching line.", takes_value: false },
    FlagDef { short: None, long: "heading", doc: "Print file paths as headings.", takes_value: false },
    FlagDef { short: None, long: "no-heading", doc: "Don't group matches by file.", takes_value: false },
    FlagDef { short: Some('p'), long: "pretty", doc: "Alias for --color always --heading --line-number.", takes_value: false },
    FlagDef { short: None, long: "vimgrep", doc: "Print results in a format compatible with Vim.", takes_value: false },
    FlagDef { short: None, long: "files", doc: "Print files that would be searched.", takes_value: false },
    FlagDef { short: None, long: "stats", doc: "Print aggregate statistics.", takes_value: false },
    FlagDef { short: None, long: "json", doc: "Print results in JSON Lines format.", takes_value: false },
    FlagDef { short: Some('q'), long: "quiet", doc: "Do not print anything to stdout.", takes_value: false },
    FlagDef { short: Some('A'), long: "after-context", doc: "Show NUM lines after each match.", takes_value: true },
    FlagDef { short: Some('B'), long: "before-context", doc: "Show NUM lines before each match.", takes_value: true },
    FlagDef { short: Some('C'), long: "context", doc: "Show NUM lines before and after each match.", takes_value: true },
    FlagDef { short: None, long: "context-separator", doc: "Set the context separator string.", takes_value: true },
    FlagDef { short: None, long: "no-context-separator", doc: "Don't print context separators.", takes_value: false },
    FlagDef { short: Some('g'), long: "glob", doc: "Include/exclude files matching the given glob.", takes_value: true },
    FlagDef { short: None, long: "iglob", doc: "Like --glob, but case-insensitive.", takes_value: true },
    FlagDef { short: Some('t'), long: "type", doc: "Only search files matching TYPE.", takes_value: true },
    FlagDef { short: Some('T'), long: "type-not", doc: "Do not search files matching TYPE.", takes_value: true },
    FlagDef { short: None, long: "type-add", doc: "Add a new file type definition.", takes_value: true },
    FlagDef { short: None, long: "type-clear", doc: "Clear globs for a file type.", takes_value: true },
    FlagDef { short: None, long: "type-list", doc: "Show all supported file types.", takes_value: false },
    FlagDef { short: Some('u'), long: "unrestricted", doc: "Reduce filtering. -u: no gitignore. -uu: no hidden. -uuu: no binary.", takes_value: false },
    FlagDef { short: None, long: "hidden", doc: "Search hidden files and directories.", takes_value: false },
    FlagDef { short: None, long: "no-hidden", doc: "Don't search hidden files (default).", takes_value: false },
    FlagDef { short: Some('L'), long: "follow", doc: "Follow symbolic links.", takes_value: false },
    FlagDef { short: None, long: "no-ignore", doc: "Don't respect ignore files.", takes_value: false },
    FlagDef { short: None, long: "no-ignore-vcs", doc: "Don't respect VCS ignore files.", takes_value: false },
    FlagDef { short: None, long: "no-ignore-global", doc: "Don't respect global gitignore.", takes_value: false },
    FlagDef { short: None, long: "no-ignore-parent", doc: "Don't respect ignore files in parent dirs.", takes_value: false },
    FlagDef { short: Some('d'), long: "max-depth", doc: "Descend at most NUM directories.", takes_value: true },
    FlagDef { short: None, long: "max-filesize", doc: "Ignore files above the specified size.", takes_value: true },
    FlagDef { short: None, long: "one-file-system", doc: "Don't cross file system boundaries.", takes_value: false },
    FlagDef { short: None, long: "binary", doc: "Search binary files.", takes_value: false },
    FlagDef { short: Some('a'), long: "text", doc: "Search binary files as if they were text.", takes_value: false },
    FlagDef { short: None, long: "engine", doc: "Select the regex engine.", takes_value: true },
    FlagDef { short: Some('P'), long: "pcre2", doc: "Use the PCRE2 regex engine.", takes_value: false },
    FlagDef { short: None, long: "no-pcre2", doc: "Use the default regex engine.", takes_value: false },
    FlagDef { short: None, long: "regex-size-limit", doc: "Set the upper size limit of the compiled regex.", takes_value: true },
    FlagDef { short: None, long: "dfa-size-limit", doc: "Set the upper size limit of the regex DFA.", takes_value: true },
    FlagDef { short: None, long: "no-unicode", doc: "Disable Unicode mode.", takes_value: false },
    FlagDef { short: None, long: "crlf", doc: "Use CRLF line terminators.", takes_value: false },
    FlagDef { short: None, long: "null-data", doc: "Use NUL as line terminator.", takes_value: false },
    FlagDef { short: Some('m'), long: "max-count", doc: "Limit the number of matching lines per file.", takes_value: true },
    FlagDef { short: None, long: "max-columns", doc: "Don't print lines longer than this limit.", takes_value: true },
    FlagDef { short: None, long: "max-columns-preview", doc: "Show preview for lines exceeding the max column limit.", takes_value: false },
    FlagDef { short: None, long: "mmap", doc: "Search using memory maps when possible.", takes_value: false },
    FlagDef { short: None, long: "no-mmap", doc: "Never use memory maps.", takes_value: false },
    FlagDef { short: Some('j'), long: "threads", doc: "The approximate number of threads to use.", takes_value: true },
    FlagDef { short: None, long: "sort", doc: "Sort results in ascending order.", takes_value: true },
    FlagDef { short: None, long: "sortr", doc: "Sort results in descending order.", takes_value: true },
    FlagDef { short: Some('z'), long: "search-zip", doc: "Search in compressed files.", takes_value: false },
    FlagDef { short: None, long: "pre", doc: "Run a preprocessor on each file.", takes_value: true },
    FlagDef { short: None, long: "pre-glob", doc: "Only run the preprocessor on matching files.", takes_value: true },
    FlagDef { short: None, long: "stop-on-nonmatch", doc: "Stop after the first non-matching line after a match.", takes_value: false },
    FlagDef { short: None, long: "color", doc: "Controls when to use color.", takes_value: true },
    FlagDef { short: None, long: "colors", doc: "Configure color settings.", takes_value: true },
    FlagDef { short: None, long: "hyperlink-format", doc: "Set the format of hyperlinks.", takes_value: true },
    FlagDef { short: None, long: "path-separator", doc: "Set the path separator.", takes_value: true },
    FlagDef { short: Some('0'), long: "null", doc: "Print NUL byte after file paths.", takes_value: false },
    FlagDef { short: None, long: "no-messages", doc: "Suppress error messages.", takes_value: false },
    FlagDef { short: None, long: "trim", doc: "Trim ASCII whitespace prefix from each line.", takes_value: false },
    FlagDef { short: None, long: "field-match-separator", doc: "Set the field separator for matches.", takes_value: true },
    FlagDef { short: None, long: "field-context-separator", doc: "Set the field separator for context lines.", takes_value: true },
    FlagDef { short: None, long: "debug", doc: "Show debug messages.", takes_value: false },
    FlagDef { short: None, long: "trace", doc: "Show trace messages.", takes_value: false },
    FlagDef { short: Some('E'), long: "encoding", doc: "Specify the text encoding of files to search.", takes_value: true },
    FlagDef { short: Some('U'), long: "multiline", doc: "Enable matching across multiple lines.", takes_value: false },
    FlagDef { short: None, long: "multiline-dotall", doc: "Make '.' match new lines when multiline is enabled.", takes_value: false },
    FlagDef { short: None, long: "passthru", doc: "Print all lines in a file.", takes_value: false },
    FlagDef { short: None, long: "generate", doc: "Generate man pages or shell completions.", takes_value: true },
    FlagDef { short: Some('h'), long: "help", doc: "Prints help information.", takes_value: false },
    FlagDef { short: Some('V'), long: "version", doc: "Prints version information.", takes_value: false },
];

pub fn generate_help() -> String {
    let mut help = String::new();
    help.push_str("ripgrep (rg) - recursively search for a regex pattern\n\n");
    help.push_str("USAGE:\n");
    help.push_str("    rg [OPTIONS] PATTERN [PATH ...]\n");
    help.push_str("    rg [OPTIONS] -e PATTERN ... [PATH ...]\n");
    help.push_str("    rg [OPTIONS] -f PATTERNFILE ... [PATH ...]\n");
    help.push_str("    rg [OPTIONS] --files [PATH ...]\n");
    help.push_str("    rg [OPTIONS] --type-list\n");
    help.push_str("    command | rg [OPTIONS] PATTERN\n\n");
    help.push_str("ARGS:\n");
    help.push_str("    <PATTERN>    A regular expression used for searching.\n");
    help.push_str("    <PATH>...    A file or directory to search.\n\n");
    help.push_str("OPTIONS:\n");
    for flag in ALL_FLAGS {
        if let Some(short) = flag.short {
            help.push_str(&format!("    -{}, --{:<28} {}\n", short, flag.long, flag.doc));
        } else {
            help.push_str(&format!("        --{:<28} {}\n", flag.long, flag.doc));
        }
    }
    help
}
