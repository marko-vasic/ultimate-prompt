# Diff Report — Iteration 0

## Summary
- **Files in original**: 58 Rust source files across 10 workspace crates + `build.rs`
- **Files produced**: 11 Rust source files (4 in `src/`, 7 single `lib.rs` files in `crates/`)
- **Equivalent**: 1
- **Partial**: 3
- **Divergent**: 7
- **Missing**: 48
- **Build status**: PASS (0 compilation errors, 8 dead code warnings)
- **Equivalence tests**: 70/89 passed (19 failed)

---

## Missing Files
The original ripgrep codebase is architected as a modular workspace with 10 distinct crates and modular internal structure. The generated implementation collapsed each crate into a single `lib.rs` file and missed several crates entirely:

- `build.rs`: Top-level build script (generates man page / shell completions at build time).
- `crates/core/`: The primary binary crate in original ripgrep (contains `main.rs`, `search.rs`, `haystack.rs`, `messages.rs`, and modular flags parser in `flags/`).
- `crates/pcre2/`: PCRE2 regex matcher integration (`lib.rs`, `matcher.rs`, `error.rs`).
- `crates/grep/`: Top-level facade crate (`lib.rs` re-exporting matcher, searcher, printer, regex).
- `crates/printer/src/*`: Modular printer files (`standard.rs`, `summary.rs`, `json.rs`, `color.rs`, `path.rs`, `counter.rs`, `stats.rs`, `util.rs`, `macros.rs`).
- `crates/searcher/src/*`: Modular searcher files (`line_buffer.rs`, `lines.rs`, `searcher/`, `sink.rs`, `testutil.rs`).
- `crates/ignore/src/*`: Modular ignore files (`walk.rs`, `dir.rs`, `gitignore.rs`, `overrides.rs`, `types.rs`, `default_types.rs`, `pathutil.rs`).
- `crates/globset/src/*`: Modular globset files (`glob.rs`, `pathutil.rs`, `fnv.rs`, `serde_impl.rs`).
- `crates/cli/src/*`: Modular CLI helper files (`process.rs`, `hostname.rs`, `decompress.rs`, `human.rs`, `escape.rs`, `pattern.rs`, `wtr.rs`).

---

## Extra Files
- `src/main.rs`, `src/args.rs`, `src/search.rs`, `src/logger.rs`: Placed directly in root binary package rather than under `crates/core/`.
- `instructions/prompt.md`: Embedded prompt copy in the implementation directory.

---

## Per-File Assessment

### Root & Binary Crate
- **`src/main.rs`**
  - Status: ❌
  - Summary: Serves as the binary entry point. Implemented `try_main()` and `main()`, but incorrectly returns exit code `1` (failure/no-match) for informational modes like `--type-list` and `--files`. Missing `--generate` CLI action handling.
  - Key gaps: Prompt did not specify exit code contract for non-search/informational CLI flags.

- **`src/args.rs`**
  - Status: ⚠️
  - Summary: Parses CLI options using `lexopt`. Covers most flags, but omits `--generate` (man pages/completions) and has bugs in reverse sorting (`--sortr`), `--vimgrep` options, and capture group replacement parsing (`-r`).
  - Key gaps: Missing flag definitions for `--generate`; missing reverse ordering flag propagation to `ignore::WalkBuilder`.

- **`src/search.rs`**
  - Status: ⚠️
  - Summary: Orchestrates searching, walk builder, single and multi-threaded execution. Returns `Ok(false)` for `--type-list` and `--files` modes, causing main to return exit code 1. Relative path formatting for file listing (`--files`) prints current working directory paths instead of user-specified root relative paths.
  - Key gaps: Path relative normalization logic missing in file walker.

### Workspace Crates

- **`crates/grep-printer` (Original: `crates/printer`)**
  - Status: ❌
  - Summary: Re-implemented as a monolithic `lib.rs`. Contains critical behavioral bugs: prints context group separators (`--`) between non-contiguous matches even when context flags (`-A`, `-B`, `-C`) are NOT requested; omits capture group replacement in `-r`; omits statistics summary printing for `--stats`; fails to format `--vimgrep` output with filename prefixes.
  - Key gaps: Context separator logic must check if context lines were explicitly requested (`before > 0 || after > 0`).

- **`crates/ignore` (Original: `crates/ignore`)**
  - Status: ⚠️
  - Summary: Re-implemented as a monolithic `lib.rs`. Correctly handles standard `.gitignore` and hidden files in most cases, but `.rgignore` precedence over `.gitignore` fails, and dead code analysis shows unused fields and methods.
  - Key gaps: Ignore hierarchy priority order (`.rgignore` > `.ignore` > `.gitignore`) is incomplete.

- **`crates/grep-searcher` (Original: `crates/searcher`)**
  - Status: ⚠️
  - Summary: Implements buffer searching logic. Max count (`-m`) stopping is ignored during stream consumption, binary detection message formatting (`Binary file matches`) is missing, and `--null-data` NUL-byte line splitting is incomplete.
  - Key gaps: Missing max-count termination check in sink wrapper; missing binary match notice callback.

- **`crates/grep-matcher` / `crates/grep-regex` (Original: `crates/matcher`, `crates/regex`)**
  - Status: ⚠️
  - Summary: Basic regex matching works, but Regex replacement with capture groups (`$1`, `$2`) passes literal strings through instead of expanding captures.
  - Key gaps: Capture replacement expansion specification missing from prompt.

- **`crates/globset` (Original: `crates/globset`)**
  - Status: ✅
  - Summary: Equivalent wrapper implementation for basic glob matching functionality.

