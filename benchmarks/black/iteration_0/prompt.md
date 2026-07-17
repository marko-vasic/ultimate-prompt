# Ultimate Prompt v[0] — Black: The Uncompromising Code Formatter

## 1. Project Identity & Philosophy

Build a deterministic, opinionated Python code formatter called **Black**. The core
philosophy is to eliminate style debates by offering minimal configuration. Black takes
away freedom of choice in formatting so that developers can focus on content.

**Key invariants:**

- **Deterministic**: identical input plus identical configuration must always produce
  identical output, regardless of platform or execution environment.
- **Idempotent**: formatting already-formatted code must produce no changes.
- **Safe**: the formatted output must be semantically equivalent to the input — the
  Abstract Syntax Tree must be preserved.
- **Opinionated**: the tool deliberately provides very few knobs. Most formatting
  decisions are not configurable.

**Licensing:** MIT.

**Runtime requirements:** Python >= 3.10 to run the formatter itself. The formatter must
be capable of formatting Python source code targeting any Python version from 3.3
through 3.15.

---

## 2. Command-Line Interface

The primary entry point is invoked as:

```
black [OPTIONS] SRC ...
```

where `SRC` is one or more files or directories.

### 2.1 Input Modes

| Option | Behavior |
|---|---|
| `SRC ...` (positional) | One or more file paths or directories to format. Directories are searched recursively. |
| `-c, --code TEXT` | Format a string of code passed directly on the command line. Output printed to stdout. |
| stdin | When `-` is passed as `SRC`, read from stdin. Use `--stdin-filename` to provide a virtual filename for filtering purposes. |

### 2.2 Formatting Options

| Option | Default | Description |
|---|---|---|
| `-l, --line-length INTEGER` | 88 | Maximum number of characters per line. |
| `-t, --target-version VERSION` | Auto-detect | Python version(s) the formatted code must support. Accepts `py33` through `py315`. May be specified multiple times. When omitted, inferred from project metadata or detected per-file from syntax usage. |
| `--pyi` | off | Treat all input files as `.pyi` typing stub files, regardless of actual extension. |
| `--ipynb` | off | Treat all input files as Jupyter Notebooks, regardless of actual extension. |
| `-x, --skip-source-first-line` | off | Ignore the first line of each source file (e.g., a shebang `#!/usr/bin/env python`). The first line is preserved verbatim in the output. |
| `-S, --skip-string-normalization` | off | Do not normalize string quotes or prefixes. |
| `-C, --skip-magic-trailing-comma` | off | Do not treat trailing commas as a signal to force multi-line layout. |
| `--preview` | off | Enable style changes that are candidates for the next major release. These may be disruptive. |
| `--unstable` | off | Enable style changes that have known bugs. Implies `--preview`. |
| `--enable-unstable-feature NAME` | none | Cherry-pick individual unstable/preview features by name. Requires `--preview`. |

Available unstable/preview feature names:

- `string_processing` (unstable)
- `hug_parens_with_braces_and_square_brackets` (unstable)
- `wrap_comprehension_in`
- `simplify_power_operator_hugging`
- `wrap_long_dict_values_in_parens`
- `fix_if_guard_explosion_in_case_statement`
- `pyi_overload_group_blank_lines`
- `fix_unnecessary_parens_in_indexed_assignment`
- `pyi_blank_line_before_decorated_class`
- `pyi_blank_line_after_function_docstring`
- `hug_comparator`
- `parenthesize_tuple_in_yield`

### 2.3 Output Modes

| Option | Behavior |
|---|---|
| *(default)* | Rewrite files in place. |
| `--check` | Do not write files. Exit code 0 if no changes needed, 1 if changes would be made, 123 on internal error. |
| `--diff` | Print a unified diff to stdout instead of writing files. |
| `--color / --no-color` | Enable or disable ANSI-colored diff output (only meaningful with `--diff`). Colors are automatically disabled when the `NO_COLOR` environment variable is set. |

### 2.4 Safety Options

