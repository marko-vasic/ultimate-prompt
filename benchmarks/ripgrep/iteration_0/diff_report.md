# Diff Report — Iteration 0

## Summary
- Build status: PASS
- Equivalence tests: 70/89 passed (19 failed)
- Capabilities equivalent: 20
- Capabilities partial: 7
- Capabilities missing/broken: 7

---

## Build Errors
None. The code compiled successfully using Cargo (`cargo build`).
- **Warnings**: 8 dead code warnings in `crates/ignore/src/lib.rs` (unused struct fields and helper methods: `message`, `ErrorKind::Parse`, `parse`, `selected`, `original`, `root`, `from_walkdir`, `sort_by`).

---

## Test Failures

### 1. `T001: Simple pattern match`
- **Error summary**: Output contains unexpected `--` context group separator lines between non-adjacent matches when context flags (`-A`, `-B`, `-C`) were not specified.
- **Expected output**:
  ```
  Hello World
  Hello Again
  ```
- **Actual output**:
  ```
  Hello World
  --
  Hello Again
  ```

### 2. `T020: Invert match`
- **Error summary**: Invert match (`-v`) output contains unexpected `--` context group separator lines.
- **Expected output**:
  ```
  alpha
  gamma
  ```
- **Actual output**:
  ```
  alpha
  --
  gamma
  ```

### 3. `T101: Capture group replace`
- **Error summary**: Replacement flag (`-r`) does not substitute regex capture group variables (`$1`).
- **Expected output**: `foo123bar`
- **Actual output**: `foo$1bar`

### 4. `T142: rgignore precedence`
- **Error summary**: `.rgignore` un-ignore rules (`!test.log`) failed to override `.gitignore` rules (`*.log`).
- **Expected output**: `test.log`
- **Actual output**: *(empty)*

### 5. `T150: Binary warning`
- **Error summary**: Searching a binary file with matches failed to output the required binary match notification.
- **Expected output**: Should contain `binary file matches` (e.g. `Binary file <path> matches`).
- **Actual output**: *(empty)*

### 6. `T160: Max count`
- **Error summary**: `--max-count 2` (`-m 2`) failed to stop matching after 2 lines.
- **Expected output**: 2 matching lines (`foo`, `foo`).
- **Actual output**: 5 matching lines (`foo`, `foo`, `foo`, `foo`, `foo`).

### 7. `T190: Multiple patterns`
- **Error summary**: Searching with multiple `-e` pattern arguments outputs unexpected `--` context group separator lines.
- **Expected output**:
  ```
  alpha
  gamma
  ```
- **Actual output**:
  ```
  alpha
  --
  gamma
  ```

### 8. `T200: Patterns from file`
- **Error summary**: Reading patterns from a file (`-f`) outputs unexpected `--` context group separator lines.
- **Expected output**:
  ```
  alpha
  gamma
  ```
- **Actual output**:
  ```
  alpha
  --
  gamma
  ```

### 9. `T210: List files`
- **Error summary**: `rg --files <path>` ignored the positional `<path>` argument and listed files from the current working directory.
- **Expected output**:
  ```
  bar.rs
  foo.txt
  sub/baz.py
  ```
- **Actual output**: Listed repository root files (e.g. `./working_dir/black/...`).

### 10. `T211: File list gitignore`
- **Error summary**: `rg --files <path>` ignored the target directory argument and printed repository root paths.
- **Expected output**: `keep.txt`
- **Actual output**: Listed repository root paths (`./working_dir/ripgrep/...`).

### 11. `T310: Version`
- **Error summary**: `rg --version` printed binary crate version string without starting with product name `ripgrep`.
- **Expected output**: Contains `ripgrep` (e.g. `ripgrep 14.1.0`).
- **Actual output**: `rg 0.1.0`

### 12. `T360: Vimgrep format`
- **Error summary**: `--vimgrep` output format omitted the file path prefix.
- **Expected output**: `test.txt:1:7:hello world`
- **Actual output**: `1:7:world`

### 13. `T371: Reverse sort path`
- **Error summary**: `--sortr path` sorted file paths in ascending order instead of descending (reverse) order.
- **Expected output**:
  ```
  c.txt
  b.txt
  a.txt
  ```
- **Actual output**:
  ```
  a.txt
  b.txt
  c.txt
  ```