- **`crates/grep-cli` (Original: `crates/cli`)**
  - Status: ❌
  - Summary: Simplified shell helper crate; missing hostname detection, decompression handling, and process signal management present in original.

---

## Build Errors
- **Compilation**: PASS (0 errors).
- **Warnings**: 8 dead code warnings in `ignore` (`message`, `ErrorKind::Parse`, `parse`, `selected`, `original`, `root`, `from_walkdir`, `sort_by`).

---

## Test Failures (19 / 89 failed)

1. **`T001: Simple pattern match`**: Output contains unexpected `--` context group separator lines between non-adjacent matching lines when no context lines were requested.
2. **`T020: Invert match`**: Output contains unexpected `--` context group separator lines.
3. **`T101: Capture group replace`**: Literal `$1` printed instead of expanding regex capture group 1 (`foo$1bar` vs `foo123bar`).
4. **`T142: rgignore precedence`**: `.rgignore` rules failed to take precedence over `.gitignore`.
5. **`T150: Binary warning`**: Missing `binary file <name> matches` notice on matching binary data.
6. **`T160: Max count`**: `-m 2` failed to limit output line count to 2.
7. **`T190: Multiple patterns`**: Extra `--` context group separator lines printed.
8. **`T200: Patterns from file`**: Extra `--` context group separator lines printed.
9. **`T210: List files`**: `--files` output path prefixing printed `./working_dir/...` instead of relative paths.
10. **`T211: File list gitignore`**: `--files` gitignore path output formatting mismatched.
11. **`T310: Version`**: Printed `rg 0.1.0` instead of `ripgrep 0.1.0`.
12. **`T360: Vimgrep format`**: `--vimgrep` printed `1:7:world` missing filename prefix `test.txt:`.
13. **`T371: Reverse sort path`**: `--sortr path` output sorted ascending instead of descending.
14. **`T390: Stats matches`**: `--stats` flag omitted summary statistics output block.
15. **`T390: Stats matched lines`**: Statistics block missing.
16. **`T390: Stats files`**: Statistics block missing.
17. **`T410: Null data`**: `--null-data` failed to match/split lines by NUL bytes.
18. **`T430: Man page content`**: `--generate man` flag unrecognized (exited 2).
19. **`T431: Generate bash exit code`**: `--generate complete-bash` unrecognized (exited 2).

---

## Critique

### What Went Wrong?

1. **Unconditional Context Group Separator (`--`)**:
   - *Generator behavior*: The printer unconditionally emitted `--` between non-contiguous line matches whenever line numbers differed by more than 1.
   - *Root Cause in Prompt*: The prompt instructed the model to print `--` between disjoint match groups, but failed to specify that `--` MUST ONLY be emitted when explicit context lines (`-A`, `-B`, `-C`) are enabled by the user.

2. **Informational Flags Exit Code Contract**:
   - *Generator behavior*: `main()` returned exit code `1` for `--type-list` and `--files` because `search::run()` returned `Ok(false)` (signifying no pattern matches found).
   - *Root Cause in Prompt*: The prompt stated "Exit code 1 when no matches are found", but failed to clarify that utility modes (`--type-list`, `--files`, `--help`, `--version`) must exit with status `0`.

3. **Missing Crate Names & Monolithic File Flattening**:
   - *Generator behavior*: Crate directory names were renamed (e.g., `grep-printer` instead of `printer`, `grep-matcher` instead of `matcher`, `src/` root instead of `crates/core/`), and multi-file module layouts were collapsed into single `lib.rs` files.
   - *Root Cause in Prompt*: The prompt lacked an explicit workspace directory tree specification mapping original crate directory paths and module file structures.

4. **Regex Capture Replacement & Output Formatting Gaps**:
   - *Generator behavior*: `-r '$1'` printed `$1` literally. `--stats` omitted summary blocks. `--sortr` inverted sorting logic.
   - *Root Cause in Prompt*: The prompt mentioned flags high-level without detailing exact expansion rules (e.g., regex `$1` replacement syntax, exact `--stats` summary output format string).

5. **CLI Generation & Binary Warnings**:
   - *Generator behavior*: `--generate` option was completely omitted from argument parsing. Binary file match notices were omitted.
   - *Root Cause in Prompt*: The prompt completely omitted requirements for `--generate (man|complete-bash)` and binary file notice suppression/warning behavior.

---

## Top Learnings

1. **Specify Exit Code Rules per CLI Mode**: Explicitly specify that informational/listing flags (`--type-list`, `--files`, `--generate`, `--help`, `--version`) MUST exit with status `0`, and exit status `1` ONLY applies to standard search operations with 0 matches.
2. **Clarify Context Separator (`--`) Emission Conditions**: Explicitly state that `--` context group separators are ONLY printed when context lines (`-A`, `-B`, or `-C`) are explicitly requested (`> 0`).
3. **Provide Full Workspace Crate Architecture & Directory Map**: Provide exact crate directory names (`crates/core`, `crates/printer`, `crates/matcher`, `crates/searcher`, `crates/cli`, `crates/regex`, `crates/ignore`, `crates/globset`, `crates/pcre2`) and file trees so the generator does not invent new crate names or collapse modules into monolithic `lib.rs` files.
4. **Detail Advanced Feature Output Contracts**: Add explicit requirements for capture group expansion in `-r`, `--stats` summary text formatting, `--vimgrep` output format (`path:line:col:line_content`), and `--generate` flag handling.
5. **Ignore Rules Precedence Hierarchy**: Explicitly define the ignore override sequence: CLI overrides > `.rgignore` > `.ignore` > `.gitignore` > global ignore files.