| Option | Default | Behavior |
|---|---|---|
| `--safe` | **enabled** | After formatting, parse both the original and formatted code into ASTs and verify equivalence. Also verify stability (re-formatting produces the same output). |
| `--fast` | disabled | Skip the AST equivalence and stability checks. |
| `--required-version TEXT` | none | Abort if the running version of Black does not match. Accepts a full version string or just a major version number. |

### 2.5 File Discovery Options

| Option | Default | Behavior |
|---|---|---|
| `--include TEXT` | `(\.pyi?\|\.ipynb)$` | Regex pattern for files to include during recursive search. |
| `--exclude TEXT` | See below | Regex for files/directories to exclude during recursive search. Overrides the built-in default. |
| `--extend-exclude TEXT` | none | Additional exclusion regex that supplements (does not replace) the default and `--exclude`. |
| `--force-exclude TEXT` | none | Exclusion regex applied even to files explicitly passed on the command line. Useful for editor integrations and pre-commit hooks. |
| `--stdin-filename TEXT` | none | Virtual filename to use when reading from stdin, so that `--force-exclude` and extension-based behavior work correctly. |

Default `--exclude` pattern:

```
/(\.direnv|\.eggs|\.git|\.hg|\.ipynb_checkpoints|\.mypy_cache|\.nox|\.pytest_cache|\.ruff_cache|\.tox|\.svn|\.venv|\.vscode|__pypackages__|_build|buck-out|build|dist|venv)/
```

### 2.6 Operational Options

| Option | Behavior |
|---|---|
| `--line-ranges START-END` | Format only the specified 1-based inclusive line range. May be specified multiple times. Cannot be combined with `--ipynb` or with multiple input files. Cannot be set via config file. |
| `-W, --workers INTEGER` | Number of parallel worker processes. Default: number of CPUs. Also settable via `BLACK_NUM_WORKERS` environment variable. |
| `-q, --quiet` | Suppress all non-critical output. |
| `-v, --verbose` | Show files that were not changed or were ignored. Show which config file is in use. |
| `--no-cache` | Do not read or write the formatting cache; always reprocess every file. |
| `--config FILE` | Read configuration from a specific file path instead of auto-discovering. |
| `--version` | Print version and exit. |
| `-h, --help` | Print help and exit. |

---

## 3. Configuration

### 3.1 Configuration File

Configuration is read from `pyproject.toml` under the `[tool.black]` section. All CLI
formatting options (except `--line-ranges`) have corresponding config keys. CLI
arguments always override config file values.

A JSON schema is provided for validating the `[tool.black]` section. This schema is
registered as a `validate_pyproject` entry point so that generic `pyproject.toml`
validators can discover it.

### 3.2 Project Root Discovery

The project root is found by searching upward from the first source path for:

1. A directory containing `.git` or `.hg`.
2. A `pyproject.toml` file that contains a `[tool.black]` section.

The first match wins. If no project-level config is found, a user-level config file is
checked at the platform-appropriate location (e.g., `~/.config/black` on Linux,
`~\.black` on Windows).

### 3.3 Target Version Inference

When no `--target-version` is specified:

1. If the project's `pyproject.toml` contains a PEP 621 `[project].requires-python`
   field, the supported Python versions are inferred from that specifier.
2. Otherwise, each file is individually analyzed: the formatter scans the parsed syntax
   tree for version-specific features (f-strings, pattern matching, `except*`, type
   parameters, etc.) and selects the minimum set of versions that support all features
   found.

---

## 4. Formatting Pipeline

### 4.1 Parsing

Source code is parsed into a **Concrete Syntax Tree** (CST) — not an Abstract Syntax
Tree. The CST preserves every token, all whitespace, and all comments. This is essential
because the formatter must reproduce non-code elements faithfully.

The parser is a custom extension of Python's `lib2to3` parser, enhanced to support all
Python syntax from version 3.0 through 3.15. Key capabilities:

- **Three grammar variants** covering the evolution of Python's syntax:
  - Pre-async-keywords grammar (Python 3.0–3.6, where `async`/`await` are not reserved).
  - Async-keywords grammar (Python 3.7–3.9).
  - Soft-keywords grammar (Python 3.10+, with `match`/`case`/`type` as context-sensitive
    keywords).
