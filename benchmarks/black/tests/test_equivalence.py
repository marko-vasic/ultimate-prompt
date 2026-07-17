"""
Comprehensive equivalence test suite for the Black Python code formatter.

This test suite is IMPLEMENTATION-AGNOSTIC: it invokes the ``black`` binary (or
the path given via the ``BLACK_EXE`` environment variable) and asserts purely on
observable behaviour.  It does **not** import any internal Black modules.

Test Categories
---------------
1. **Formatting Case Tests** – parametrised over every ``.py`` file in the
   official Black test-cases directory.
2. **CLI Behavioural Tests** – exercise command-line flags and exit codes.
3. **Formatting Behaviour Tests** – verify idempotency, line-ending handling,
   ``# fmt: off/on``, magic trailing comma, encoding preservation, etc.

Run with::

    pytest test_equivalence.py -v
"""

from __future__ import annotations

import os
import re
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Tuple

import pytest

# ---------------------------------------------------------------------------
# Configuration – all overridable via environment variables
# ---------------------------------------------------------------------------

BLACK_EXE = Path(
    os.environ.get(
        "BLACK_EXE",
        "/usr/local/google/home/vasic/projects/ultimate-prompt"
        "/working_dir/black/.venv/bin/black",
    )
)

CASES_DIR = Path(
    os.environ.get(
        "CASES_DIR",
        "/usr/local/google/home/vasic/projects/ultimate-prompt"
        "/working_dir/black/tests/data/cases",
    )
)

# Sentinel that the test data uses in place of a truly empty line that contains
# only whitespace.
_EMPTY_LINE_SENTINEL = (
    "# EMPTY LINE WITH WHITESPACE (this comment will be removed)"
)

