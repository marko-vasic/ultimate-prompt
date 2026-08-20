# Diff Report — Iteration 2

## Summary
- Build status: PASS
- Equivalence tests: 79/89 passed (10 failed)
- Capabilities equivalent: 38
- Capabilities partial: 4
- Capabilities missing/broken: 4

---

## Build Errors
None. The generated codebase compiled cleanly using Cargo (`cargo build`) in 10.91s.
- **Warnings**: 32 compiler warnings across crates (10 in `grep-searcher`, 8 in `grep-printer`, 7 in `ignore`, 5 in `ripgrep` binary, 1 in `globset`, 1 in `grep-cli`), primarily unused helper methods (`write_matched_line`, `detect_binary`, `count_matches_in_line`, `from_path`), dead struct fields, and unused variables.
- **Internal Tests**: 45/45 integration tests in `tests/integration.rs` passed cleanly.

---

## Test Failures

### 1. `T051: Count matches`
- **Error summary**: `--count-matches` counted the number of matching lines (2) rather than the total count of non-overlapping regex match occurrences across all lines (3 in `foofoo` + `foo`).
- **Expected output**:
  ```
  3
  ```
- **Actual output**:
  ```
  2
  ```

### 2. `T101: Capture group replace`
- **Error summary**: `-r '$1'` literal string replacement was performed rather than expanding regex capture group references (`$1`, `$2`, etc.).
- **Expected output**:
  ```
  foo123bar
  ```
- **Actual output**:
  ```
  foo$1bar
  ```

### 3. `T150: Binary warning`
- **Error summary**: Binary match detection output `Binary file <path> matches` with path prefix and uppercase `B`, rather than single-file un-prefixed format `binary file matches (found "\0" byte around offset N)`. The test string assertion specifically checked for substring `"binary file matches"`.
- **Expected output**: Contains `binary file matches`
- **Actual output**:
  ```
  Binary file /tmp/.../t150/test.bin matches
  ```

### 4. `T210: List files`
- **Error summary**: Positional directory path passed to `rg --files <PATH>` was parsed by Clap into `pattern` rather than `paths`. Because `paths` remained empty, `rg` defaulted to listing files in the current working directory (`.`) instead of the target directory.
- **Expected output**:
  ```
  bar.rs
  foo.txt
  sub/baz.py
  ```
- **Actual output**:
  ```
  ./.git/COMMIT_EDITMSG
  ./.git/FETCH_HEAD
  ... (files in current repo root)
  ```

### 5. `T211: File list gitignore`
- **Error summary**: (Cascading failure from T210) `rg --files` listed files from `.` instead of the fixture directory containing `.gitignore`.
- **Expected output**:
  ```
  keep.txt
  ```
- **Actual output**:
  ```
  ./working_dir/ripgrep/UNLICENSE
  ./working_dir/ripgrep/ci/...
  ```

### 6. `T310: Version`
- **Error summary**: `--version` output `rg 14.1.1` instead of `ripgrep 14.1.1` (missing full binary name `ripgrep`).
- **Expected output**: Contains `ripgrep`
- **Actual output**:
  ```
  rg 14.1.1
  ```

### 7. `T331: CLI overrides config`
- **Error summary**: When `RIPGREP_CONFIG_PATH` contained `--case-sensitive` and CLI arguments contained `-i` (`--ignore-case`), the config file flag took precedence because boolean resolution prioritized `if args.case_sensitive` over `args.ignore_case` without tracking argument order or using Clap override rules.
- **Expected output**:
  ```
  Hello
  hello
  HELLO
  ```
- **Actual output**:
  ```
  hello
  ```

### 8. `T360: Vimgrep format`
- **Error summary**: `--vimgrep` was mistakenly routed to `write_only_matching` instead of printing the full matching line content, outputting `PATH:LINE:COL:MATCH` (`test.txt:1:7:world`) instead of `PATH:LINE:COL:LINE_CONTENT` (`test.txt:1:7:hello world`).
- **Expected output**:
  ```
  test.txt:1:7:hello world
  ```
- **Actual output**:
  ```
  test.txt:1:7:world
  ```

### 9. `T410: Null data`
- **Error summary**: In `PrinterSink.matched()`, `binary_behavior` checked the CLI default (`BinaryBehavior::Quit`) and aborted search upon detecting `\0` bytes in the matched slice, terminating `--null-data` searches with no output.
- **Expected output**:
  ```
  bar
  ```
