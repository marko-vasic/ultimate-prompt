# Diff Report — Iteration 1

## Summary
- Build status: PASS
- Equivalence tests: 34/89 passed (55 failed)
- Capabilities equivalent: 20
- Capabilities partial: 7
- Capabilities missing/broken: 10

---

## Build Errors
None. The generated codebase compiled successfully using Cargo (`cargo build`).
- **Warnings**: 15 compiler warnings across crates (2 in `grep-searcher`, 6 in `ignore`, 7 in `ripgrep` binary), mostly dead code and unused variables/imports (`last_output_line`, `capacity`, `GlobMatcher`, `takes_value`, `stdout`).

---

## Test Failures

### 1. `T001: Simple pattern match`
- **Error summary**: `--no-filename` was ignored, outputting path prefix `/tmp/.../test.txt:`, and spurious context separator `--` was emitted between non-adjacent matches.
- **Expected output**:
  ```
  Hello World
  Hello Again
  ```
- **Actual output**:
  ```
  /tmp/.../t001/test.txt:Hello World
  --
  /tmp/.../t001/test.txt:Hello Again
  ```

### 2. `T004: Regex pattern`
- **Error summary**: `--no-filename` was ignored, outputting path prefix.
- **Expected output**:
  ```
  foo123bar
  foo456baz
  ```
- **Actual output**:
  ```
  /tmp/.../t004/test.txt:foo123bar
  /tmp/.../t004/test.txt:foo456baz
  ```

### 3. `T005: Multiple files`
- **Error summary**: `--sort path` was ignored during directory traversal, resulting in non-deterministic file output order.
- **Expected output**:
  ```
  a.txt:apple
  c.txt:apricot
  ```
- **Actual output**:
  ```
  b.txt:... (or c.txt before a.txt)
  ```

### 4. `T010: Case insensitive`
- **Error summary**: `--no-filename` ignored (path prefix) and spurious `--` separator lines.
- **Expected output**:
  ```
  Hello
  hello
  HELLO
  ```
- **Actual output**:
  ```
  /tmp/.../t010/test.txt:Hello
  --
  /tmp/.../t010/test.txt:hello
  --
  /tmp/.../t010/test.txt:HELLO
  ```

### 5. `T011: Smart-case lowercase`
- **Error summary**: `--no-filename` ignored + spurious `--` separator lines.
- **Expected output**: `Hello\nhello\nHELLO`
- **Actual output**: `/tmp/.../t011/test.txt:Hello\n--\n/tmp/.../t011/test.txt:hello\n--\n/tmp/.../t011/test.txt:HELLO`

### 6. `T012: Smart-case mixed`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `Hello`
- **Actual output**: `/tmp/.../t012/test.txt:Hello`

### 7. `T013: Case sensitive`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `hello`
- **Actual output**: `/tmp/.../t013/test.txt:hello`

### 8. `T020: Invert match`
- **Error summary**: `--no-filename` ignored + spurious `--` separator lines.
- **Expected output**: `alpha\ngamma`
- **Actual output**: `/tmp/.../t020/test.txt:alpha\n--\n/tmp/.../t020/test.txt:gamma`

### 9. `T030: Line numbers`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `2:bar`
- **Actual output**: `/tmp/.../t030/test.txt:2:bar`

### 10. `T031: No line numbers`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `bar`
- **Actual output**: `/tmp/.../t030/test.txt:bar`

### 11. `T032: Column numbers`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `1:7:hello world`
- **Actual output**: `/tmp/.../t032/test.txt:1:7:hello world`

### 12. `T040: After context`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `two\nthree`
- **Actual output**: `/tmp/.../t040/test.txt:two\n/tmp/.../t040/test.txt-three`

### 13. `T041: Before context`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `two\nthree`
- **Actual output**: `/tmp/.../t041/test.txt-two\n/tmp/.../t041/test.txt:three`

### 14. `T042: Context both`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `two\nthree\nfour`
- **Actual output**: `/tmp/.../t042/test.txt-two\n/tmp/.../t042/test.txt:three\n/tmp/.../t042/test.txt-four`