- **Soft keyword handling** via backtracking: when the parser encounters a token that
  could be either an identifier or a soft keyword, it tries both parses in parallel.
- **Modern syntax support**: match/case statements, type parameter lists, type defaults,
  t-strings, lazy imports.
- **Performance**: uses a Rust-based tokenizer for speed. Grammar tables are serialized
  for fast loading.
- **CST node structure**: interior nodes (nonterminals representing grammar productions)
  and leaf nodes (terminals representing tokens). Both store leading whitespace and
  comments in a prefix field.
- **Single-child optimization**: when a grammar production yields exactly one child node,
  the child is used directly instead of wrapping it.
- **Compilable** with ahead-of-time compilers (mypyc/Cython) for additional performance.

### 4.2 Format Directive Processing

Before formatting, the CST is scanned for format control comments:

- `# fmt: off` / `# fmt: on` — all code between these markers is preserved exactly.
  `# yapf: disable` / `# yapf: enable` are recognized as aliases.
- `# fmt: skip` — preserves the entire logical line (including multi-line bracketed
  expressions) containing this comment.

Suppressed regions are converted into opaque, atomic comment nodes in the CST so that
the formatting engine passes over them without modification. The scanning is optimized
to avoid quadratic behavior.

When `--line-ranges` is active, lines outside the specified ranges are similarly
converted to opaque comment blocks to prevent modification.

### 4.3 Line Generation

The CST is traversed using a visitor pattern to produce an intermediate representation
of **logical lines** — sequences of tokens that together form a single output line.
During this traversal:

- Whitespace is normalized.
- Empty line rules are enforced (see §6).
- Tokens are annotated with bracket depth and delimiter priority information for later
  splitting decisions.

### 4.4 Line Transformation (Splitting)

Lines exceeding the configured line length are transformed by a cascade of strategies,
tried in priority order:

1. **Delimiter-based splitting**: identify the highest-priority delimiter at bracket
   depth 0 and split there. Delimiter priorities from highest to lowest: commas →
   ternary operators → logical operators → string concatenation → comparison operators →
   arithmetic operators → dot access. Splits occur **after** commas but **before** all
   other delimiter types.

2. **Right-hand-side splitting**: for assignment-like statements, split at the opening
   bracket on the right-hand side, yielding a head line (left side + opening bracket), a
   body (contents), and a tail line (closing bracket).

3. **String transformations** (preview mode): merge adjacent string literals, split long
   strings at word boundaries, or wrap strings in parentheses.

4. **Standalone comment overflow**: comment-only lines that exceed the line length are
   left unchanged.

**"Second opinion" mechanism**: when a right-hand-side split still produces a first line
that is too long, the formatter re-attempts splitting with forced optional parentheses.
If every resulting line fits within the limit, this alternative is preferred. This
mechanism is suppressed in certain edge cases (multiline strings in the expression, type
comments, comments inside subscript brackets).

**Bracket depth tracking**: every token is assigned a bracket depth. Tokens at depth 0
are candidates for splitting. Commas between `for` and `in` in loops and comprehensions
are given artificially elevated depth so they never become split points (splitting there
would produce invalid Python). Lambda argument commas are similarly protected.

### 4.5 Two-Pass Formatting

The formatter always executes **two full passes**. The first pass produces a candidate
output. If the candidate differs from the input, the second pass reformats the first
pass's output. This two-pass approach resolves interactions between optional trailing
commas and optional parentheses that a single pass cannot handle.

### 4.6 Rendering

After transformation, all logical lines are joined into the final output string. The
original file's line ending style is preserved (see §11).

---

## 5. Formatting Rules

### 5.1 Line Length

- Default maximum: **88 characters**.
- Line length is measured with **Unicode East Asian Width** awareness:
  - Full-width characters (common in CJK text) count as **2 columns**.
  - Half-width characters count as **1 column**.
  - Control characters count as **0 columns**.
- For pure-ASCII content (the common case), a simple character count is used as a fast
  path.