- **Actual output**:
  *(empty)*

### 10. `T430: Man page content`
- **Error summary**: `--generate man` produced man page header `.TH rg 1 ...` with lowercase command title `rg` rather than `.TH RG 1` / `.TH RIPGREP 1`.
- **Expected output**: Contains `.TH RG`
- **Actual output**:
  ```
  .TH rg 1  "rg 14.1.1"
  ```

---

## Capability Assessment

### 1. Match Occurrence Counting (`--count-matches` vs `-c` / `--count`)
- **Status**: ⚠️ Partial
- **Expected behavior**: `-c` / `--count` prints the number of *lines* containing matches. `--count-matches` prints the total number of *individual match occurrences* across all lines (e.g. 2 matches on line 1 + 1 match on line 3 = 3 matches total).
- **Actual behavior**: `--count-matches` incremented a match counter once per matching line in `PrinterSink.matched()`, outputting 2 instead of 3.
- **Prompt gap**: The prompt mentions `--count-matches` in the flags table, but needs an explicit behavioral specification: *"Distinguish `--count` from `--count-matches`: `-c`/`--count` outputs the number of matching lines. `--count-matches` outputs the total count of individual non-overlapping regex match occurrences across all lines in each file."*

### 2. Capture Group Expansion in Replacement (`-r` / `--replace`)
- **Status**: ❌ Broken
- **Expected behavior**: When `-r REPLACEMENT` is provided, `$1`, `$2`, `${name}`, etc., in the replacement text must be expanded to the corresponding captured subgroups from the regex match.
- **Actual behavior**: `PrinterSink` performed literal substring replacement or output the raw replacement string with unexpanded `$1` tokens.
- **Prompt gap**: The prompt should state: *"When `-r` / `--replace` is provided, perform regex capture replacement. Variables like `$1`, `$2`, `${name}` in the replacement template must be expanded to the contents of the corresponding regex capture groups for each match."*

### 3. Binary Match Warning Format (`--text`, default binary quitting)
- **Status**: ⚠️ Partial
- **Expected behavior**: When a binary file matches in normal mode: if searching a single file without `-H`, emit `binary file matches (found "\0" byte around offset N)`; if searching multiple files or with `-H`, emit `Binary file <path> matches (found "\0" byte around offset N)`.
- **Actual behavior**: The generator emitted `Binary file <path> matches` with full path prefix even for single-file searches, which missed the un-prefixed format expected in standard single-file search.
- **Prompt gap**: Specify the exact binary warning message format: *"When a binary file matches in default search mode, output `binary file matches (found "\0" byte around offset N)` when searching a single file or stdin without `-H`, or `Binary file <path> matches (found "\0" byte around offset N)` when searching multiple files or with `-H`."*

### 4. Positional Argument Handling in File Listing (`--files`)
- **Status**: ❌ Broken
- **Expected behavior**: `rg --files [PATH...]` lists all files that would be searched within the specified `PATH` directories. When `--files` is active, NO regex pattern argument is expected or consumed — all positional arguments are path roots.
- **Actual behavior**: Clap parsed the first positional argument into `pattern`, leaving `paths` empty, which caused `rg` to default to listing files in `.`.
- **Prompt gap**: The prompt must explicitly specify: *"When `--files` is provided, ripgrep does not search for a pattern. All positional command-line arguments are search paths / directories to list. If no positional arguments are provided, default to listing the current directory (`.`)."*

### 5. CLI Override Precedence Over Config Files (`RIPGREP_CONFIG_PATH`)
- **Status**: ⚠️ Partial
- **Expected behavior**: Flags supplied directly on the CLI must always override conflicting flags loaded from `RIPGREP_CONFIG_PATH`. For example, if the config file specifies `--case-sensitive` and the CLI specifies `-i`, search must be case-insensitive.
- **Actual behavior**: Config args and CLI args were merged and parsed with Clap, but mutually exclusive boolean flags (like `case_sensitive` and `ignore_case`) were evaluated with static `if args.case_sensitive ... else if args.ignore_case` branch order rather than tracking the last-specified flag or setting Clap argument override relationships.
- **Prompt gap**: The prompt should specify: *"Command-line flags must take precedence over configuration file flags (`RIPGREP_CONFIG_PATH`). When mutually exclusive flags (e.g. `--case-sensitive` / `-i` / `-S`, `--heading` / `--no-heading`, `--line-number` / `-N`) appear in both config file and CLI, the last flag specified on the command line must win."*