### 14. `T390: Stats matches`
- **Error summary**: `--stats` failed to print match count summary block.
- **Expected output**: Output contains `2 matches`.
- **Actual output**: Match results followed by `--` without stats summary.

### 15. `T390: Stats matched lines`
- **Error summary**: `--stats` failed to print line count summary.
- **Expected output**: Output contains `2 matched lines`.
- **Actual output**: Match results without stats summary.

### 16. `T390: Stats files`
- **Error summary**: `--stats` failed to print files searched summary.
- **Expected output**: Output contains `1 files searched`.
- **Actual output**: Match results without stats summary.

### 17. `T410: Null data`
- **Error summary**: `--null-data` (`-0`) failed to treat NUL bytes as line terminators during matching.
- **Expected output**: `bar`
- **Actual output**: *(empty)*

### 18. `T430: Man page content`
- **Error summary**: `rg --generate man` failed because `--generate` flag was not implemented.
- **Expected output**: Generates man page output on stdout.
- **Actual output**: Exit code 2 (unknown flag).

### 19. `T431: Generate bash exit code`
- **Error summary**: `rg --generate complete-bash` returned exit code 2 instead of 0.
- **Expected exit code**: 0
- **Actual exit code**: 2

---

## Capability Assessment

### Basic Search & Regex Matching
- Status: ✅
- Expected behavior: Given a positional pattern and target path, search file lines matching the regex pattern and print matching lines.
- Actual behavior: Matches patterns using Rust regex-automata engine as expected.
- Prompt gap: None.

### Case Sensitivity & Smart Case (`-i`, `-s`, `-S`)
- Status: ✅
- Expected behavior: Smart-case searching by default (case-insensitive if all lowercase, case-sensitive if uppercase present); explicitly overridable via `-i` and `-s`.
- Actual behavior: Operates correctly according to specified flags.
- Prompt gap: None.

### Line Numbers & Columns (`-n`, `-b`, `--column`)
- Status: ✅
- Expected behavior: Displays 1-based line numbers and byte offsets when requested.
- Actual behavior: Output matches expected line and column numbers.
- Prompt gap: None.

### Count Mode (`-c`)
- Status: ✅
- Expected behavior: Outputs file paths with total matching line count (`path:count`).
- Actual behavior: Correctly computes and formats line counts.
- Prompt gap: None.

### File Match Listing (`-l`, `-L`)
- Status: ✅
- Expected behavior: `-l` lists paths with matches; `-L` lists paths without matches.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Only Matching (`-o`)
- Status: ✅
- Expected behavior: Prints only the matching substring of each matched line.
- Actual behavior: Matches expected output.
- Prompt gap: None.

### Fixed Strings, Word & Line Regexp (`-F`, `-w`, `-x`)
- Status: ✅
- Expected behavior: Respects literal pattern matching, word boundary wrapping, and full line matching.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Glob Filtering (`-g`, `--iglob`)
- Status: ✅
- Expected behavior: Filters searched files using include/exclude glob patterns.
- Actual behavior: Correctly includes and excludes paths based on globs.
- Prompt gap: None.

### Hidden File Search (`-.`)
- Status: ✅
- Expected behavior: Skips hidden files (`.*`) by default; searches them when `-./--hidden` is supplied.
- Actual behavior: Skips and includes hidden files correctly.
- Prompt gap: None.

### Quiet Mode (`-q`)
- Status: ✅
- Expected behavior: Suppresses all stdout/stderr output and exits immediately with 0 if a match is found, 1 if no match is found.
- Actual behavior: Works as expected.
- Prompt gap: None.

### Stdin Search
- Status: ✅
- Expected behavior: Reads and searches from stdin when input is piped or path is `-`.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Max Depth (`--max-depth`)
- Status: ✅
- Expected behavior: Limits directory traversal depth.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Null-Terminated Output (`--null`)
- Status: ✅
- Expected behavior: Separates file paths with NUL bytes in file listing / match headers.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### JSON Output (`--json`)
- Status: ✅
- Expected behavior: Emits structured JSON Lines search events (begin, match, end).
- Actual behavior: Emits valid JSON payload matching expected schema.
- Prompt gap: None.