### 5.2 Indentation

- Always **4 spaces**. This is not configurable.
- Tab characters in the input are converted to spaces.
- Continuation lines inside brackets are indented by 4 spaces relative to the enclosing
  bracket's line.

### 5.3 String Normalization

Enabled by default. Disabled with `-S` / `--skip-string-normalization`.

| Rule | Example |
|---|---|
| Prefer double quotes | `'hello'` → `"hello"` |
| Keep single quotes if conversion adds escapes | `'it\'s'` stays if double-quoting would not reduce escapes |
| Never alter triple-double-quoted strings | `"""..."""` unchanged |
| Lowercase prefixes | `F"x"` → `f"x"`, `B"x"` → `b"x"` |
| Remove `u` prefix | `u"text"` → `"text"` |
| Normalize prefix order | Two-character prefixes: `r` comes first (e.g., `rb` not `br`) |
| Raw strings: change quotes only | Escape sequences inside raw strings are never modified |
| F-strings: skip if conversion introduces backslashes in expressions | Prevents invalid syntax in older Python versions |
| Unicode escapes: lowercase hex, uppercase names | `\x0A` → `\x0a`, `\N{snowman}` → `\N{SNOWMAN}` |

### 5.4 Numeric Literal Normalization

| Type | Rule | Example |
|---|---|---|
| Hexadecimal | Uppercase digits | `0xff` → `0xFF` |
| Scientific | Normalize mantissa, lowercase `e`, strip `+` | `1.0e+03` → `1e3` |
| Float | Explicit leading/trailing zero | `.5` → `0.5`, `1.` → `1.0` |
| Complex | Real part normalized as above | `1.0e+2j` → `100j` |
| Octal / Binary | Unchanged | — |

### 5.5 Magic Trailing Comma

When enabled (the default), a trailing comma inside any bracket pair (parentheses,
square brackets, curly braces) **forces** the collection to be formatted with one
element per line, even if everything would fit on a single line.

This gives users explicit control over multi-line formatting: add a trailing comma to
force multi-line, remove it to allow collapsing.

Special case: `from X import (a, b, c,)` — the trailing comma forces one-import-per-line.

With `-C` / `--skip-magic-trailing-comma`, trailing commas are ignored for layout
decisions.

### 5.6 Parenthesization

- Expressions that need to be split across lines receive **invisible parentheses**
  (parentheses that appear in the output but were not in the input):
  - `return` and `yield` values
  - `assert` conditions and messages
  - Assignment right-hand sides
  - Dictionary values in long lines (preview)
  - Case guard conditions (preview)
  - Tuple values in yield expressions (preview)
- `from X import ...` is parenthesized when it contains multiple names.
- Single-name `from X import (a,)` has trailing comma and parentheses removed.
- **Unnecessary parentheses are removed** around `return`, `yield`, `assert`, `del`,
  `print`, and `for` targets when the parentheses serve no syntactic purpose.

### 5.7 Compound Statement Splitting

- One-line compound statements like `if x: pass` are split onto separate lines.
- Semicolons separating statements are removed; each statement goes on its own line.
- **Exception for stub files**: in `.pyi` files, stub definitions like `def f(): ...` or
  `def f(): pass` are kept on a single line.

### 5.8 Power Operator Spacing

When both operands of `**` are "simple" (a bare name, number, or dotted lookup,
optionally preceded by a unary operator), spaces around `**` are removed:

```python
# Before
a ** 2
# After
a**2
```

A preview feature (`simplify_power_operator_hugging`) provides refined heuristics.

### 5.9 Docstring Formatting

- Indentation is fixed according to PEP 257.
- Leading and trailing blank lines within the docstring are handled.
- Content lines are re-indented to match the containing definition's indentation level.
- Quotes are normalized if string normalization is enabled.

### 5.10 Import Formatting

- Multi-name `from X import ...` statements are wrapped in parentheses for splitting.
- Single-name imports: parentheses and trailing commas are removed.

### 5.11 Comment Normalization