### 15. `T043: Context separator`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `2:b\n3-c\n--\n6:f\n7-g`
- **Actual output**: `/tmp/.../t043/test.txt:2:b\n/tmp/.../t043/test.txt-3-c\n--\n/tmp/.../t043/test.txt:6:f\n/tmp/.../t043/test.txt-7-g`

### 16. `T050: Count lines`
- **Error summary**: `--no-filename` ignored in count mode, prefixing filename.
- **Expected output**: `3`
- **Actual output**: `/tmp/.../t050/test.txt:3`

### 17. `T051: Count matches`
- **Error summary**: `--no-filename` ignored in count mode, prefixing filename.
- **Expected output**: `3`
- **Actual output**: `/tmp/.../t051/test.txt:3`

### 18. `T060: Files with matches`
- **Error summary**: `--sort path` was ignored, outputting files in directory walk order.
- **Expected output**: `a.txt\nc.txt`
- **Actual output**: `c.txt\na.txt`

### 19. `T070: Only matching`
- **Error summary**: `--no-filename` ignored (path prefix) and spurious `--`.
- **Expected output**: `foo\nfoo`
- **Actual output**: `/tmp/.../t070/test.txt:foo\n--\n/tmp/.../t070/test.txt:foo`

### 20. `T080: Fixed string`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `a.b`
- **Actual output**: `/tmp/.../t080/test.txt:a.b`

### 21. `T090: Word regexp`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `foo bar`
- **Actual output**: `/tmp/.../t090/test.txt:foo bar`

### 22. `T091: Line regexp`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `foo`
- **Actual output**: `/tmp/.../t091/test.txt:foo`

### 23. `T100: Simple replace`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `hello planet`
- **Actual output**: `/tmp/.../t100/test.txt:hello planet`

### 24. `T101: Capture group replace`
- **Error summary**: Capture group variable (`$1`) in `--replace` replacement pattern was not expanded, and `--no-filename` was ignored.
- **Expected output**: `foo123bar`
- **Actual output**: `/tmp/.../t101/test.txt:foo$1bar`

### 25. `T110: Include glob`
- **Error summary**: Glob filter (`-g '*.rs'`) was not applied during directory traversal; searched all files.
- **Expected output**: `match` (from `b.rs`)
- **Actual output**: Matches from `a.txt`, `b.rs`, and `c.py`.

### 26. `T111: Exclude glob`
- **Error summary**: Glob negative filter (`-g '!*.rs'`) was not applied during directory traversal.
- **Expected output**: `a.txt\nc.txt`
- **Actual output**: `b.rs\na.txt\nc.txt` (unsorted and un-filtered).

### 27. `T120: File type filter`
- **Error summary**: Type filter (`-t rust`) was not applied during directory traversal.
- **Expected output**: `match` (from `a.rs`)
- **Actual output**: Matches from `a.rs`, `b.py`, and `c.txt`.

### 28. `T121: Exclude file type`
- **Error summary**: Type exclusion (`-T rust`) was not applied during directory traversal.
- **Expected output**: `b.py`
- **Actual output**: `a.rs\nb.py`

### 29. `T122: Type list has python`
- **Error summary**: `--type-list` is missing standard programming language definitions, specifically Python (`py:`).
- **Expected output**: Contains `py: *.py, *.pyw`
- **Actual output**: Only lists 5 types (`md`, `rust`, `ts`, `toml`, `c`).

### 30. `T140: Gitignore respected`
- **Error summary**: `.gitignore` was not applied during traversal; ignored files were searched.
- **Expected output**: `keep.txt`
- **Actual output**: `ignored.log\nkeep.txt`

### 31. `T143: .ignore file`
- **Error summary**: `.ignore` file was not applied during traversal.
- **Expected output**: `keep.txt`
- **Actual output**: `skip.dat\nkeep.txt`

### 32. `T150: Binary warning`
- **Error summary**: Binary files with matches were skipped silently without printing `Binary file <path> matches`.
- **Expected output**: Contains `binary file matches` or `Binary file ... matches`
- **Actual output**: *(empty)*