### Multiline Matching (`-U`)
- Status: ✅
- Expected behavior: Allows regex pattern matching across newline boundaries.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Max Columns & Truncation (`-M`)
- Status: ✅
- Expected behavior: Truncates long lines exceeding max column limit.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Symlink Traversal (`-L`)
- Status: ✅
- Expected behavior: Follows symbolic links during directory traversal.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Trim Whitespace (`--trim`)
- Status: ✅
- Expected behavior: Trims leading whitespace from printed matching lines.
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Unrestricted Search (`-u`, `-uu`, `-uuu`)
- Status: ✅
- Expected behavior: Progressively disables ignore files (`-u`), hidden file filtering (`-uu`), and binary filtering (`-uuu`).
- Actual behavior: Operates as expected.
- Prompt gap: None.

### Invert Match (`-v`)
- Status: ⚠️
- Expected behavior: Prints non-matching lines without inserting `--` group separators unless context flags are provided.
- Actual behavior: Filters non-matching lines correctly, but inserts `--` group separators between non-adjacent matches.
- Prompt gap: Prompt did not clarify that `--` context separators must only be printed when context flags (`-A`, `-B`, `-C`, `--passthru`) are active.

### Context Lines (`-A`, `-B`, `-C`)
- Status: ⚠️
- Expected behavior: Displays before/after context lines with `--` separators between non-adjacent match groups. Default search (without context flags) must never print `--`.
- Actual behavior: Context lines display correctly, but `--` separator is printed even when no context flags are supplied.
- Prompt gap: Prompt failed to state that context separators are gated on context flags being enabled.

### Multiple Patterns & Pattern File (`-e`, `-f`)
- Status: ⚠️
- Expected behavior: Combines patterns from `-e` flags and `-f` files, printing matching lines.
- Actual behavior: Patterns are combined and matched correctly, but output includes unwanted `--` group separators.
- Prompt gap: Prompt did not clarify that default output without context flags must never output `--` separators.

### File Types (`-t`, `-T`, `--type-list`)
- Status: ⚠️
- Expected behavior: Filters files by type definitions; `--type-list` prints defined types and exits with code 0.
- Actual behavior: Filtering by file type works, but `--type-list` exits with code 1 instead of 0.
- Prompt gap: Prompt did not specify exit code 0 for non-search informational CLI actions.

### Version Output (`--version`, `-V`)
- Status: ⚠️
- Expected behavior: Prints `ripgrep <version>` followed by feature/revision details and exits with code 0.
- Actual behavior: Prints `rg 0.1.0`.
- Prompt gap: Prompt did not specify exact application name string format for `--version` output.

### Sorting (`--sort`, `--sortr`)
- Status: ⚠️
- Expected behavior: `--sort path` sorts paths in ascending order; `--sortr path` sorts in descending (reverse) order.
- Actual behavior: Both `--sort path` and `--sortr path` sort in ascending order.
- Prompt gap: Prompt listed `--sortr TYPE` without explicitly stating that `--sortr` specifies reverse (descending) sorting order.

### Gitignore & Ignore File Precedence
- Status: ⚠️
- Expected behavior: Respects ignore files with precedence order: `.rgignore` > `.ignore` > `.gitignore` > global gitignore. Negation rules (`!pattern`) in higher precedence files must un-ignore files ignored by lower precedence files.
- Actual behavior: Standard `.gitignore` works, but `.rgignore` negation rules fail to override `.gitignore`.
- Prompt gap: Prompt did not specify explicit precedence hierarchy among ignore files or rules for negation override.

### Capture Group Replacement (`-r`)
- Status: ❌
- Expected behavior: Replaces matched text with replacement string, expanding `$1`, `$2`, or `${name}` to corresponding regex capture groups.
- Actual behavior: Replaces matched text with literal `$1`.
- Prompt gap: Prompt specified `-r REPLACEMENT`, but omitted rules and syntax for capture group variable expansion.

### Binary Match Notification
- Status: ❌
- Expected behavior: When searching binary files (containing NUL bytes) without `-a`/`--text`, print `Binary file <path> matches` when a match occurs instead of outputting matching lines.
- Actual behavior: Suppresses matching lines but fails to print the `Binary file <path> matches` warning notice.
- Prompt gap: Prompt mentioned "skips binary files with a brief notification", but did not specify the exact notice string format (`Binary file <path> matches`) or target stream.