- A single space is inserted after `#` in comments: `#comment` → `# comment`.
- Exceptions: `##`, `#!`, `#:`, and `#'` are left unchanged.
- Type comments (`# type: ...`) have their spacing normalized.
- Non-breaking space characters (NBSP) at the start of comments are replaced with regular
  spaces (except in type comments).

---

## 6. Vertical Whitespace (Empty Lines)

### 6.1 General Rules

| Context | Empty Lines |
|---|---|
| Before/after top-level function or class definition | 2 |
| Before/after nested function or class definition | 1 |
| After a group of imports (at any nesting depth) | 1 |
| After a module-level docstring | 1 |
| Within function bodies (module scope) | Preserved, up to maximum of 2 |
| Within function bodies (nested scope) | Preserved, up to maximum of 1 |

- Form feed characters (`\f`) at the beginning of top-level blocks are preserved.

### 6.2 Stub File Rules (`.pyi`)

- Maximum **1** empty line everywhere (never 2).
- No blank line between consecutive class definitions that have empty bodies.
- Blank lines between attributes and methods are preserved based on user intent.
- A stub definition immediately followed by another definition has no empty line between
  them (if the user did not include one).

### 6.3 Overload Groups (preview)

In `.pyi` files, consecutive decorated functions with the same name (overload groups,
property/setter pairs) have **zero** blank lines between them. This extends to overloads
split across conditional blocks. Comments between overloads preserve user-specified blank
lines.

### 6.4 Semantic Leading Comments

When a block comment appears immediately before a function or class definition, empty
lines are inserted **before** the comment block, not between the comment and the
definition. The comment is treated as part of the definition.

---

## 7. Format Directives

### 7.1 `# fmt: off` / `# fmt: on`

All code between these markers is preserved exactly as written. The formatter does not
modify any whitespace, line breaks, or tokens within the suppressed region.

- `# yapf: disable` / `# yapf: enable` are recognized as aliases.
- Whitespace-insensitive: both `# fmt: off` and `#fmt:off` are recognized.
- `# fmt: off` placed before a closing bracket is invalid and ignored.
- Mismatched bracket depths between `# fmt: off` and `# fmt: on` are handled
  gracefully.

### 7.2 `# fmt: skip`

Preserves the entire logical line containing this comment. For multi-line bracketed
statements, the **entire** enclosing statement is preserved. For one-line compound
statements (`if x: pass  # fmt: skip`), the header and body are preserved together.

Directives may appear alongside other comments using `#` or `;` separators.

---

## 8. Safety & Correctness

### 8.1 AST Equivalence Check

Unless `--fast` is specified, after formatting:

1. Both the original source and the formatted output are parsed using `ast.parse`.
2. The resulting ASTs are serialized to strings and compared.
3. Several normalizations are applied during comparison to accommodate known harmless
   differences:
   - Docstring indentation variations.
   - Trailing whitespace in type comments.
   - The `u` string prefix (which Black removes).
   - Nested tuple flattening in `del` statements.
4. If the ASTs differ, an error is raised and debug information is written to a
   temporary file.

When the target Python version exceeds the runtime Python version, a clear error
message suggests using `--fast` or running under the correct Python version.

### 8.2 Stability Check

The formatted output is reformatted a second time. If the result of the second pass
differs from the first, an internal error is raised — this indicates a formatter bug.
This check is skipped when `--line-ranges` is used (line mapping between passes is
imprecise).

### 8.3 Error Reporting

- **Syntax errors in input**: reported with file path, line number, column number, and a
  caret pointer to the error location.
- **AST safety failures**: full debug information dumped to a temporary file for
  inspection.
- **Internal errors**: exit code 123.

---

## 9. File Discovery & Filtering

Files are discovered recursively from provided source paths. Multiple filtering layers
are applied in order:

1. **`.gitignore` patterns**: respected by default. Nested `.gitignore` files are
   inherited.
2. **`--exclude`**: regex applied to file/directory paths found during recursive search.
   Overrides the default exclusion pattern.
3. **`--extend-exclude`**: additional regex that supplements (does not replace) the
   default and `--exclude` patterns.
