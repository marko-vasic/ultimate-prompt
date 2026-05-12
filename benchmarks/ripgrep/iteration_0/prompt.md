# Ultimate Prompt v0 — ripgrep

Build **ripgrep** (`rg`), a line-oriented search tool that recursively searches directories for a regex pattern. It is extremely fast, Unicode-aware, and respects `.gitignore` rules by default. The implementation must be in **Rust**, using **Cargo** as the build system.

The binary name is `rg`. It should be a single compiled binary with no runtime dependencies (except optional PCRE2).

---

## 1. Project Overview

ripgrep is a command-line search tool similar to `grep`, but designed for speed, ergonomics, and intelligent default behavior. Key differentiators from traditional grep:

1. **Respects ignore files**: By default, respects `.gitignore`, `.ignore`, and `.rgignore` files at every directory level, plus the global git ignore file. This means it automatically skips files that version control ignores.
2. **Skips hidden files and directories**: Files/dirs starting with `.` are skipped by default.
3. **Skips binary files**: Files detected as binary (containing NUL bytes) are skipped, with a brief notification.
4. **Smart recursive search**: When given a directory, searches it recursively. When no path is specified, searches the current directory.
5. **Extremely fast**: Uses memory maps when appropriate, parallelizes directory traversal and searching across threads, and leverages literal optimizations in the regex engine.
6. **Unicode-aware**: Patterns match Unicode by default (e.g., `\w` matches Unicode word characters, `.` matches any Unicode scalar value).
7. **First-class support** on Linux, macOS, and Windows.

### Usage Patterns

```
rg [OPTIONS] PATTERN [PATH ...]
rg [OPTIONS] -e PATTERN ... [PATH ...]
rg [OPTIONS] -f PATTERNFILE ... [PATH ...]
rg [OPTIONS] --files [PATH ...]
rg [OPTIONS] --type-list
command | rg [OPTIONS] PATTERN
```

### Exit Codes

- `0`: At least one match was found, and no errors occurred.
- `1`: No matches were found.
- `2`: An error occurred (parse error, I/O error, etc.). Some errors (e.g., permission denied on one file during recursive search) are non-fatal: they are reported to stderr but do not cause immediate termination.

Broken pipe errors (e.g., `rg pattern | head`) should be treated as graceful termination with exit code 0.

---

## 2. Architecture

The project should be structured as a **Cargo workspace** with a modular design that cleanly separates concerns. The individual library components should be independently usable outside of the main binary. At a minimum, the following logical concerns should be separated into distinct modules or crates:

### 2.1 Logical Separation of Concerns