### Max Count Limit (`-m`)
- Status: ❌
- Expected behavior: Stops reading/matching a file after `NUM` matching lines have been output.
- Actual behavior: Continues scanning and outputs all matching lines in the file.
- Prompt gap: Prompt specified `-m NUM` in CLI table, but searcher execution did not enforce per-file match limits (Generator limitation / prompt ambiguity on early termination).

### File Listing (`--files`)
- Status: 开启 ❌
- Expected behavior: `rg --files [PATH...]` lists files that would be searched within the specified `PATH` arguments (or current directory if none provided), formatted relative to `PATH`, and exits with code 0.
- Actual behavior: Ignores positional `PATH` arguments, lists files from current working directory using CWD-relative paths, and exits with code 1.
- Prompt gap: Prompt listed `rg [OPTIONS] --files [PATH ...]`, but did not specify that positional `PATH` arguments apply to `--files`, how paths should be formatted relative to inputs, or that non-search modes exit with code 0.

### Vimgrep Output Format (`--vimgrep`)
- Status: ❌
- Expected behavior: Formats each match line as `{path}:{line}:{column}:{text}`.
- Actual behavior: Omits `{path}:` prefix, outputting `{line}:{column}:{text}`.
- Prompt gap: Prompt listed `--vimgrep` in table 3.2 without specifying the template `{path}:{line}:{column}:{text}`.

### Statistics Summary (`--stats`)
- Status: ❌
- Expected behavior: When `--stats` is provided, print search summary block at the end of output containing lines matched, matches found, files searched, and execution time.
- Actual behavior: Prints no statistics summary.
- Prompt gap: Prompt listed `--stats`: "Print statistics about the search", but omitted the required output format and mandatory metrics.

### Null Data Input (`--null-data`, `-0`)
- Status: ❌
- Expected behavior: Uses NUL byte (`\0`) instead of newline (`\n`) as line record terminator for matching.
- Actual behavior: Fails to match records delimited by NUL bytes.
- Prompt gap: Prompt listed `--null-data`, but did not detail record buffering and line terminator adjustment for searcher input.

### Code & Artifact Generation (`--generate`)
- Status: ❌
- Expected behavior: `rg --generate <KIND>` (where KIND is `man`, `complete-bash`, `complete-zsh`, `complete-fish`) generates documentation or shell completion scripts to stdout and exits with code 0.
- Actual behavior: Flag not recognized; exits with code 2.
- Prompt gap: Prompt referenced generation mode in Section 2.1 table, but omitted `--generate` flag specification in Section 3.

---

## Prompt Compliance
- Prompt said: "Structure argument parsing so that special modes (like `--help`, `--version`) can succeed...". The generator implemented `--help` and `--version`, but omitted `--generate`.
- Prompt said: "`-m NUM` / `--max-count NUM`: Limit the number of matching lines per file to NUM." The generator parsed `-m`, but failed to terminate per-file search loops when match count was reached.
- Prompt said: "`--sortr TYPE`: Sort search results in reverse order." The generator accepted `--sortr`, but performed ascending order sorting instead of descending.
- Prompt said: "`--vimgrep`: Output in Vim grep format." The generator accepted `--vimgrep`, but omitted file path prefixes from the output.
- Prompt said: "`-r REPLACEMENT`: Replace matches with REPLACEMENT." The generator accepted `-r`, but failed to expand regex capture group variables like `$1`.

---

## Critique

### Root-Cause Analysis of Behavioral Divergences

1. **Unwanted Context Group Separators (`--`)**
   - *Root cause*: Prompt gap. Section 3.2 stated: "`-A N`: Show N lines after each match. Print `--` between non-contiguous match groups." The prompt did not specify that `--` group separators must **ONLY** be emitted when context flags (`-A`, `-B`, `-C`, `--passthru`) are active. The generator unconditionally printed `--` between non-adjacent matches regardless of context settings.

2. **File Listing (`--files`) Path Handling & Non-Search Exit Codes**
   - *Root cause*: Prompt gap. Section 1 defined general exit codes (0 for match found, 1 for no match), but failed to state that non-search modes (`--files`, `--type-list`, `--help`, `--version`, `--generate`) must exit with code 0 upon success. Furthermore, Section 3.4 defined `--files` without explicitly stating that positional `PATH` arguments must restrict the file walk and determine path formatting.