4. **`--force-exclude`**: regex applied even to files explicitly passed on the command
   line.
5. **`--include`**: regex that files must match to be processed.
6. **Symlink safety**: symbolic links pointing outside the project root are ignored.

---

## 10. Jupyter Notebook Support

Notebook support requires optional dependencies (a tokenizer helper library and IPython).
When these are not installed, notebook files are silently skipped with a warning.

### Behavior

- Notebook JSON structure is validated; non-Python notebooks are skipped.
- Only `code` cells are processed.
- **Trailing semicolons**: removed before formatting (Jupyter uses trailing semicolons to
  suppress cell output), then restored afterward.
- **IPython magics**: line magics (`%`, `%%`), shell commands (`!`, `!!`), and help
  syntax (`?`, `??`) are masked with random tokens of equal length before formatting,
  then unmasked afterward.
- **Cell magics with Python bodies**: known Python-body cell magics (`%%time`,
  `%%timeit`, `%%capture`, etc.) have their bodies formatted. Non-Python cell magics are
  left untouched.
- Custom cell magic names can be registered via the `--python-cell-magics` option.
- Trailing newlines are stripped from each cell.

---

## 11. Encoding & Line Endings

- Source file encoding is detected via magic encoding comments
  (`# -*- coding: ... -*-`).
- The **original line ending style** (LF, CRLF, or CR) is preserved in the output.
- **Mixed line endings** are normalized to whichever style occurs most frequently in the
  file.

---

## 12. Caching

A file-level cache avoids redundant reformatting across runs.

### Cache Key

Each file's cache entry stores: modification time, file size, and SHA-256 content hash.

### Invalidation Logic

1. If file size has changed → reformat.
2. If modification time has changed → recompute hash; if hash differs → reformat.
3. If neither size nor mtime changed → skip (cached).

### Storage

- Cache files are stored in a platform-appropriate cache directory, customizable via the
  `BLACK_CACHE_DIR` environment variable.
- Each unique configuration (line length, target versions, flags) produces a separate
  cache file, identified by a hash of the configuration.
- Caches are versioned by the Black version — upgrading Black invalidates all caches.
- Cache files are written atomically (write to temp file, then rename). OS errors during
  cache operations are handled gracefully and never cause formatting failures.
- Caching is disabled in diff/color-diff modes and with `--no-cache`.

---

## 13. Concurrent Processing

When processing multiple files, formatting is parallelized across worker processes.

| Aspect | Behavior |
|---|---|
| Default workers | Number of CPUs (via `os.cpu_count()`) |
| Configuration | `--workers` flag or `BLACK_NUM_WORKERS` env var |
| Windows limit | Capped at 60 workers (workaround for a Python bug) |
| Frozen/packaged builds | Forced to 1 worker |
| Restricted environments | Falls back to single-threaded when multiprocessing is unavailable (e.g., AWS Lambda, Termux) |
| Event loop | Uses asyncio for scheduling; optionally accelerated by uvloop (Unix) or winloop (Windows) |
| Signal handling | SIGINT and SIGTERM cancel pending tasks gracefully |
| Diff output | A lock prevents interleaving of diff output from multiple workers |

---

## 14. Preview & Unstable Feature System

Features progress through three stability tiers:

| Tier | Activation | Description |
|---|---|---|
| **Stable** | Always active | Default formatting rules. |
| **Preview** | `--preview` | Style changes deemed ready for potential promotion. May be disruptive. |
| **Unstable** | `--unstable` (implies `--preview`) | Features with known bugs or behavioral issues. |

Individual features from the preview or unstable tier can be cherry-picked using
`--enable-unstable-feature NAME` (requires `--preview`).

Currently unstable features (known bugs): `string_processing`,
`hug_parens_with_braces_and_square_brackets`. All other named features are preview-tier.

---

## 15. HTTP Daemon

A separate entry point provides an HTTP-based formatting service.

### Endpoint

Accepts `POST` requests with Python source code in the request body.

### Response Codes

| Code | Meaning |
|---|---|
| 200 | Formatted code returned (changes were made) |
| 204 | No changes needed |
| 400 | Syntax error in input |
| 500 | Internal error |

