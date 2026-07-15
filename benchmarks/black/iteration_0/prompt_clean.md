## Overview
You are tasked with implementing **Black**, the uncompromising code formatter for Python. It is a deterministic formatter that parses Python code, generates logical lines, and transforms them to fit within a specified line length while maintaining syntactic equivalence.

## High-Level Architecture
The system consists of the following major components:
1. **CLI and Configuration**: Handles command-line arguments, loads configuration from `pyproject.toml`, and orchestrates the formatting process.
2. **Parser (`blib2to3`)**: A custom fork of Python's standard `lib2to3` used to parse Python source code into a custom AST node structure.
3. **Line Generation**: Traverses the AST and generates logical lines of code (`Line` objects).
4. **Line Transformation**: Modifies generated lines to fit length limits and adhere to style rules (e.g., splitting long lines, parenthesizing).
5. **AST Utilities**: Provides helpers for navigating and manipulating the custom AST.

## Behavioral Requirements
- **Determinism**: Given the same input and configuration, the output must always be identical.
- **Syntactic Equivalence**: The formatted code must be syntactically equivalent to the original code. This should be verified by comparing the AST of the original code with the AST of the formatted code (unless safety checks are explicitly disabled).
- **Line Length**: The formatter attempts to fit code within a specified line length (default 88 characters).
- **Style Consistency**: Implements specific, uncompromising rules for spacing, indentation (4 spaces), and parenthesization. Prefers double quotes over single quotes. Appends trailing commas in multi-line lists/tuples/calls.

## File Manifest & Responsibilities

### Core Package (`src/black/`)

- `__init__.py`: The main entry point. Contains the CLI implementation (using `click`), configuration parsing (`pyproject.toml`), and high-level formatting functions (`format_str`, `format_file_in_place`). Orchestrates line generation and transformation.
- `linegen.py`: Contains the `LineGenerator` class which traverses the AST and yields logical lines. Also contains the main transformation dispatcher (`transform_line`).
- `lines.py`: Defines the `Line` class and related abstractions for representing logical lines of code, tracking empty lines, and line blocks.
- `trans.py`: Implements specific line transformers (e.g., for splitting strings, merging strings, hugging power operators). These transformers take a line and yield one or more modified lines.
- `nodes.py`: Provides utility functions and visitor patterns for navigating and manipulating the custom AST nodes (`blib2to3.pytree`).
- `comments.py`: Handles comment preservation, normalization, and formatting directives (e.g., `# fmt: off`).
- `brackets.py`: Utilities for tracking bracket depth, matching brackets, and calculating priority for line splitting.
- `mode.py`: Defines the `Mode` configuration object (line length, target versions, etc.) and `Feature` flags.
- `files.py`: File system utilities for finding project roots, `pyproject.toml`, and generating lists of Python files to format.
- `cache.py`: Implements caching mechanism to avoid reformatting unchanged files.
- `concurrency.py`: Parallel processing support for formatting multiple files concurrently.
- `output.py`: Utilities for printing output, diffs, and error messages.
- `report.py`: Tracking and reporting formatting statistics (changed, unchanged, failed files).
- `ranges.py`: Logic for formatting specific line ranges.
- `numerics.py`: Normalization of numeric literals.
- `strings.py`: String normalization utilities (quotes, prefixes, etc.).
- `handle_ipynb_magics.py`: Special handling for Jupyter notebook magic commands.
- `schema.py`: Validation schema for configuration.
- `debug.py`: Debugging utilities (e.g., printing AST trees).

### Custom Parser (`src/blib2to3/`)
A custom fork of `lib2to3` optimized for formatting needs. Includes grammar files (`Grammar.txt`, `PatternGrammar.txt`) and parser implementation (`pgen2` package).

## Build & Test Expectations
- The project uses `hatchling` as the build backend and dependencies are managed via `pyproject.toml`.
- It should be buildable and installable via `pip`.
- Testing is done via `pytest`. All tests in the `tests/` directory should pass.
- Code should be type-hinted and pass `mypy` checks.