3. **Missing Capture Group Replacement (`-r`)**
   - *Root cause*: Prompt gap. Section 3.2 defined `-r REPLACEMENT` as "Replace matches with REPLACEMENT", but provided no specification for syntax or expansion of capture group references (`$1`, `${name}`).

4. **Missing `--stats` Summary Block Format**
   - *Root cause*: Prompt gap. Section 3.2 listed `--stats`: "Print statistics about the search", but did not define the output block structure or required fields (matched lines, total matches, files searched, time elapsed).

5. **Omission of Artifact Generation (`--generate`)**
   - *Root cause*: Prompt gap. Section 2.1 briefly mentioned "generation mode", but Section 3 completely omitted the `--generate <KIND>` flag definition (`man`, `complete-bash`, `complete-zsh`, `complete-fish`).

6. **Ignore File Precedence Hierarchy**
   - *Root cause*: Prompt gap. Section 1 noted that ripgrep respects `.gitignore`, `.ignore`, and `.rgignore`, but omitted the exact priority ordering (`.rgignore` > `.ignore` > `.gitignore`) and how un-ignore/negation rules (`!pattern`) in higher priority files override lower priority files.

7. **Binary Match Warning Specification**
   - *Root cause*: Prompt gap. Section 1 mentioned "skips binary files with a brief notification", but Section 3 omitted the explicit notice format (`Binary file <path> matches`) and output channel.

8. **Vimgrep Format Template**
   - *Root cause*: Prompt gap. Section 3.2 listed `--vimgrep` without providing the required string template `{path}:{line}:{column}:{text}`.

9. **Reverse Sorting Specification (`--sortr`)**
   - *Root cause*: Generator limitation / minor prompt ambiguity. The prompt listed `--sortr TYPE` as reverse sort, but the generator mapped `--sortr` to the same ascending sort handler as `--sort`.

10. **Max Count Limit Enforcement (`-m`)**
    - *Root cause*: Generator limitation. The prompt explicitly defined `-m NUM`, but the generator's search sink failed to increment and evaluate per-file match counts to stop iteration.

---

## Top Learnings

1. **Explicitly gate context group separators on context flags**:
   - *Prompt gap*: Specify that `--` group separators must **ONLY** be printed between non-contiguous matches when context lines (`-A`, `-B`, `-C`) or `--passthru` are explicitly enabled, and **NEVER** in default search mode.

2. **Specify non-search exit code behavior and positional argument handling for `--files`**:
   - *Prompt gap*: Explicitly state that non-search/informational commands (`--files`, `--type-list`, `--help`, `--version`, `--generate`) must exit with code 0 on success. Clarify that `rg --files [PATH...]` must walk the specified `PATH` arguments and format output paths relative to those input paths.

3. **Define replacement string variable expansion syntax for `-r`**:
   - *Prompt gap*: Explicitly specify that replacement strings in `-r` / `--replace` support `$1`, `$2`, ..., `$N` and `${name}` variable expansion for regex capture groups.

4. **Define output format and required fields for `--stats`**:
   - *Prompt gap*: Provide the exact text template and metrics for `--stats` summary output (showing count of matches, matched lines, files searched, and elapsed time).

5. **Fully specify `--generate` CLI flag and supported target kinds**:
   - *Prompt gap*: Add `--generate <KIND>` to CLI flag specifications with supported targets: `man`, `complete-bash`, `complete-zsh`, `complete-fish`, and require exit code 0 on completion.

6. **Specify ignore file precedence hierarchy and negation override rules**:
   - *Prompt gap*: Explicitly specify the precedence order: `.rgignore` > `.ignore` > `.gitignore` > global gitignore, and state that negation rules (`!pattern`) in higher precedence files override ignore rules in lower precedence files.

7. **Specify exact text format for binary file match notices**:
   - *Prompt gap*: Specify that when a binary file (containing NUL bytes) matches without `-a`/`--text`, the tool must print `Binary file <path> matches` instead of line content.

8. **Provide exact line template for `--vimgrep`**:
   - *Prompt gap*: Explicitly define `--vimgrep` output template as `{path}:{line}:{column}:{text}`.