### 33. `T160: Max count`
- **Error summary**: `-m 2` flag was parsed into CLI struct but ignored during execution.
- **Expected output**: 2 matching lines (`foo\nfoo`)
- **Actual output**: 5 matching lines

### 34. `T180: Stdin search`
- **Error summary**: Piped input (`echo "hello world" | rg "hello"`) searched the current directory `.` recursively instead of reading from standard input.
- **Expected output**: `hello world`
- **Actual output**: Search results across the entire repo directory tree.

### 35. `T181: Stdin no match`
- **Error summary**: Piped input with no match exited 0 (or searched the repo) instead of exiting 1.
- **Expected output**: Exit code 1
- **Actual output**: Exit code 0

### 36. `T190: Multiple patterns`
- **Error summary**: `--no-filename` ignored + spurious `--`.
- **Expected output**: `alpha\ngamma`
- **Actual output**: `/tmp/.../t190/test.txt:alpha\n--\n/tmp/.../t190/test.txt:gamma`

### 37. `T200: Patterns from file`
- **Error summary**: `--no-filename` ignored + spurious `--`.
- **Expected output**: `alpha\ngamma`
- **Actual output**: `/tmp/.../t200/test.txt:alpha\n--\n/tmp/.../t200/test.txt:gamma`

### 38. `T210: List files`
- **Error summary**: `--files --sort path` failed to sort paths.
- **Expected output**: `bar.rs\nfoo.txt\nsub/baz.py`
- **Actual output**: `sub/baz.py\nbar.rs\nfoo.txt`

### 39. `T211: File list gitignore`
- **Error summary**: `rg --files` did not apply `.gitignore` rules during directory listing.
- **Expected output**: `keep.txt`
- **Actual output**: `skip.log\nkeep.txt`

### 40. `T230: Null-terminated`
- **Error summary**: `--sort path` was ignored in null-terminated file listing.
- **Expected output**: `a.txt\nb.txt`
- **Actual output**: `b.txt\na.txt`

### 41. `T240: Byte offset`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `4:bbb`
- **Actual output**: `/tmp/.../t240/test.txt:4:bbb`

### 42. `T251: JSON submatches`
- **Error summary**: JSON Lines `"match"` object is missing the required `"submatches"` field.
- **Expected output**: Contains `"submatches"`
- **Actual output**: `{"data":{"absolute_offset":0,"line_number":1,"lines":{"text":"hello world\n"},"path":{"text":"..."}},"type":"match"}`

### 43. `T260: Multiline match`
- **Error summary**: `-U` multiline flag did not enable cross-newline matching in the search buffer; searcher continued to scan line-by-line.
- **Expected output**: `foo\nbar`
- **Actual output**: *(empty)*

### 44. `T290: Trim`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `hello`
- **Actual output**: `/tmp/.../t290/test.txt:hello`

### 45. `T330: Config file`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `hello`
- **Actual output**: `/tmp/.../t330/test.txt:hello`

### 46. `T331: CLI overrides config`
- **Error summary**: `--no-filename` ignored + spurious `--`.
- **Expected output**: `Hello\nhello\nHELLO`
- **Actual output**: `/tmp/.../t331/test.txt:Hello\n--\n/tmp/.../t331/test.txt:hello\n--\n/tmp/.../t331/test.txt:HELLO`

### 47. `T351: No-heading format`
- **Error summary**: `--sort path` ignored, producing wrong first line.
- **Expected output**: `a.txt:1:match`
- **Actual output**: `b.txt:1:match`

### 48. `T370: Sort path`
- **Error summary**: `--sort path` did not sort results in ascending order.
- **Expected output**: `a.txt\nb.txt\nc.txt`
- **Actual output**: `b.txt\na.txt\nc.txt`

### 49. `T371: Reverse sort path`
- **Error summary**: `--sortr path` did not sort results in descending order.
- **Expected output**: `c.txt\nb.txt\na.txt`
- **Actual output**: `b.txt\na.txt\nc.txt`