### Request Headers

| Header | Type | Description |
|---|---|---|
| `X-Line-Length` | integer | Line length override |
| `X-Skip-String-Normalization` | truthy | Disable string normalization |
| `X-Skip-Magic-Trailing-Comma` | truthy | Disable magic trailing comma |
| `X-Fast-Or-Safe` | `"fast"` or `"safe"` | Safety check mode |
| `X-Python-Variant` | string | Target version (e.g., `"py310"`, `"3.10"`, `"pyi"`, `"ipynb"`) |
| `X-Preview` | truthy | Enable preview mode |
| `X-Unstable` | truthy | Enable unstable mode |
| `X-Diff` | truthy | Return diff instead of formatted code |
| `X-Skip-Source-First-Line` | truthy | Skip first line |

### Server Options

| Option | Default | Description |
|---|---|---|
| `--bind-host TEXT` | `localhost` | Host to bind to |
| `--bind-port INTEGER` | `45484` | Port to bind to |
| `--cors-allow-origin TEXT` | none | Allowed CORS origins (multiple allowed) |
| `--max-body-size INTEGER` | `5242880` (5 MB) | Maximum request body size |

The daemon preserves the original line endings of the input.

---

## 16. Build, Packaging & Distribution

### 16.1 Build System

- Build backend: hatchling (>= 1.27.0).
- Version: dynamically derived from VCS (git tags) using a hatch-vcs plugin. The
  resolved version is written to a version file for runtime access.
- README: dynamically assembled from the project README and changelog files using a
  readme plugin.

### 16.2 Core Dependencies

| Dependency | Purpose |
|---|---|
| click >= 8.0.0 | CLI framework |
| mypy-extensions >= 0.4.3 | Type system utilities |
| packaging >= 22.0 | Version parsing |
| pathspec >= 1.0.0 | Gitignore-style path matching |
| platformdirs >= 2 | Platform-appropriate config/cache directories |
| pytokens ~= 0.4.0 | Rust-based Python tokenizer |
| tomli >= 1.1.0 | TOML parsing (Python < 3.11 only) |
| typing-extensions >= 4.0.1 | Backported typing features (Python < 3.11 only) |

### 16.3 Optional Extras

| Extra name | Dependencies | Purpose |
|---|---|---|
| `colorama` | colorama >= 0.4.3 | Colored terminal output |
| `uvloop` | uvloop >= 0.15.2 (Unix) / winloop >= 0.5.0 (Windows) | Faster async event loop |
| `d` | aiohttp >= 3.10 | HTTP daemon |
| `jupyter` | ipython >= 7.8.0, tokenize-rt >= 3.2.0 | Jupyter Notebook formatting |

### 16.4 Entry Points

| Type | Name | Description |
|---|---|---|
| Console script | `black` | Main formatter |
| Console script | `blackd` | HTTP daemon (requires `[d]` extra) |
| validate_pyproject | — | JSON schema for `[tool.black]` validation |

### 16.5 Ahead-of-Time Compilation

The project optionally supports compilation via mypyc for improved runtime performance.
When enabled: optimization level 3, debug level 0. Certain components (the daemon,
concurrency management, I/O, reporting, and debug utilities) are excluded from
compilation.

---

## 17. Integrations

### 17.1 GitHub Action

A composite GitHub Action that:

- Accepts inputs: `options` (default: `--check --diff`), `src` (default: `.`), `jupyter`
  (default: false), `version`, `use_pyproject` (reads version from pyproject.toml
  project metadata or tool dependencies), `summary` (adds results to workflow summary),
  `output-file`.
- Creates a temporary virtualenv, installs the specified Black version, and runs the
  formatter.
- Version resolution priority: explicit `version` input → pyproject.toml tools/deps →
  git archival metadata.

### 17.2 Vim Plugin

- Requires Vim 7.0+ with Python 3 support.
- Automatically installs Black into a virtualenv on first use.
- Configurable via Vim variables and `pyproject.toml` (pyproject.toml takes precedence).
- Commands: `:Black` (format current buffer), `:BlackUpgrade`, `:BlackVersion`.
- Provides tab-completion for target version values.
- Preserves cursor positions across all windows and tabs after formatting.