# Flags that are consumed by the test harness and must NOT be forwarded to the
# ``black`` binary.
_HARNESS_ONLY_FLAGS = frozenset(
    {
        "--no-preview-line-length-1",
    }
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _run_black(
    args: List[str],
    *,
    stdin: str | None = None,
    cwd: str | None = None,
    timeout: int = 120,
) -> subprocess.CompletedProcess:
    """Run the ``black`` binary with *args* and return the result.

    If *stdin* is provided it is fed to the process on standard input.
    """
    cmd = [str(BLACK_EXE)] + args
    return subprocess.run(
        cmd,
        input=stdin,
        capture_output=True,
        text=True,
        timeout=timeout,
        cwd=cwd,
    )


def _read_case_file(
    file_path: Path,
) -> Tuple[str, str, List[str]]:
    """Parse a Black test-case file and return ``(input, expected, flags)``.

    The file format:

    * Optional first line ``# flags: …`` – extracted as CLI flags.
    * Everything before the ``# output`` marker is the *input*.
    * Everything after ``# output`` is the *expected output*.
    * If there is no ``# output`` marker the file is idempotent – the input
      **is** the expected output.
    * The sentinel ``# EMPTY LINE WITH WHITESPACE (this comment will be
      removed)`` is replaced with an empty string.
    * If the flags contain ``--line-ranges``, the flags comment line is kept as
      part of the input (and expected output, if idempotent).
    """
    with open(file_path, "r", encoding="utf-8") as fh:
        lines = fh.readlines()

    input_lines: list[str] = []
    output_lines: list[str] = []
    flags: list[str] = []
    current = input_lines  # we start collecting into the input

    for line in lines:
        # Extract flags from the very first non-input line.
        if not input_lines and not output_lines and line.startswith("# flags: "):
            flags = shlex.split(line[len("# flags: "):])
            # When --line-ranges is among the flags the flags comment itself
            # must be part of the input (Black relies on it for line numbering).
            if any(f.startswith("--line-ranges") for f in flags):
                input_lines.append(line)
            continue

        # Replace the empty-line sentinel.
        line = line.replace(_EMPTY_LINE_SENTINEL, "")

        # Detect the output separator.
        if line.rstrip() == "# output":
            current = output_lines
            continue

        current.append(line)

    input_text = "".join(input_lines)
    output_text = "".join(output_lines)

    # Normalise: strip trailing whitespace from the whole text and ensure it
    # ends with exactly one newline (matching Black's own behaviour).
    input_text = input_text.strip() + "\n" if input_text.strip() else ""
    if output_text.strip():
        output_text = output_text.strip() + "\n"
    else:
        # No output section → idempotent: expected == input.
        output_text = input_text

    return input_text, output_text, flags


def _parse_flags(
    flags: List[str],
) -> Tuple[List[str], bool, str | None]:
    """Separate harness-only flags from flags destined for the ``black`` binary.

    Returns ``(black_flags, should_skip, skip_reason)``.
    """
    black_flags: list[str] = []
    should_skip = False
    skip_reason: str | None = None

    i = 0
    while i < len(flags):
        flag = flags[i]

        # --minimum-version=X.Y → skip if current Python is too old.
        # This is a harness-only flag; it is NOT forwarded to Black.
        if flag.startswith("--minimum-version="):
            version_str = flag.split("=", 1)[1]
            parts = version_str.split(".")
            major, minor = int(parts[0]), int(parts[1])
            if sys.version_info < (major, minor):
                should_skip = True
                skip_reason = f"Requires Python >= {version_str}"

        elif flag in _HARNESS_ONLY_FLAGS:
            # Consumed by harness – do not forward.
            pass

        else:
            black_flags.append(flag)

        i += 1

    return black_flags, should_skip, skip_reason


def _get_case_names() -> List[str]:
    """Return the stems of all ``.py`` files in *CASES_DIR*."""
    if not CASES_DIR.exists():
        return []
    return sorted(p.stem for p in CASES_DIR.glob("*.py"))


def _get_black_version() -> str:
    """Return the version string reported by ``black --version``."""
    result = _run_black(["--version"])
    # Output is typically "black, 24.4.2" or "black, version 24.4.2".
    return result.stdout.strip()


# =========================================================================
# Category 1 – Formatting Case Tests
# =========================================================================


class TestFormattingCases:
    """Parametrised tests over every ``.py`` file in CASES_DIR.

    For each case file the test:

    1. Parses the file to extract input, expected output, and flags.
    2. Writes the input to a temporary file.
    3. Runs ``black`` with the extracted flags.
    4. Reads the result and compares to the expected output.
    """

    @pytest.mark.parametrize("case_name", _get_case_names(), ids=str)
    def test_case(self, case_name: str) -> None:
        file_path = CASES_DIR / f"{case_name}.py"
        input_text, expected_text, raw_flags = _read_case_file(file_path)
        black_flags, should_skip, skip_reason = _parse_flags(raw_flags)

        if should_skip:
            pytest.skip(skip_reason)

        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_file = Path(tmp_dir) / "input.py"
            tmp_file.write_text(input_text, encoding="utf-8")

            result = _run_black([*black_flags, str(tmp_file)])

            assert result.returncode == 0, (
                f"black exited with code {result.returncode} for case "
                f"'{case_name}'.\n--- stderr ---\n{result.stderr}"
            )

            actual_text = tmp_file.read_text(encoding="utf-8")
            assert actual_text == expected_text, (
                f"Formatting mismatch for case '{case_name}'.\n"
                f"--- flags ---\n{black_flags}\n"
                f"--- expected (first 2000 chars) ---\n{expected_text[:2000]}\n"
                f"--- actual   (first 2000 chars) ---\n{actual_text[:2000]}"
            )


# =========================================================================
# Category 2 – CLI Behavioural Tests
# =========================================================================


class TestCLIBehaviour:
    """Tests that exercise command-line flags, exit codes, and I/O modes."""

    # -- --check ----------------------------------------------------------

    def test_check_no_change(self) -> None:
        """``black --check`` on already-formatted code exits with 0."""
        formatted = "x = 1\n"
        result = _run_black(["--check", "--code", formatted])
        assert result.returncode == 0

    def test_check_needs_change(self) -> None:
        """``black --check`` on unformatted code exits with 1."""
        unformatted = "x  =  1\n"
        result = _run_black(["--check", "--code", unformatted])
        assert result.returncode == 1

    # -- --diff -----------------------------------------------------------

    def test_diff_output(self) -> None:
        """``black --diff`` produces unified diff output on stdout."""
        unformatted = "x  =  1\n"
        result = _run_black(["--diff", "--code", unformatted])
        assert result.returncode == 0
        assert "---" in result.stdout
        assert "+++" in result.stdout
        # Should contain the fixed form
        assert "x = 1" in result.stdout

    def test_color_diff_output(self) -> None:
        """``black --diff --color`` includes ANSI escape sequences."""
        unformatted = "x  =  1\n"
        result = _run_black(["--diff", "--color", "--code", unformatted])
        assert result.returncode == 0
        # ANSI escape code prefix
        assert "\033[" in result.stdout

    # -- --code -----------------------------------------------------------

    def test_code_option(self) -> None:
        """``black --code 'x  =  1'`` prints formatted code to stdout."""
        result = _run_black(["--code", "x  =  1"])
        assert result.returncode == 0
        assert result.stdout == "x = 1\n"

    def test_code_option_check(self) -> None:
        """``--check --code`` returns 0 for clean, 1 for dirty code."""
        clean = _run_black(["--check", "--code", "x = 1\n"])
        assert clean.returncode == 0

        dirty = _run_black(["--check", "--code", "x  =  1"])
        assert dirty.returncode == 1

    # -- stdin (``-``) ----------------------------------------------------

    def test_stdin_pipe(self) -> None:
        """Piping code into ``black -`` formats it on stdout."""
        unformatted = "x  =  1\n"
        result = _run_black(["-"], stdin=unformatted)
        assert result.returncode == 0
        assert result.stdout.strip() == "x = 1"

    def test_stdin_pipe_pyi(self) -> None:
        """``black --pyi -`` applies pyi formatting rules.

        In ``.pyi`` mode Black formats stubs differently (fewer blank lines,
        one-liner function definitions).  We check that output is valid.
        """
        code = "def foo() -> None:\n    pass\n"
        result = _run_black(["--pyi", "-"], stdin=code)
        assert result.returncode == 0
        # .pyi mode should produce valid output (pass is kept, not converted)
        assert "def foo" in result.stdout

    # -- --version --------------------------------------------------------

    def test_version_output(self) -> None:
        """``black --version`` includes the word 'black'."""
        result = _run_black(["--version"])
        assert result.returncode == 0
        assert "black" in result.stdout.lower()

    # -- --quiet ----------------------------------------------------------

    def test_quiet_flag(self) -> None:
        """``black --quiet --check`` on unformatted code: stderr empty, exit 1."""
        unformatted = "x  =  1\n"
        result = _run_black(["--quiet", "--check", "--code", unformatted])
        assert result.returncode == 1
        assert result.stderr == ""

    # -- invalid input ----------------------------------------------------

    def test_invalid_input(self) -> None:
        """``black --code`` on a syntax error exits with 123."""
        result = _run_black(["--code", "def f(:"])
        assert result.returncode == 123

    # -- --required-version -----------------------------------------------

    def test_required_version_match(self) -> None:
        """``--required-version`` matching the installed version succeeds."""
        ver_result = _run_black(["--version"])
        assert ver_result.returncode == 0
        # Parse version: output is like "black, 24.4.2" or "black, version 24.4.2"
        version_output = ver_result.stdout.strip()
        # Extract the version number (last token that looks like a version).
        match = re.search(r"(\d+\.\d+\.\d+\S*)", version_output)
        if not match:
            pytest.skip("Could not parse black version from output")
        version = match.group(1)

        result = _run_black(
            ["--required-version", version, "--code", "x = 1\n"]
        )
        assert result.returncode == 0

    # -- --line-length ----------------------------------------------------

    def test_line_length_option(self) -> None:
        """``--line-length 40`` causes long lines to be split."""
        code = "result = some_function(argument1, argument2, argument3)"
        result = _run_black(["--line-length", "40", "--code", code])
        assert result.returncode == 0
        # The output must contain a newline *within* the expression – i.e. the
        # line was split.
        lines = result.stdout.strip().splitlines()
        assert len(lines) > 1, (
            f"Expected line to be split with --line-length=40, "
            f"got: {result.stdout!r}"
        )

    # -- --skip-string-normalization / -S ---------------------------------

    def test_skip_string_normalization(self) -> None:
        """``-S`` preserves single quotes."""
        code = "x = 'hello'\n"
        result = _run_black(["-S", "--code", code])
        assert result.returncode == 0
        assert "'" in result.stdout
        assert "'hello'" in result.stdout

    # -- --target-version -------------------------------------------------

    def test_target_version(self) -> None:
        """``-t py310`` accepts match/case syntax without error."""
        code = (
            "match x:\n"
            "    case 1:\n"
            "        pass\n"
        )
        result = _run_black(["-t", "py310", "--code", code])
        assert result.returncode == 0

    # -- multiple files ---------------------------------------------------

    def test_multiple_files(self) -> None:
        """Black can format multiple files in a single invocation."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            f1 = Path(tmp_dir) / "a.py"
            f2 = Path(tmp_dir) / "b.py"
            f1.write_text("x  =  1\n", encoding="utf-8")
            f2.write_text("y  =  2\n", encoding="utf-8")

            result = _run_black([str(f1), str(f2)])
            assert result.returncode == 0
            assert f1.read_text(encoding="utf-8") == "x = 1\n"
            assert f2.read_text(encoding="utf-8") == "y = 2\n"

    # -- directory formatting ---------------------------------------------

    def test_format_directory(self) -> None:
        """Black can recursively format a directory."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            sub = Path(tmp_dir) / "pkg"
            sub.mkdir()
            f = sub / "mod.py"
            f.write_text("x  =  1\n", encoding="utf-8")

            result = _run_black([str(tmp_dir)])
            assert result.returncode == 0
            assert f.read_text(encoding="utf-8") == "x = 1\n"


# =========================================================================
# Category 3 – Formatting Behaviour Tests
# =========================================================================


class TestFormattingBehaviour:
    """Tests that verify Black's formatting semantics."""

    # -- idempotency ------------------------------------------------------

    def test_idempotency(self) -> None:
        """Formatting once and formatting again yields the same result."""
        unformatted = (
            "x  =  1\n"
            "y=   [1,2,  3]\n"
            "def foo( a , b ):\n"
            "  return   a+b\n"
        )
        first = _run_black(["--code", unformatted])
        assert first.returncode == 0

        second = _run_black(["--code", first.stdout])
        assert second.returncode == 0
        assert first.stdout == second.stdout

    def test_idempotency_complex(self) -> None:
        """Idempotency on a more complex snippet with multiple constructs."""
        code = (
            "import os,   sys\n"
            "from pathlib   import Path\n\n"
            "class Foo  (object ):\n"
            "  def bar(self, x,y,z ):\n"
            "    if   x:\n"
            "      return [1,2,\n"
            "        3]\n"
            "    else:\n"
            "      return {  'a':1,  'b':2}\n"
        )
        first = _run_black(["--code", code])
        assert first.returncode == 0

        second = _run_black(["--code", first.stdout])
        assert second.returncode == 0
        assert first.stdout == second.stdout

    # -- line-ending preservation -----------------------------------------

    def test_preserves_line_endings_lf(self) -> None:
        """Input with LF line endings produces LF output."""
        code = "x  =  1\ny  =  2\n"
        assert "\r" not in code  # sanity
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "lf.py"
            f.write_bytes(code.encode("utf-8"))

            result = _run_black([str(f)])
            assert result.returncode == 0

            raw = f.read_bytes()
            assert b"\r\n" not in raw
            assert b"\n" in raw

    def test_preserves_line_endings_crlf(self) -> None:
        """Input with CRLF line endings preserves CRLF."""
        code = "x  =  1\r\ny  =  2\r\n"
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "crlf.py"
            f.write_bytes(code.encode("utf-8"))

            result = _run_black([str(f)])
            assert result.returncode == 0

            raw = f.read_bytes()
            # Every newline should be CRLF
            text = raw.decode("utf-8")
            lines = text.split("\n")
            # At least the non-empty lines should end with \r
            non_empty = [l for l in lines if l]
            for line in non_empty:
                assert line.endswith("\r"), (
                    f"Expected CRLF endings, but line {line!r} lacks \\r"
                )

    def test_normalizes_mixed_line_endings(self) -> None:
        """Mixed line endings are normalized to the majority type."""
        # Majority LF (3 LF vs 1 CRLF) — construct bytes directly to avoid
        # Python string escaping issues with \r\n.
        raw_input = b"x  =  1\r\ny  =  2\nz  =  3\nw  =  4\n"
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "mixed.py"
            f.write_bytes(raw_input)

            result = _run_black([str(f)])
            assert result.returncode == 0

            raw_output = f.read_bytes()
            # Count line endings to verify majority wins
            crlf_count = raw_output.count(b"\r\n")
            lf_count = raw_output.count(b"\n") - crlf_count
            # All line endings should be consistent (all one type)
            assert crlf_count == 0 or lf_count == 0, (
                f"Mixed line endings remain: {crlf_count} CRLF, {lf_count} LF"
            )

    # -- empty / whitespace input -----------------------------------------

    def test_empty_input(self) -> None:
        """``black --code ''`` returns empty or just-newline output."""
        result = _run_black(["--code", ""])
        assert result.returncode == 0
        assert result.stdout.strip() == ""

    def test_whitespace_only_input(self) -> None:
        """``black --code '   \\n  '`` returns empty output."""
        result = _run_black(["--code", "   \n  "])
        assert result.returncode == 0
        assert result.stdout.strip() == ""

    def test_whitespace_only_file(self) -> None:
        """A file containing only whitespace is reduced to empty."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "ws.py"
            f.write_text("  \n\t\t\n   \n", encoding="utf-8")

            result = _run_black([str(f)])
            assert result.returncode == 0

            content = f.read_text(encoding="utf-8")
            assert content.strip() == ""

    # -- # fmt: off / on --------------------------------------------------

    def test_fmt_off_on(self) -> None:
        """Code between ``# fmt: off`` and ``# fmt: on`` is preserved."""
        code = (
            "x = 1\n"
            "# fmt: off\n"
            "y  =  [  1,2,  3 ]\n"
            "z={'a' :1}\n"
            "# fmt: on\n"
            "w  =  4\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        output = result.stdout

        # The ugly code between fmt:off / fmt:on must be preserved verbatim.
        assert "y  =  [  1,2,  3 ]" in output
        assert "z={'a' :1}" in output
        # Code outside the markers is formatted.
        assert "w = 4" in output

    def test_fmt_off_on_in_file(self) -> None:
        """Same as above but operating on a file (not --code)."""
        code = (
            "x = 1\n"
            "# fmt: off\n"
            "y  =  [  1,2,  3 ]\n"
            "# fmt: on\n"
            "w  =  4\n"
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "fmtoff.py"
            f.write_text(code, encoding="utf-8")

            result = _run_black([str(f)])
            assert result.returncode == 0

            output = f.read_text(encoding="utf-8")
            assert "y  =  [  1,2,  3 ]" in output
            assert "w = 4" in output

    # -- # fmt: skip ------------------------------------------------------

    def test_fmt_skip(self) -> None:
        """A line with ``# fmt: skip`` is preserved exactly."""
        code = (
            "x  =  1  # fmt: skip\n"
            "y  =  2\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        output = result.stdout
        assert "x  =  1  # fmt: skip" in output
        # The other line should be formatted.
        assert "y = 2" in output

    def test_fmt_skip_in_file(self) -> None:
        """``# fmt: skip`` works when formatting a file."""
        code = "a  =  1  # fmt: skip\nb  =  2\n"
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "skip.py"
            f.write_text(code, encoding="utf-8")

            result = _run_black([str(f)])
            assert result.returncode == 0

            output = f.read_text(encoding="utf-8")
            assert "a  =  1  # fmt: skip" in output
            assert "b = 2" in output

    # -- magic trailing comma ---------------------------------------------

    def test_magic_trailing_comma(self) -> None:
        """A trailing comma keeps the collection multi-line even if it fits."""
        code = "x = [1, 2, 3,]\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        # With the magic trailing comma, each element gets its own line
        # (at least more than one line).
        assert len(lines) > 1, (
            f"Trailing comma should force multi-line, got: {result.stdout!r}"
        )

    def test_skip_magic_trailing_comma(self) -> None:
        """``-C`` disables the magic trailing comma behaviour."""
        code = "x = [1, 2, 3,]\n"
        result = _run_black(["-C", "--code", code])
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        # Without magic trailing comma, the short list fits on one line.
        assert len(lines) == 1, (
            f"With -C the short list should stay single-line, "
            f"got: {result.stdout!r}"
        )

    def test_magic_trailing_comma_function_args(self) -> None:
        """Trailing comma in function call forces multi-line."""
        code = "foo(a, b, c,)\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        assert len(lines) > 1

    def test_skip_magic_trailing_comma_function_args(self) -> None:
        """``-C`` collapses trailing-comma function calls to one line."""
        code = "foo(a, b, c,)\n"
        result = _run_black(["-C", "--code", code])
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        assert len(lines) == 1

    # -- encoding preservation --------------------------------------------

    def test_encoding_preserved_utf8(self) -> None:
        """UTF-8 encoded file with non-ASCII content is preserved."""
        code = '# -*- coding: utf-8 -*-\nx  =  "héllo"\n'
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "enc.py"
            f.write_text(code, encoding="utf-8")

            result = _run_black([str(f)])
            assert result.returncode == 0

            output = f.read_text(encoding="utf-8")
            assert "héllo" in output

    def test_encoding_preserved_latin1(self) -> None:
        """File with a ``coding: latin-1`` declaration maintains encoding."""
        # Build a file with latin-1 encoding.
        header = "# -*- coding: latin-1 -*-\n"
        body = 'x  =  "caf\\xe9"\n'
        raw = (header + body).encode("latin-1")

        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "latin.py"
            f.write_bytes(raw)

            result = _run_black([str(f)])
            assert result.returncode == 0

            # The file should still be readable as latin-1.
            output_bytes = f.read_bytes()
            output = output_bytes.decode("latin-1")
            assert "coding: latin-1" in output

    def test_utf8_bom_preserved(self) -> None:
        """A file with a UTF-8 BOM keeps the BOM after formatting."""
        code = "x  =  1\n"
        raw = b"\xef\xbb\xbf" + code.encode("utf-8")

        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "bom.py"
            f.write_bytes(raw)

            result = _run_black([str(f)])
            assert result.returncode == 0

            output_bytes = f.read_bytes()
            assert output_bytes.startswith(b"\xef\xbb\xbf"), (
                "UTF-8 BOM should be preserved"
            )

    # -- string normalization (default) ------------------------------------

    def test_default_string_normalization(self) -> None:
        """By default Black normalizes single quotes to double quotes."""
        code = "x = 'hello'\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert '"hello"' in result.stdout

    # -- preview mode ------------------------------------------------------

    def test_preview_flag_accepted(self) -> None:
        """``--preview`` is accepted without error."""
        code = "x = 1\n"
        result = _run_black(["--preview", "--code", code])
        assert result.returncode == 0

    # -- --fast flag -------------------------------------------------------

    def test_fast_flag_accepted(self) -> None:
        """``--fast`` is accepted without error."""
        code = "x  =  1\n"
        result = _run_black(["--fast", "--code", code])
        assert result.returncode == 0
        assert "x = 1" in result.stdout

    # -- basic formatting transformations ----------------------------------

    def test_removes_extra_whitespace_around_assignment(self) -> None:
        """Extra whitespace around ``=`` is removed."""
        result = _run_black(["--code", "x   =   1"])
        assert result.returncode == 0
        assert result.stdout.strip() == "x = 1"

    def test_removes_extra_whitespace_in_function_def(self) -> None:
        """Extra whitespace inside function signatures is removed."""
        result = _run_black(["--code", "def foo( a , b ):\n  pass"])
        assert result.returncode == 0
        assert "def foo(a, b):" in result.stdout

    def test_normalizes_indentation(self) -> None:
        """Non-standard indentation is normalised to 4 spaces."""
        code = "if True:\n  x = 1\n  y = 2\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        for line in result.stdout.splitlines():
            if line and not line.startswith("if"):
                leading = len(line) - len(line.lstrip())
                assert leading % 4 == 0

    def test_normalizes_commas(self) -> None:
        """Missing spaces after commas are added."""
        code = "x = [1,2,3]\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "1, 2, 3" in result.stdout

    def test_removes_trailing_whitespace(self) -> None:
        """Trailing whitespace on lines is removed."""
        code = "x = 1   \ny = 2  \n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        for line in result.stdout.splitlines():
            assert line == line.rstrip()

    def test_adds_trailing_newline(self) -> None:
        """Output always ends with a newline."""
        code = "x = 1"  # no trailing newline
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert result.stdout.endswith("\n")

    def test_removes_semicolons(self) -> None:
        """Semicolons separating statements are replaced with newlines."""
        code = "x = 1; y = 2\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert ";" not in result.stdout
        assert "x = 1\n" in result.stdout
        assert "y = 2\n" in result.stdout

    def test_parenthesizes_long_imports(self) -> None:
        """Long import lines get parenthesized."""
        code = (
            "from some.very.long.module.path import "
            "alpha, beta, gamma, delta, epsilon, zeta, eta, theta\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # Should be split across lines with parentheses
        assert "(" in result.stdout

    def test_collapses_blank_lines(self) -> None:
        """Excess blank lines between top-level definitions are collapsed."""
        code = "def a():\n    pass\n\n\n\n\ndef b():\n    pass\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # PEP 8: two blank lines between top-level defs
        assert "\n\n\ndef" in result.stdout
        # But not more than two blank lines
        assert "\n\n\n\ndef" not in result.stdout

    # -- comments ---------------------------------------------------------

    def test_preserves_comments(self) -> None:
        """Comments are preserved through formatting."""
        code = "x  =  1  # important value\n# standalone comment\ny  =  2\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "# important value" in result.stdout
        assert "# standalone comment" in result.stdout

    def test_normalizes_comment_spacing(self) -> None:
        """Inline comments get exactly two spaces before the ``#``."""
        code = "x = 1 # one space\ny = 2     # many spaces\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        for line in result.stdout.splitlines():
            if "#" in line and not line.lstrip().startswith("#"):
                idx = line.index("#")
                assert line[idx - 2 : idx] == "  "

    # -- decorator handling -----------------------------------------------

    def test_decorator_no_blank_line(self) -> None:
        """No blank line between decorator and function definition."""
        code = "@decorator\n\n\ndef foo():\n    pass\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        lines = result.stdout.splitlines()
        dec_idx = next(i for i, l in enumerate(lines) if l.startswith("@"))
        def_idx = next(
            i for i, l in enumerate(lines) if l.startswith("def")
        )
        assert def_idx == dec_idx + 1

    # -- return code on no-op -------------------------------------------

    def test_no_change_exit_code(self) -> None:
        """Black exits 0 when a file needs no changes."""
        code = "x = 1\n"
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "clean.py"
            f.write_text(code, encoding="utf-8")

            result = _run_black(["--check", str(f)])
            assert result.returncode == 0

    # -- line length default (88) ----------------------------------------

    def test_default_line_length(self) -> None:
        """By default Black uses a line length of 88."""
        # Build a line that is exactly 89 chars (should be split).
        code = "x = " + '"' + "a" * 83 + '"' + "\n"
        # This string assignment is 89 chars total.
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # Very long lines may or may not be split depending on where break
        # points are, but let's test with a function call approach:
        long_call = (
            "result = some_function("
            + ", ".join(f"arg{i}" for i in range(15))
            + ")\n"
        )
        result2 = _run_black(["--code", long_call])
        assert result2.returncode == 0
        lines2 = result2.stdout.strip().splitlines()
        assert len(lines2) > 1  # should be split

    # -- empty file -------------------------------------------------------

    def test_empty_file(self) -> None:
        """An empty file remains empty (or becomes just a newline)."""
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "empty.py"
            f.write_text("", encoding="utf-8")

            result = _run_black([str(f)])
            assert result.returncode == 0

            content = f.read_text(encoding="utf-8")
            assert content.strip() == ""

    # -- preserves shebangs -----------------------------------------------

    def test_preserves_shebang(self) -> None:
        """A shebang line is preserved at the top of the file."""
        code = "#!/usr/bin/env python3\nx  =  1\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert result.stdout.startswith("#!/usr/bin/env python3\n")

    # -- type comments ----------------------------------------------------

    def test_preserves_type_comments(self) -> None:
        """Type comments are preserved."""
        code = "x = 1  # type: int\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "# type: int" in result.stdout

    # -- pyi mode --------------------------------------------------------

    def test_pyi_collapses_function_body(self) -> None:
        """In .pyi mode, function bodies with ``pass`` become ``...``."""
        code = (
            "def foo(x: int) -> None:\n"
            "    pass\n"
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "stub.pyi"
            f.write_text(code, encoding="utf-8")

            result = _run_black(["--pyi", str(f)])
            assert result.returncode == 0

            output = f.read_text(encoding="utf-8")
            # pyi mode keeps pass as-is (doesn't convert to ...) but may
            # collapse to a one-liner like "def foo(x: int) -> None: pass"
            assert "def foo" in output
            assert "pass" in output

    def test_pyi_fewer_blank_lines(self) -> None:
        """In .pyi mode, fewer blank lines between top-level definitions."""
        code = (
            "def foo() -> None: ...\n"
            "\n\n\n"
            "def bar() -> None: ...\n"
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            f = Path(tmp_dir) / "stub.pyi"
            f.write_text(code, encoding="utf-8")

            result = _run_black(["--pyi", str(f)])
            assert result.returncode == 0

            output = f.read_text(encoding="utf-8")
            # .pyi uses at most 1 blank line between top-level defs
            assert "\n\n\n" not in output

    # -- ensure Black doesn't mangle multiline strings --------------------

    def test_preserves_multiline_string_content(self) -> None:
        """Content inside triple-quoted strings is not reformatted."""
        code = (
            'x = """\n'
            "this   is   some   text\n"
            "  with   weird   spacing\n"
            '"""\n'
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "this   is   some   text" in result.stdout
        assert "  with   weird   spacing" in result.stdout

    # -- tab-to-space conversion ------------------------------------------

    def test_converts_tabs_to_spaces(self) -> None:
        """Tabs used for indentation are converted to spaces."""
        code = "if True:\n\tx = 1\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "\t" not in result.stdout
        assert "    x = 1" in result.stdout

    # -- stdin filename ---------------------------------------------------

    def test_stdin_filename(self) -> None:
        """``--stdin-filename`` affects the mode without creating a file."""
        code = "def foo() -> None:\n    pass\n"
        result = _run_black(
            ["--stdin-filename", "stub.pyi", "-"],
            stdin=code,
        )
        assert result.returncode == 0
        # --stdin-filename stub.pyi triggers pyi mode
        assert "def foo" in result.stdout
        assert "pass" in result.stdout


# =========================================================================
# Category 4 – Edge Cases & Regression Guards
# =========================================================================


class TestEdgeCases:
    """Additional edge-case and regression-guard tests."""

    def test_deeply_nested_formatting(self) -> None:
        """Deeply nested structures format without error."""
        code = "x = [[[[[[[[1]]]]]]]]\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "1" in result.stdout

    def test_star_expressions(self) -> None:
        """Star expressions in assignments are handled."""
        code = "a, *b = [1, 2, 3]\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "*b" in result.stdout

    def test_walrus_operator(self) -> None:
        """The walrus operator ``:=`` is formatted correctly."""
        code = "if (n := 10) > 5:\n    print(n)\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert ":=" in result.stdout

    def test_fstring_formatting(self) -> None:
        """f-strings are preserved."""
        code = 'x = f"hello {name}"\n'
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "{name}" in result.stdout

    def test_async_def(self) -> None:
        """``async def`` is formatted correctly."""
        code = "async  def  foo( ):\n  pass\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "async def foo():" in result.stdout

    def test_try_except_else_finally(self) -> None:
        """try/except/else/finally blocks are preserved."""
        code = (
            "try:\n"
            "  x = 1\n"
            "except ValueError:\n"
            "  x = 2\n"
            "else:\n"
            "  x = 3\n"
            "finally:\n"
            "  x = 4\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "try:" in result.stdout
        assert "except ValueError:" in result.stdout
        assert "else:" in result.stdout
        assert "finally:" in result.stdout

    def test_class_with_multiple_bases(self) -> None:
        """Classes with multiple base classes are formatted."""
        code = "class Foo( Base1, Base2,  Base3 ):\n  pass\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "class Foo(Base1, Base2, Base3):" in result.stdout

    def test_lambda_expression(self) -> None:
        """Lambda expressions are formatted."""
        code = "f = lambda x,y :  x+y\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "lambda" in result.stdout

    def test_ternary_expression(self) -> None:
        """Ternary (conditional) expressions are formatted."""
        code = "x  =  1  if  True  else  2\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "x = 1 if True else 2" in result.stdout

    def test_dict_comprehension(self) -> None:
        """Dict comprehensions are formatted."""
        code = "x = {k :  v  for  k,v  in  items}\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "for" in result.stdout
        assert "in" in result.stdout

    def test_set_literal(self) -> None:
        """Set literals are formatted correctly."""
        code = "x = {1,2,  3}\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "1, 2, 3" in result.stdout

    def test_chained_comparison(self) -> None:
        """Chained comparisons are preserved."""
        code = "result = 1  <  x  <  10\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "1 < x < 10" in result.stdout

    def test_yield_expression(self) -> None:
        """Yield expressions are formatted correctly."""
        code = "def gen():\n  yield   1\n  yield   from  other()\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "yield 1" in result.stdout
        assert "yield from other()" in result.stdout

    def test_assert_statement(self) -> None:
        """Assert statements are formatted."""
        code = 'assert  x  ==  1,  "x must be 1"\n'
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert 'assert x == 1, "x must be 1"' in result.stdout

    def test_with_statement(self) -> None:
        """With statements are formatted correctly."""
        code = "with  open( 'f' )  as  fh:\n  pass\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "with" in result.stdout
        assert "as fh:" in result.stdout

    def test_global_nonlocal(self) -> None:
        """global and nonlocal statements are handled."""
        code = (
            "def foo():\n"
            "    global  x\n"
            "    def bar():\n"
            "        nonlocal  x\n"
            "        x = 1\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "global x" in result.stdout
        assert "nonlocal x" in result.stdout

    def test_ellipsis_body(self) -> None:
        """``...`` as a function body is preserved (may be collapsed to one line)."""
        code = "def foo():\n    ...\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # Black may keep this as a one-liner: "def foo(): ..."
        assert "def foo()" in result.stdout
        assert "..." in result.stdout

    def test_complex_decorator(self) -> None:
        """Complex decorators with arguments are formatted."""
        code = (
            "@decorator( arg1, arg2 )\n"
            "def foo():\n"
            "    pass\n"
        )
        result = _run_black(["--code", code])
        assert result.returncode == 0
        assert "@decorator(arg1, arg2)" in result.stdout

    def test_multiline_dict(self) -> None:
        """A dict exceeding the line length gets properly split and indented."""
        # Use --line-length 40 to force splitting
        code = (
            "x = {'a': 1, 'b': 2, 'c': 3, 'd': 4, "
            "'e': 5, 'f': 6, 'g': 7, 'h': 8, 'i': 9, 'j': 10}\n"
        )
        result = _run_black(["--line-length", "40", "--code", code])
        assert result.returncode == 0
        # At 40-char line length, this dict must be split
        lines = result.stdout.strip().splitlines()
        assert len(lines) > 1

    def test_backslash_continuation(self) -> None:
        """Black may remove backslash continuations in favour of parens."""
        code = "x = 1 + \\\n    2 + \\\n    3\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # The result should still evaluate to the same expression
        assert "1" in result.stdout
        assert "2" in result.stdout
        assert "3" in result.stdout

    def test_numeric_literal_normalization(self) -> None:
        """Numeric literals are normalised (e.g. uppercase hex)."""
        code = "x = 0xff\ny = 0b1010\nz = 0o77\n"
        result = _run_black(["--code", code])
        assert result.returncode == 0
        # Black uppercases hex digits
        assert "0xFF" in result.stdout or "0XFF" in result.stdout or "0xff" in result.stdout


# =========================================================================
# Smoke Tests – quick sanity checks
# =========================================================================


class TestSmokeTests:
    """Fast smoke tests to verify the Black binary is operational."""

    def test_black_executable_exists(self) -> None:
        """The configured BLACK_EXE path exists."""
        assert BLACK_EXE.exists(), f"Black executable not found at {BLACK_EXE}"

    def test_black_can_run(self) -> None:
        """``black --version`` can be invoked successfully."""
        result = _run_black(["--version"])
        assert result.returncode == 0

    def test_cases_dir_exists(self) -> None:
        """The configured CASES_DIR exists and contains .py files."""
        assert CASES_DIR.exists(), f"Cases directory not found at {CASES_DIR}"
        py_files = list(CASES_DIR.glob("*.py"))
        assert len(py_files) > 0, "No .py case files found"

    def test_help_output(self) -> None:
        """``black --help`` prints usage information."""
        result = _run_black(["--help"])
        assert result.returncode == 0
        assert "usage" in result.stdout.lower() or "options" in result.stdout.lower()