### 50. `T380: Stop on nonmatch`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `foo1\nfoo2`
- **Actual output**: `/tmp/.../t380/test.txt:foo1\n/tmp/.../t380/test.txt:foo2`

### 51. `T390: Stats matches`
- **Error summary**: `--stats` failed to print statistics summary block.
- **Expected output**: Contains `2 matches`
- **Actual output**: Only output matching lines; no statistics summary printed.

### 52. `T390: Stats matched lines`
- **Error summary**: `--stats` failed to print statistics summary block.
- **Expected output**: Contains `2 matched lines`
- **Actual output**: No statistics summary printed.

### 53. `T390: Stats files`
- **Error summary**: `--stats` failed to print statistics summary block.
- **Expected output**: Contains `1 files searched`
- **Actual output**: No statistics summary printed.

### 54. `T410: Null data`
- **Error summary**: `--null-data` failed because binary detection treated NUL bytes in the input as binary and aborted immediately.
- **Expected output**: `bar`
- **Actual output**: *(empty)*

### 55. `T420: CRLF`
- **Error summary**: `--no-filename` ignored (path prefix).
- **Expected output**: `hello`
- **Actual output**: `/tmp/.../t420/test.txt:hello`

---

## Capability Assessment

### 1. Single-File Search & Filename Suppressing (`--no-filename`, `-H`, `--with-filename`)
- **Status**: ⚠️ Partial
- **Expected behavior**: When searching a single positional file argument, or when `--no-filename` is passed, matched lines and count summaries must NOT include a file path prefix (e.g. `Hello World`, not `test.txt:Hello World`; and `3`, not `test.txt:3`). When multiple files are searched or `-H` is passed, paths must be prefixed.
- **Actual behavior**: The CLI argument parser stored `with_filename = Some(false)` in the configuration struct, but the printer configuration pipeline in `src/search.rs` never passed this value to `StandardBuilder` or `SummaryBuilder`. As a result, the printer defaulted to prefixing the path on every line across almost all search modes.
- **Prompt gap**: The prompt specifies `--no-filename` and `--with-filename` in the CLI table, but does not explicitly specify the default filename behavior: "When searching exactly one file or reading from stdin, do not prefix the filename by default unless `-H`/`--with-filename` is given. When searching a directory or multiple paths, prefix the filename by default unless `--no-filename` is given. Both standard search and count modes (`-c`) must respect this."

### 2. Context Group Separator (`--context-separator`, `--no-context-separator`)
- **Status**: ⚠️ Partial
- **Expected behavior**: Context separator lines (`--`) must ONLY appear when context flags (`-A`, `-B`, or `-C`) are in effect and non-contiguous match groups are separated by non-matching lines. When searching without context flags (`-A 0 -B 0`), matches must be output consecutively with no separators.
- **Actual behavior**: In `crates/grep-searcher/src/searcher.rs`, the search loop called `sink.context_break()` whenever there was any gap between matching line numbers, regardless of whether context lines were requested. In `crates/grep-printer`, the printer unconditionally printed `--` whenever `needs_separator` was true.
- **Prompt gap**: Although the prompt included a critical rule for this in v1, the generator decoupled `grep-searcher` and `grep-printer` into separate subcrates and failed to wire the context count condition across the crate boundary. The prompt should explicitly state in the searcher/printer architectural specification: "The searcher must only emit context break events when the configured before or after context count is greater than zero."

### 3. Path & Result Sorting (`--sort`, `--sortr`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: Passing `--sort path` must sort search results and file listings in ascending lexicographical order by path. Passing `--sortr path` must sort in descending order.
- **Actual behavior**: `--sort` and `--sortr` were parsed and caused the searcher to use single-threaded execution, but the directory traversal engine in `crates/ignore` never collected and sorted the paths before yielding them.
- **Prompt gap**: The prompt lists `--sort=TYPE` and `--sortr=TYPE` in the flags table, but does not specify that the directory iterator / walker must sort directory entries before descending or yielding paths when sort flags are specified.