### 6. Vimgrep Format Line Output (`--vimgrep`)
- **Status**: ❌ Broken
- **Expected behavior**: `--vimgrep` outputs each match in the format `PATH:LINE:COLUMN:LINE_CONTENT` where `LINE_CONTENT` is the complete matching line. If a line contains multiple matches, emit one line per match with its respective column number and the complete line content.
- **Actual behavior**: The generator conflated `--vimgrep` with `--only-matching`, outputting only the matched substring instead of the entire line content.
- **Prompt gap**: The prompt should specify: *"With `--vimgrep`, output format must be `PATH:LINE:COLUMN:LINE_CONTENT`. The line content must be the entire line containing the match (not just the matched substring). If multiple matches occur on the same line, output that line once for each match with the corresponding 1-based column number."*

### 7. Null-Data Mode Printer Integration (`--null-data`)
- **Status**: ❌ Broken
- **Expected behavior**: `--null-data` sets the line terminator to `\0` (NUL). It must allow searching and printing lines containing NUL bytes without binary detection quitting or suppressing the output.
- **Actual behavior**: `SearcherBuilder` configured `LineTerminator::Byte(b'\0')` and disabled binary detection on the searcher, but `PrinterSink` maintained `binary_behavior: BinaryBehavior::Quit` and explicitly aborted on seeing `\0` in `mat.bytes()`.
- **Prompt gap**: The prompt should specify: *"When `--null-data` is enabled, the line terminator is NUL (`\0`) and binary detection is completely disabled across both searcher and printer. Matched lines separated by NUL bytes must be emitted with NUL terminators."*

### 8. Program Name and Man Page Header (`--version`, `--generate man`)
- **Status**: ⚠️ Partial
- **Expected behavior**: `--version` first line should be `ripgrep <VERSION>` (not `rg <VERSION>`). `--generate man` header should start with `.TH RG 1` or `.TH RIPGREP 1`.
- **Actual behavior**: Clap command name was set to `rg`, generating `rg 14.1.1` and `.TH rg 1`.
- **Prompt gap**: Specify: *"The application name is `ripgrep`. `rg --version` must output `ripgrep <VERSION>` as its first line. Man page generation (`--generate man`) must use uppercase program title `RG`."*

---

## Prompt Compliance
- **Single-File Filename Defaults**: Compliant. In iteration 2, the generator successfully suppressed filename prefixes on single-file searches and stdin searches by default.
- **Context Separators (`--`)**: Compliant. The generator only emitted `--` when `-A`, `-B`, or `-C` was requested (> 0) and non-adjacent match groups were encountered.
- **Ignore Stack & Traversal (`.gitignore`, `.ignore`, `-g`, `-t`)**: Compliant. The directory traversal engine correctly applied layered `.gitignore`, `.ignore`, glob overrides, and file type filters.
- **Sorting (`--sort`, `--sortr`)**: Compliant. Directory traversal sorted entries lexicographically when sort flags were supplied.
- **Stdin TTY Detection**: Compliant. Piped stdin vs current directory traversal worked properly.
- **JSON Output & Submatches**: Compliant. JSON Lines schema output `begin`, `match` (with `submatches`), and `end` objects.
- **Multiline (`-U`)**: Compliant. Multiline regex search across newline boundaries executed successfully.
- **Stats (`--stats`)**: Compliant. Aggregate statistics summary printed correctly at the end of search.
- **Non-compliant Areas**:
  1. `--files` positional arguments were misrouted to `pattern`.
  2. `--vimgrep` output only matching substrings rather than full line content.
  3. `-r` capture group replacement expanded literally rather than substituting `$1`.
  4. `--null-data` was blocked by printer-level binary detection checks.
  5. `--count-matches` counted matching lines instead of total match occurrences.
  6. Config file vs CLI flag override precedence was lost due to static boolean if-else chains.

---

## Critique

Iteration 2 represents major progress: **equivalence test pass rate rose from 38.2% (34/89) in iteration 1 to 88.8% (79/89)**. All 8 workspace crates compiled cleanly, and all 45 internal integration tests passed.