### 17.3 Docker

- Multi-stage build using a Python slim base image.
- Builder stage enables mypyc compilation for performance.
- Runtime stage: minimal image containing the compiled formatter plus colorama, the HTTP
  daemon, and uvloop.
- Default container command: the formatter binary.

### 17.4 Pre-commit Hooks

Two hooks are provided:

| Hook ID | Purpose |
|---|---|
| `black` | Standard Python file formatting |
| `black-jupyter` | Python + Jupyter Notebook formatting |

- Requires pre-commit >= 2.9.2.
- Hooks run with `require_serial: true`.
- A pre-commit mirror repository is recommended for faster installation.

---

## 18. Output & Reporting

### 18.1 Summary Report

After processing, a summary is printed showing counts of:

- Files reformatted (changed).
- Files left unchanged.
- Files that failed (errors).

The summary uses color styling when output is a terminal.

### 18.2 Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success (or no changes in `--check` mode) |
| 1 | Files would be reformatted (`--check` mode only) |
| 123 | Internal error |

### 18.3 Diff Output

When `--diff` is active, output is in unified diff format. With `--color`:

| Element | Color |
|---|---|
| Added lines | Green |
| Removed lines | Red |
| Section headers | Cyan |
| File names | Bold |

Colors are suppressed when the `NO_COLOR` environment variable is set.

---

## 19. Python Version–Specific Syntax Support

The parser and formatter must handle all Python syntax from 3.3 through 3.15. Key
version-gated features include:

| Version | Feature |
|---|---|
| 3.5 | `await` expressions, async `for`/`with` |
| 3.6 | F-strings, variable annotations, underscores in numeric literals |
| 3.7 | `async`/`await` as reserved keywords |
| 3.8 | Assignment expressions (`:=`), positional-only parameters |
| 3.10 | `match`/`case` (soft keywords), parenthesized context managers |
| 3.12 | Type parameter lists (`type X = ...`, `class C[T]: ...`) |
| 3.13 | Type parameter defaults |
| 3.14 | T-strings (template strings) |
| 3.15 | Lazy imports |

The parser uses grammar variants and backtracking to correctly parse soft keywords
that are valid identifiers in older Python versions.

---

## 20. Design Decisions & Invariants

1. **88 characters, not 79 or 100**: 88 is ~10% shorter than 100 (reducing nesting
   pressure) while being meaningfully wider than PEP 8's 79 (reducing artificial line
   splits). It produces files that are ~0.1% more compact than equivalent 80-column
   formatting.

2. **Two-pass formatting**: a single formatting pass cannot resolve all interactions
   between trailing commas and optional parentheses. The second pass catches these cases.

3. **CST, not AST**: the formatter requires a Concrete Syntax Tree because it must
   faithfully preserve comments, whitespace in format-suppressed regions, and all
   structural information that an AST discards.

4. **Magic trailing comma as user intent signal**: rather than adding heuristics for when
   collections should be multi-line, Black uses the presence of a trailing comma as an
   explicit user signal. This is the primary mechanism users have for controlling
   vertical layout.

5. **Minimal configuration**: the deliberate lack of options is a feature. Style
   arguments waste time. The only meaningful options are line length, string quote
   normalization, magic trailing comma behavior, and target version.

6. **Safety by default**: the AST equivalence check runs by default because correctness
   is non-negotiable. Users must explicitly opt into `--fast` to skip it.

7. **Cache keyed on configuration**: different option combinations produce different
   caches because the same file may format differently under different settings.

8. **Symlinks outside root ignored**: prevents unexpected behavior when project trees
   contain symlinks to system files or other projects.

9. **Graceful degradation**: in environments without multiprocessing (Lambda, Termux),
   the formatter falls back to single-threaded operation rather than failing.

10. **Line ending preservation**: the formatter never changes a file's line ending
    convention. Mixed endings are normalized to the majority style within the file.