### 4. Layered Ignore & Filtering Integration (`.gitignore`, `.ignore`, `-g`, `-t`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: Recursive directory traversal must automatically skip files and directories matching `.gitignore`, `.ignore`, and `.rgignore` files at each directory level. Passing `-g GLOB` must include/exclude matching files, and `-t TYPE` must filter by file extension.
- **Actual behavior**: The generator wrote the logic for `Gitignore`, `Override`, and `Types` in separate submodules (`crates/ignore/src/{gitignore,overrides,types}.rs`), but `IgnoreBuilder::build()` in `crates/ignore/src/lib.rs` simply ran raw `walkdir::WalkDir` without calling any of the ignore/override/type matching logic during the directory walk.
- **Prompt gap**: The prompt lists the modular crates and ignore features, but does not explicitly emphasize that the `Walk` iterator must evaluate the composite ignore stack (`.gitignore`, `.ignore`, `.rgignore`, glob overrides, and file types) on each directory entry encountered during traversal before yielding it.

### 5. Standard Input (Piped Stdin) Detection
- **Status**: ❌ Missing/Broken
- **Expected behavior**: When data is piped into `rg` (e.g. `echo "foo" | rg "foo"`) and no positional search path is provided on the command line, `rg` must automatically read from standard input (`<stdin>`), not search the current directory `.`.
- **Actual behavior**: When positional paths were empty, the CLI parser unconditionally appended `PathBuf::from(".")`, causing `echo "hello" | rg "hello"` to recursively search the entire filesystem tree under the working directory instead of reading stdin.
- **Prompt gap**: The prompt should explicitly state: "When no PATH arguments are provided: if standard input is a pipe/redirect (not a TTY), search stdin; if standard input is a TTY, search the current directory (`.`)."

### 6. Multi-line Regex Matching (`-U` / `--multiline`, `--multiline-dotall`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: With `-U`, regex patterns containing `\n` or matching across line boundaries (e.g. `foo\nbar`) must successfully match across lines and output the entire multiline match.
- **Actual behavior**: `SearcherBuilder.multi_line(true)` was recorded, but `Searcher::search_slice` strictly iterated line-by-line using a line iterator. Patterns matching across lines always failed.
- **Prompt gap**: The prompt should specify that in multiline mode (`-U`), regex search cannot operate strictly on individual lines; the searcher must execute regex matches against multi-line buffer chunks and calculate line numbers from match byte offsets.

### 7. Null-Data Mode & Binary Detection (`--null-data`, `--binary`)
- **Status**: ⚠️ Partial
- **Expected behavior**: `--null-data` treats `\0` (NUL) as the line separator instead of `\n`. It must allow searching data containing NUL bytes without binary detection aborting the search.
- **Actual behavior**: Binary detection (`BinaryDetection::Quit`) defaulted to active and aborted search on the first `\0` byte encountered, immediately terminating `--null-data` searches with zero output. Furthermore, when binary files matched in normal mode, the `Binary file <path> matches` message was omitted.
- **Prompt gap**: The prompt should specify: "When `--null-data` is enabled, the line terminator is NUL (`\0`) and binary detection is disabled. When searching in normal mode, if a binary file matches, emit `Binary file <path> matches` (unless suppressed by `-q` or summary modes)."

### 8. Match Limiting (`-m` / `--max-count`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: `-m N` must limit the number of matching lines output per file to at most `N`.
- **Actual behavior**: `-m` was parsed into `Args.max_count` but never passed to the searcher or printer, outputting all matches regardless of count.
- **Prompt gap**: The prompt should specify: "When `-m N` / `--max-count=N` is given, stop searching each file after `N` matching lines have been output."

### 9. Aggregate Search Statistics (`--stats`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: Passing `--stats` must output the aggregate search summary table at the end of execution on stderr/stdout (e.g., lines matched, matches found, files searched).
- **Actual behavior**: The `--stats` flag was parsed into `Args.stats`, and a `Stats` data structure was implemented in `crates/grep-printer`, but `src/search.rs` never invoked the stats collection or printing routines at the end of the search.
- **Prompt gap**: The prompt should explicitly state: "When `--stats` is specified, track aggregate counters (total matches, matched lines, files searched, duration) and print the summary statistics block after search completes."