| Concern | Responsibility |
|---------|---------------|
| **Regex engine abstraction** | Define an abstract interface for regex matching to allow swapping between multiple regex engines (e.g., Rust's `regex` crate and PCRE2). The interface should support finding matches, capture groups, and configuring line terminators. |
| **Regex engine — default** | Implement the regex abstraction using Rust's `regex-automata` crate with optimizations like literal extraction, prefix stripping, and detection of patterns incompatible with line-oriented search. |
| **Regex engine — PCRE2** (optional) | Implement the regex abstraction using the PCRE2 C library, providing look-arounds and backreferences. This should be behind an optional feature flag. |
| **Search orchestration** | Read bytes from a source (file, stdin, memory map), apply the regex engine, track line numbers, manage context windows (before/after context), handle binary detection, multi-line mode, and encoding conversion. Report results to an output consumer via a callback/sink pattern. |
| **Output formatting** | Consume search results and format them for display. Support three output modes: standard grep-like format, summary/count format, and JSON Lines format. |
| **CLI utilities** | Terminal output buffering (line-buffered for ttys, block-buffered for pipes), decompression stream wrappers, byte escape/unescape, pattern file reading, hostname detection, and human-readable size parsing. |
| **Glob matching** | Cross-platform glob pattern matching. Support matching multiple glob patterns simultaneously against a single candidate path efficiently. |
| **Directory traversal** | Fast recursive directory iterator that respects layered ignore rules (`.gitignore`, `.ignore`, `.rgignore`, global gitignore). Support parallel traversal. Handle file type definitions and glob overrides. |
| **Binary (CLI)** | The main binary entry point. Argument parsing, config file handling, orchestration of the search pipeline, and dispatch to different modes (search, file listing, type listing, generation). |

### 2.2 Key Architectural Patterns

- **Abstract regex interface**: Define a trait-based abstraction for regex engines so that the search and output layers are decoupled from any specific regex implementation. This allows supporting both the default Rust regex engine and PCRE2 with the same search pipeline. Use a callback-driven (push/internal iteration) model for match iteration rather than an external iterator — this accommodates regex engines that natively require internal iteration.

- **Sink/callback pattern for output**: Decouple the search loop from output formatting by having the searcher report events (match found, context line, begin/end file) to a consumer via a trait/callback interface. Output formatters implement this interface.

- **Deferred validation**: Structure argument parsing so that "special modes" (like `--help`, `--version`) can succeed even when the environment is invalid (e.g., missing current directory). This typically means separating raw flag parsing from the expensive validation/compilation step.

### 2.3 Concurrency Requirements

- In single-threaded mode, output should stream directly to stdout.
- In multi-threaded mode, output from different files must never be interleaved. Each file's results must be printed atomically.
- The parallelism model should combine directory traversal and file searching in the same thread pool.
- File listing (`--files`) in parallel mode should be optimized to minimize memory overhead for small per-file output.

---

## 3. CLI Flags & Behavior

ripgrep has an extensive CLI. Below are the major flag categories and their behavioral specifications. Implement all of these.

### 3.1 Pattern Specification

| Flag | Behavior |
|------|----------|
| `PATTERN` (positional) | The regex pattern to search for. |
| `-e PATTERN` / `--regexp=PATTERN` | Specify one or more patterns. Lines matching any pattern are printed. |
| `-f FILE` / `--file=FILE` | Read patterns from a file (one per line). `-` reads from stdin. |
| `-F` / `--fixed-strings` | Treat patterns as literal strings, not regexes. |
| `-w` / `--word-regexp` | Surround each pattern with `\b...\b`. |
| `-x` / `--line-regexp` | Surround each pattern with `^...$` (match entire line). |
| `-i` / `--ignore-case` | Case-insensitive search. |
| `-S` / `--smart-case` | Case-insensitive if pattern is all lowercase; case-sensitive otherwise. This is the default. |
| `-s` / `--case-sensitive` | Case-sensitive search (override smart-case). |
| `-v` / `--invert-match` | Print non-matching lines. |
| `-U` / `--multiline` | Allow patterns to match across line boundaries. Enables `.` to match `\n` with `--multiline-dotall`. |

### 3.2 Output Control

| Flag | Behavior |
|------|----------|
| `-c` / `--count` | Print only the count of matching lines per file. |
| `--count-matches` | Print count of individual matches (not matching lines). |
| `-l` / `--files-with-matches` | Print only filenames containing matches. |
| `--files-without-match` | Print only filenames with no matches. |
| `-o` / `--only-matching` | Print only the matched parts of a line, each on its own line. |
| `-r REPLACEMENT` / `--replace=REPLACEMENT` | Replace matches with the given string. Supports capture group references (`$1`, `$name`). |
| `-n` / `--line-number` | Show line numbers (default when output is a terminal). |
| `-N` / `--no-line-number` | Suppress line numbers. |
| `--column` | Show the 1-based column number of the first match on each line. |
| `-H` / `--with-filename` | Show filenames (default when searching multiple files). |
| `--no-filename` | Suppress filenames. |
| `-b` / `--byte-offset` | Show the 0-based byte offset of each matching line. |
| `--heading` | Group matches by file, with the filename as a header (default for terminal). |
| `--no-heading` | Print filename on every line (default for pipe). |
| `-p` / `--pretty` | Alias for `--color=always --heading --line-number`. |
| `--vimgrep` | Print results in a format compatible with Vim's errorformat: `file:line:column:match`. |
| `--json` | Output results in JSON Lines format. Each line is a JSON object with a `type` field (`begin`, `match`, `context`, `end`, `summary`). |
| `-q` / `--quiet` | Suppress all output. Exit with 0 if a match is found, 1 otherwise. |

### 3.3 Context Lines

| Flag | Behavior |
|------|----------|
| `-A NUM` / `--after-context=NUM` | Show NUM lines after each match. |
| `-B NUM` / `--before-context=NUM` | Show NUM lines before each match. |
| `-C NUM` / `--context=NUM` | Show NUM lines before and after each match. |
| `--context-separator=STRING` | Separator between non-contiguous context groups (default: `--`). |
| `--no-context-separator` | Disable context separators. |

### 3.4 Filtering (Which Files to Search)

| Flag | Behavior |
|------|----------|
| `-g GLOB` / `--glob=GLOB` | Include/exclude files matching the glob. Prefix with `!` to exclude. Can be specified multiple times. |
| `--iglob=GLOB` | Like `--glob`, but case-insensitive. |
| `-t TYPE` / `--type=TYPE` | Only search files of the given type (e.g., `rust`, `py`, `json`). |
| `-T TYPE` / `--type-not=TYPE` | Exclude files of the given type. |
| `--type-add=SPEC` | Add a custom file type definition. |
| `--type-clear=TYPE` | Clear all globs for the given file type. |
| `--type-list` | List all supported file types and their globs, then exit. |
| `-u` / `--unrestricted` | Reduce filtering. One `-u` disables `.gitignore`. Two `-uu` also searches hidden files. Three `-uuu` also searches binary files. |
| `--hidden` / `--no-hidden` | Search/skip hidden files and directories. |
| `-L` / `--follow` | Follow symbolic links during directory traversal. |
| `--no-ignore` | Don't respect ignore files (`.gitignore`, `.ignore`, `.rgignore`). |
| `--no-ignore-vcs` | Don't respect VCS ignore files (`.gitignore`). |
| `--no-ignore-global` | Don't respect global gitignore. |
| `--no-ignore-parent` | Don't respect ignore files in parent directories. |
| `-d NUM` / `--max-depth=NUM` | Limit directory traversal depth. |
| `--max-filesize=SIZE` | Skip files larger than the given size (e.g., `1M`, `500K`). |
| `--one-file-system` | Don't cross filesystem boundaries during traversal. |

### 3.5 Binary File Handling

| Flag | Behavior |
|------|----------|
| `--binary` | Search binary files. Don't print a warning for binary files found by directory traversal, but still suppress binary output. |
| `-a` / `--text` | Treat binary files as text. Print binary matches as-is. |
| `--no-binary` | Default behavior — skip binary files (print warning when skipping). |

Binary detection works by scanning for NUL bytes. For files discovered via directory traversal, binary detection quits searching the file when a NUL is found and prints a warning. For files explicitly specified on the command line, binary detection never causes the file to be skipped entirely — but a NUL byte triggers a warning.

### 3.6 Regex Engine Configuration

| Flag | Behavior |
|------|----------|
| `--engine=ENGINE` | Select the regex engine: `default` (Rust regex), `pcre2`, or `auto`. |
| `--pcre2` | Use PCRE2 regex engine. |
| `--no-pcre2` | Use default Rust regex engine. |
| `--regex-size-limit=SIZE` | Set max compiled regex size. |
| `--dfa-size-limit=SIZE` | Set max DFA state size. |
| `--no-unicode` | Disable Unicode mode for patterns. |
| `-P` / `--pcre2` | Shorthand for `--engine=pcre2`. |
| `--crlf` | Treat `\r\n` as a line terminator. |
| `--null-data` | Use NUL (`\x00`) as the line terminator instead of newline. |

### 3.7 Search Behavior

| Flag | Behavior |
|------|----------|
| `-m NUM` / `--max-count=NUM` | Stop searching a file after NUM matching lines. |
| `--max-columns=NUM` | Truncate/omit lines longer than NUM bytes. |
| `--max-columns-preview` | Show a preview of truncated lines (instead of omitting entirely). |
| `--mmap` / `--no-mmap` | Control use of memory-mapped I/O. Default: auto (uses mmap heuristically for small file sets). |
| `-j NUM` / `--threads=NUM` | Number of threads to use. Default: heuristic based on available parallelism. |
| `--sort=CRITERIA` / `--sortr=CRITERIA` | Sort results by path, modified time, accessed time, or created time (ascending/descending). Sorting disables parallelism. |
| `-z` / `--search-zip` | Search inside compressed files (gz, bz2, xz, lz4, lzma, zstd, etc.). |
| `--pre=COMMAND` | Run a preprocessor command on each file before searching. |
| `--pre-glob=GLOB` | Only run the preprocessor on files matching the given glob. |
| `--stop-on-nonmatch` | After finding a match, stop searching the file as soon as a non-matching line is seen. |

### 3.8 Output Formatting

| Flag | Behavior |
|------|----------|
| `--color=WHEN` | Control colorized output: `never`, `always`, `auto` (default: auto, color when output is a terminal). |
| `--colors=SPEC` | Configure specific colors for match, path, line, and column. Format: `{type}:{attribute}:{value}`. |
| `--hyperlink-format=FORMAT` | Control hyperlink format for supported terminals. |
| `--path-separator=SEP` | Set the path separator character (default: OS-native). |
| `-0` / `--null` | Print NUL byte after file paths (for use with `xargs -0`). |
| `--no-messages` | Suppress error messages. |
| `--trim` | Trim leading ASCII whitespace from each line. |
| `--field-match-separator=SEP` | Set separator between fields in output (default: `:`). |
| `--field-context-separator=SEP` | Set separator between fields in context lines (default: `-`). |

### 3.9 Configuration File

ripgrep reads default arguments from a configuration file pointed to by the `RIPGREP_CONFIG_PATH` environment variable. Each line in the file is treated as a single argument. Lines starting with `#` are comments. Empty lines are ignored. The config file arguments are prepended to the actual CLI arguments, so CLI flags override config file flags.

### 3.10 Shell Completion & Man Page Generation

ripgrep can generate shell completions and a man page from its flag definitions:

| Flag | Output |
|------|--------|
| `--generate=man` | Generate man page in roff format. |
| `--generate=complete-bash` | Generate Bash completions. |
| `--generate=complete-zsh` | Generate Zsh completions. |
| `--generate=complete-fish` | Generate Fish completions. |
| `--generate=complete-powershell` | Generate PowerShell completions. |

---

## 4. Ignore File Precedence

Ignore rules are applied in a strict precedence order. Higher-precedence rules override lower ones:

1. **`--glob` / `--iglob`** flags (highest precedence)
2. **`.rgignore`** in the current directory and ancestors
3. **`.ignore`** in the current directory and ancestors
4. **`.gitignore`** in the current directory and ancestors
5. **Global git ignore** (from `core.excludesFile` in git config)

Each ignore file follows gitignore syntax. Within a single file, later rules override earlier rules. Rules in a child directory override rules in a parent directory.

A `.gitignore` rule can be negated with `!` prefix.

---

## 5. File Type System

ripgrep has a built-in registry of file type definitions mapping type names to glob patterns (e.g., `rust: *.rs`, `py: *.py`, `json: *.json`). The registry should be comprehensive (covering 100+ common file types).

Users can:
- Filter to specific types: `rg -t rust pattern`
- Exclude specific types: `rg -T py pattern`
- Add custom types: `rg --type-add 'mytype:*.xyz' -t mytype pattern`
- Clear type definitions: `rg --type-clear rust ...`
- List all types: `rg --type-list`

---

## 6. JSON Output Format

When `--json` is specified, ripgrep outputs JSON Lines (one JSON object per line). The schema:

### Message Types

- **`begin`**: Emitted at the start of searching a file. Contains: `path` (with `text` and/or `bytes` fields).
- **`end`**: Emitted at the end of searching a file. Contains: `path`, `stats` (matches, lines, bytes searched, etc.), `binary_offset` (if binary detected).
- **`match`**: Emitted for each matching line. Contains: `path`, `lines` (text/bytes), `line_number`, `absolute_offset`, `submatches` (array of `{match: {text}, start, end}`).
- **`context`**: Emitted for context lines. Same fields as `match` but without `submatches`.
- **`summary`**: Emitted at the very end (when `--stats` is used). Contains: `elapsed_total`, `stats` (aggregate).

The `text` field uses lossy UTF-8. The `bytes` field is base64-encoded raw bytes.

---

## 7. Color System

Colored output uses ANSI escape sequences. Colorable elements:
- **match**: The matched text (default: bold red)
- **path**: File paths (default: magenta)
- **line**: Line numbers (default: green)
- **column**: Column numbers (default: green)

The `--colors` flag allows full customization with the syntax `{type}:{attribute}:{value}`:
- Attributes: `fg`, `bg`, `style`
- Values: `red`, `green`, `blue`, `yellow`, `magenta`, `cyan`, `white`, `black`, or `{r},{g},{b}` for 24-bit color
- Styles: `bold`, `italic`, `underline`, `nobold`, etc.

---

## 8. Encoding Support

ripgrep supports transcoding files from various encodings to UTF-8 before searching. The `-E/--encoding` flag accepts any encoding label supported by the Encoding Standard (e.g., `utf-16le`, `shift_jis`, `euc-jp`).

When no encoding is specified, ripgrep assumes UTF-8 by default, falling back to byte-level matching for non-UTF-8 content.

---

## 9. Decompression Support

When `--search-zip` / `-z` is enabled, ripgrep transparently decompresses files before searching, based on file extension. Supported formats include:

| Extension | Decompression Tool |
|-----------|--------------------|
| `.gz` | `gzip -d` |
| `.bz2` | `bzip2 -d` |
| `.xz` | `xz -d` |
| `.lz4` | `lz4 -d` |
| `.lzma` | `xz --format=lzma -d` |
| `.zst` | `zstd -dq` |
| `.Z` | `uncompress` |
| `.br` | `brotli -d` |

Decompression is done by spawning the appropriate external tool and piping its output. If the tool is not available, the file is silently skipped.

---

## 10. Hyperlink Support

ripgrep supports terminal hyperlinks (OSC 8) for file paths in output. The format is configurable via `--hyperlink-format`, which supports placeholders:

- `{path}` — absolute file path
- `{host}` — hostname
- `{line}` — line number
- `{column}` — column number
- `{wslprefix}` — WSL path prefix conversion

Predefined aliases: `file` (default), `vscode`, `vscode-insiders`, `jetbrains`, `macvim`, `textmate`, `none`.

---

## 11. Preprocessor Support

The `--pre` flag specifies an external command that is run on each file before searching. The command receives the file path as its first argument and its stdin is connected to the file. ripgrep searches the command's stdout.

`--pre-glob` restricts which files the preprocessor is applied to.

---

## 12. Memory Map Heuristics

ripgrep uses memory-mapped I/O selectively:
- **Auto mode** (default): Uses mmap only when the number of files to search is very small (heuristic: ≤10 files) and all targets are regular files (not stdin, not pipes). This avoids performance cliffs and SIGBUS risks in highly concurrent scenarios.
- **Always mode** (`--mmap`): Force mmap for all file reads.
- **Never mode** (`--no-mmap`): Disable mmap entirely.

---

## 13. Parallelism Model

- Default thread count is based on available parallelism (typically number of CPU cores).
- Thread count of 1 forces single-threaded mode.
- Sorting (`--sort`, `--sortr`) forces single-threaded mode.
- In multi-threaded mode, directory traversal and file searching happen concurrently across the thread pool.
- Output from different files must never be interleaved in multi-threaded mode. Each file's complete output must be printed atomically.

---

## 14. Build Configuration

### 14.1 Binary

The build must produce a single binary named `rg`. The project should include integration tests that invoke the compiled binary with various arguments and assert on stdout/stderr output and exit codes.

### 14.2 Features

- `pcre2`: Optional feature that enables PCRE2 support.

### 14.3 Build Script

A `build.rs` should:
- Embed the current git revision hash as an environment variable for the version string.
- On Windows MSVC, embed a Windows manifest for long path support.

### 14.4 Allocator

When building for `musl` targets on 64-bit, use `jemalloc` as the global allocator (musl's default allocator has poor performance for ripgrep's workload).

### 14.5 Edition & MSRV

- Rust edition: 2024
- Minimum supported Rust version: 1.85

---

## 15. Logging & Debugging

ripgrep uses the `log` crate for debug output. The `--debug` flag enables debug-level logging, which shows why files are being skipped (ignore rules, binary detection, etc.).

`--trace` enables even more verbose logging.

---

## 16. Error Handling Conventions

- Non-fatal errors (permission denied, invalid encoding in a single file) should be reported to stderr and the search should continue.
- Fatal errors (invalid regex, broken pipe in specific contexts) should cause immediate termination.
- Broken pipe errors should result in graceful exit with code 0.

---

## 17. Key Design Decisions to Preserve

1. **Smart-case is the default**: Case-insensitive only when the entire pattern is lowercase.
2. **Pattern deduplication**: When multiple patterns are provided, duplicate patterns should be removed before compilation to avoid DFA performance penalties.
3. **Line-buffered vs block-buffered**: Output to terminals is line-buffered (for responsiveness); output to pipes/files is block-buffered (for throughput).
4. **Implicit vs explicit paths**: The behavior of binary detection differs between files found via directory traversal (implicit) and files specified on the command line (explicit). Explicit files are never silently skipped due to binary content.
5. **Config file arguments are prepended**: CLI args override config file args because config file args come first in the argument list.
6. **No search when matches impossible**: If the regex engine determines that no match is possible (e.g., the pattern is self-contradictory), skip the search entirely and exit with code 1.
7. **The "nothing searched" warning**: When no files are searched and the path was implicit (current directory), print a helpful warning suggesting `--debug` to investigate.

---

## 18. Build & Test Expectations

### Build

```bash
cargo build
```

must succeed, producing a `target/debug/rg` binary.

### Tests

```bash
cargo test
```

must pass all tests. The project should have integration tests that invoke the compiled `rg` binary with various arguments and assert on stdout/stderr output and exit codes.

---

## 19. Statistics Output

When `--stats` is passed, ripgrep prints aggregate statistics after the search:

In text mode:
```
N matches
N matched lines
N files contained matches
N files searched
N bytes printed
N bytes searched
0.NNNNNN seconds spent searching
0.NNNNNN seconds total
```

In JSON mode, a `summary` JSON message is emitted with `stats` and `elapsed_total` fields.