Root causes of the remaining 10 failures fall into four distinct categories:

### 1. Positional CLI Argument Ambiguity (`--files` vs normal search)
When `--files` is passed, the CLI invocation has no pattern argument (`rg --files [PATH...]`). Because Clap was configured with `pattern: Option<String>` and `paths: Vec<PathBuf>`, Clap consumed the first positional argument into `pattern`, leaving `paths` empty and causing `rg` to default to `.`.
*Root cause in prompt*: The prompt did not explicitly document the argument grammar distinction when `--files` is active.

### 2. Output Formatting Conflation (`--vimgrep` vs `--only-matching`)
The generator conflated the semantics of `--vimgrep` (Vim quickfix errorformat `FILE:LINE:COL:LINE_CONTENT`) with `--only-matching` (`-o`), directing vimgrep through the substring-only printer branch.
*Root cause in prompt*: While the prompt specified the format string, it did not explicitly warn against truncating the line content to the matched text.

### 3. Feature Flag Interactions Across Abstraction Layers (`--null-data` and `-r`)
- In `--null-data`, the searcher layer was updated to disable binary detection, but the printer layer (`PrinterSink`) retained a separate binary check that aborted upon encountering NUL bytes in the match slice.
- In `-r` / `--replace`, the printer performed literal text replacement without passing capture group indices from the regex matcher.
*Root cause in prompt*: The prompt must explicitly mandate that `--null-data` disables binary detection across all layers (searcher and printer), and specify regex capture group replacement (`$1`, `$2`, `${name}`).

### 4. CLI Argument Precedence & Mutually Exclusive Flags
When configuration file arguments are prepended to CLI arguments, standard Clap parsing with separate boolean fields (`ignore_case: bool`, `case_sensitive: bool`) loses the order of appearance. Static `if args.case_sensitive ... else if args.ignore_case` logic caused config file flags to take precedence over subsequent CLI overrides.
*Root cause in prompt*: The prompt needs an explicit rule stating that CLI arguments always override config file arguments, and that the last specified flag among mutually exclusive sets (`-i`/`-s`/`-S`, `--heading`/`--no-heading`, `-n`/`-N`) takes precedence.

---

## Top Learnings

1. **Positional Argument Semantics with `--files`**: The prompt must state: *"When `--files` is supplied, no pattern argument is accepted. Every positional argument is interpreted as a path root to list files from. If no paths are given, default to listing the current directory (`.`)."*
2. **Vimgrep Full Line Content**: The prompt must state: *"With `--vimgrep`, print `PATH:LINE:COLUMN:LINE_CONTENT` where `LINE_CONTENT` is the complete matching line. Never truncate `LINE_CONTENT` to only the matching substring. If a line contains multiple matches, emit a separate line for each match with its corresponding 1-based column number."*
3. **Count Lines vs Count Match Occurrences**: The prompt must specify: *"`-c` / `--count` prints the number of matching lines. `--count-matches` prints the total number of non-overlapping regex match occurrences across all lines in the file."*
4. **Capture Group Expansion in Replacement**: The prompt must state: *"When `-r REPLACEMENT` / `--replace` is used, replace matched spans using regex capture groups, expanding `$1`, `$2`, `${name}`, etc."*
5. **Null-Data Disables Binary Quitting in Both Searcher and Printer**: The prompt must specify: *"When `--null-data` is enabled, binary detection is completely disabled in both searcher and printer sinks. Lines delimited by NUL (`\0`) must be output normally."*
6. **CLI Flag Precedence Over Config Files**: The prompt must specify: *"Flags on the command line must override flags from `RIPGREP_CONFIG_PATH`. For mutually exclusive flags (`-i`/`-s`/`-S`, `--heading`/`--no-heading`, `-n`/`-N`, `-c`/`--count-matches`), the last flag specified on the command line takes precedence."*
7. **Single-File Binary Warning Format**: The prompt must state: *"When a binary file matches, output `binary file matches (found "\0" byte around offset N)` when searching a single file without `-H`, and `Binary file <path> matches (found "\0" byte around offset N)` when searching multiple files or with `-H`."*
8. **Program Identity in Version Output**: The prompt must state: *"`rg --version` must print `ripgrep <VERSION>` on the first line."*