### 10. Replacement with Capture Groups (`-r` / `--replace`)
- **Status**: ❌ Missing/Broken
- **Expected behavior**: Passing `-r REPLACEMENT` replaces matching spans with the replacement string. If the replacement string contains `$1`, `$2`, or `$name`, these must be expanded to the corresponding regex capture groups.
- **Actual behavior**: `StandardBuilder` stored the replacement string, but `StandardSink` never performed replacements when printing lines, and capture group expansion was not implemented.
- **Prompt gap**: The prompt should state: "When `-r` / `--replace` is provided, printed output must replace the matched substring with the replacement text. Support expanding regex capture groups like `$1`, `$2`, and `${name}` in the replacement text."

### 11. JSON Output Submatches Schema (`--json`)
- **Status**: ⚠️ Partial
- **Expected behavior**: Each `"match"` object in `--json` output must contain a `"submatches"` array recording each match within the line: `[{"match": {"text": "..."}, "start": 0, "end": 5}]`.
- **Actual behavior**: The generated JSON printer output `"type": "match"` with `"path"`, `"line_number"`, `"lines"`, and `"absolute_offset"`, but omitted the `"submatches"` field.
- **Prompt gap**: The prompt should provide the exact JSON structure for `"match"` objects, including the mandatory `"submatches"` list containing `match.text`, `start`, and `end` byte offsets.

### 12. Standard File Type Definitions (`--type-list`, `-t`, `-T`)
- **Status**: ⚠️ Partial
- **Expected behavior**: `--type-list` must include standard programming language types (Python `py: *.py, *.pyw`, C/C++, Java, Rust, JavaScript, TypeScript, Go, HTML, CSS, JSON, Markdown, etc.).
- **Actual behavior**: `--type-list` only included 5 hardcoded types (`md`, `rust`, `ts`, `toml`, `c`), missing Python and other standard languages.
- **Prompt gap**: The prompt should specify a comprehensive list of default file types that must be supported out of the box.

---

## Prompt Compliance
- **Context Separators (`--`)**: The prompt explicitly specified: *"Context group separators (`--`) MUST ONLY be printed when context lines (`-A`, `-B`, or `-C`) are explicitly requested (`> 0`). When no context lines are requested (the default single-line matching mode), context separator lines (`--`) must NEVER be printed between non-adjacent matching lines."* The generator failed to comply because it unconditionally called `sink.context_break()` in `grep-searcher` on non-adjacent lines and unconditionally printed `--` in `grep-printer`.
- **Filename Suppressing (`--no-filename`)**: The prompt explicitly defined `--no-filename` to suppress filenames. The generator parsed the flag into `Args` but failed to wire it into the `grep-printer` builder invocation.
- **Sorting (`--sort`)**: The prompt explicitly listed `--sort` and `--sortr`. The generator switched to single-threaded mode but failed to sort entries in the directory walker.
- **Statistics (`--stats`)**: The prompt explicitly listed `--stats`. The generator created a stats struct in `grep-printer` but never invoked it in the CLI search pipeline.
- **Stdin Piped Search**: The prompt listed `command | rg [OPTIONS] PATTERN`. The generator unconditionally defaulted empty paths to `.`, breaking piped stdin search.

---

## Critique

Iteration 1 made architectural progress by implementing an 8-crate workspace structure (`grep-matcher`, `grep-regex`, `grep-searcher`, `grep-printer`, `grep-cli`, `ignore`, `globset`, `ripgrep`) that compiled cleanly. However, the iteration suffered from a significant regression in equivalence test pass rate (34/89 vs 70/89 in iteration 0). 

Root-cause analysis reveals that this regression was caused by two main failure modes:

### 1. The "Wired vs Unwired" Crate Gap (Generator Limitation & Structural Blindspot)
The generator successfully wrote rich domain logic inside isolated subcrates (e.g. `Gitignore` in `crates/ignore/src/gitignore.rs`, `Override` in `crates/ignore/src/overrides.rs`, `Types` in `crates/ignore/src/types.rs`, `Stats` in `crates/grep-printer/src/stats.rs`), but completely failed to wire these components into the main execution loop in `src/search.rs` and `crates/ignore/src/lib.rs`.
- `IgnoreBuilder::build()` fell back to a primitive `walkdir` wrapper that ignored all `.gitignore`, `.ignore`, glob, and type rules.
- `src/search.rs` failed to pass `with_filename`, `max_count`, and `stats` to the printer and searcher builders.
- This represents a generator failure to connect decoupled components across crate boundaries. To prevent this, the prompt must explicitly specify end-to-end integration requirements at the CLI orchestration layer.

### 2. Pervasive Output Cascading (Filename Prefix & Context Separator)
A massive cluster of 30+ test failures was caused by just two simple printer bugs:
- Always prefixing filenames on single-file searches and ignoring `--no-filename`.
- Emitting spurious `--` separators between matches when context was not requested.
Because almost all unit/equivalence tests verify single-file output with `--no-filename`, these two formatting defects broke tests across Basic Search, Case Sensitivity, Invert Match, Line Numbers, Fixed Strings, Word/Line Regexp, and Config Files.

### 3. Missing Runtime Dynamic Defaults (Stdin & Null-Data)
- The generator failed to implement standard Unix CLI behavior for stdin detection (`is_terminal()` / TTY check), resulting in `rg` ignoring piped input when no path was specified.
- Binary detection was implemented as a hard-coded short-circuit on NUL bytes, which broke `--null-data` search where NUL is the valid line separator.

---

## Top Learnings

1. **Default Filename & Prefixing Behavior**: The prompt must explicitly specify the exact rules for filename output: *"When searching stdin or a single file positional argument, do NOT prefix the file path to matching lines or count summaries unless `-H`/`--with-filename` is given. When searching multiple files or a directory, prefix the file path on every line unless `--no-filename` or `--heading` is given."*
2. **Searcher Context Break Contract**: The prompt must specify in the `grep-searcher` / `grep-printer` contract: *"The searcher must ONLY invoke `context_break()` when before-context (`-B`) or after-context (`-A`) is greater than 0. Under default single-line matching (`-A 0 -B 0`), `context_break()` must never be called, and `--` separators must never be emitted."*
3. **Ignore Traversal Stack Integration**: The prompt must explicitly specify that the directory walker (`Walk` / `WalkParallel`) must actively evaluate the layered ignore stack (`.gitignore`, `.ignore`, `.rgignore`), glob overrides (`-g`), and file types (`-t`) against each file and directory before yielding or descending into it.
4. **Piped Stdin vs Current Directory Default**: The prompt must specify the standard POSIX input detection rule: *"When no positional PATH is provided, check if standard input is piped (non-TTY). If stdin is piped, search stdin. If stdin is a TTY, default to searching the current directory (`.`)."*
5. **Path Sorting in Directory Walker**: The prompt must specify that when `--sort` or `--sortr` is supplied, directory entries must be collected and sorted by path/date before processing or outputting.
6. **Multiline Search Execution**: The prompt must specify that multiline mode (`-U`) requires executing regex matches against multi-line buffer slices rather than per-line iteration.
7. **Null-Data & Binary Detection Interaction**: The prompt must specify that `--null-data` disables binary detection (since NUL is the line separator). In normal search, binary files with matches must output `Binary file <path> matches`.
8. **Statistics & Match Limiting Orchestration**: The prompt must explicitly state that `-m N` stops searching each file after `N` matches, and `--stats` prints the aggregate statistics summary at the end of the search run.
9. **Capture Group Expansion in Replacement**: The prompt must state that `-r` / `--replace` must expand capture group placeholders (`$1`, `$2`, `${name}`) when replacing matching text.
10. **JSON Submatches Structure**: The prompt must provide the exact JSON Lines format for `"match"` items, including the `"submatches"` array containing match text, start, and end offsets.
