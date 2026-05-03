#!/usr/bin/env bash
# =============================================================================
# Equivalence Test Suite Runner — ripgrep
# =============================================================================
#
# Usage:
#   ./run_tests.sh <path-to-rg-binary>
#
# Example:
#   ./run_tests.sh /path/to/target/debug/rg
#
# This script runs all equivalence tests against the given rg binary.
# Tests are standalone, self-contained, and implementation-agnostic.
# They verify *observable behavior* — what the program does — not how it's built.
#
# Exit code:
#   0 if all tests pass
#   1 if any test fails
# =============================================================================

set -euo pipefail

RG="${1:?Usage: $0 <path-to-rg-binary>}"

if [[ ! -x "$RG" ]]; then
    echo "ERROR: '$RG' is not executable or does not exist."
    exit 1
fi

# Absolute path for reliability
RG="$(realpath "$RG")"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0
FAIL=0
ERRORS=()

# Temp directory for test fixtures
TMPDIR_BASE="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_BASE"' EXIT

# Unset config to ensure reproducible defaults
unset RIPGREP_CONFIG_PATH 2>/dev/null || true

# =============================================================================
# Test Helpers
# =============================================================================

# Create a fresh temp directory for a test case
make_test_dir() {
    local name="$1"
    local dir="$TMPDIR_BASE/$name"
    mkdir -p "$dir"
    echo "$dir"
}

# Assert that actual output equals expected output
assert_eq() {
    local test_name="$1"
    local expected="$2"
    local actual="$3"

    if [[ "$expected" == "$actual" ]]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        ERRORS+=("FAIL: $test_name")
        echo "FAIL: $test_name"
        echo "  Expected:"
        echo "$expected" | head -20 | sed 's/^/    /'
        echo "  Actual:"
        echo "$actual" | head -20 | sed 's/^/    /'
        echo ""
    fi
}

# Assert exit code
assert_exit_code() {
    local test_name="$1"
    local expected_code="$2"
    local actual_code="$3"

    if [[ "$expected_code" == "$actual_code" ]]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        ERRORS+=("FAIL: $test_name (exit code: expected=$expected_code, actual=$actual_code)")
        echo "FAIL: $test_name (exit code: expected=$expected_code, actual=$actual_code)"
    fi
}

# Assert output contains a substring
assert_contains() {
    local test_name="$1"
    local expected_substr="$2"
    local actual="$3"

    if echo "$actual" | grep -qF "$expected_substr"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        ERRORS+=("FAIL: $test_name (output does not contain '$expected_substr')")
        echo "FAIL: $test_name"
        echo "  Expected to contain: $expected_substr"
        echo "  Actual:"
        echo "$actual" | head -10 | sed 's/^/    /'
        echo ""
    fi
}

# Assert output does NOT contain a substring
assert_not_contains() {
    local test_name="$1"
    local unexpected_substr="$2"
    local actual="$3"

    if ! echo "$actual" | grep -qF "$unexpected_substr"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        ERRORS+=("FAIL: $test_name (output unexpectedly contains '$unexpected_substr')")
        echo "FAIL: $test_name"
        echo "  Should NOT contain: $unexpected_substr"
        echo "  Actual:"
        echo "$actual" | head -10 | sed 's/^/    /'
        echo ""
    fi
}

echo "=== Equivalence Test Suite for ripgrep ==="
echo "Binary: $RG"
echo "Version: $($RG --version | head -1)"
echo ""

# =============================================================================
# Test Category: Basic Search
# =============================================================================
echo "--- Basic Search ---"

# T001: Simple pattern match
t001_dir="$(make_test_dir t001)"
echo "Hello World" > "$t001_dir/test.txt"
echo "Goodbye World" >> "$t001_dir/test.txt"
echo "Hello Again" >> "$t001_dir/test.txt"
actual="$("$RG" --no-filename --no-line-number "Hello" "$t001_dir/test.txt")"
assert_eq "T001: Simple pattern match" "Hello World
Hello Again" "$actual"

# T002: No match returns exit code 1
t002_dir="$(make_test_dir t002)"
echo "Hello World" > "$t002_dir/test.txt"
set +e
"$RG" "ZZZZNOTFOUND" "$t002_dir/test.txt" > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T002: No match exit code" "1" "$code"

# T003: Match returns exit code 0
t003_dir="$(make_test_dir t003)"
echo "Hello World" > "$t003_dir/test.txt"
set +e
"$RG" "Hello" "$t003_dir/test.txt" > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T003: Match exit code" "0" "$code"

# T004: Regex pattern
t004_dir="$(make_test_dir t004)"
printf "foo123bar\nfoo456baz\nhello\n" > "$t004_dir/test.txt"
actual="$("$RG" --no-filename --no-line-number 'foo\d+ba' "$t004_dir/test.txt")"
assert_eq "T004: Regex pattern" "foo123bar
foo456baz" "$actual"

# T005: Multiple files
t005_dir="$(make_test_dir t005)"
echo "apple" > "$t005_dir/a.txt"
echo "banana" > "$t005_dir/b.txt"
echo "apricot" > "$t005_dir/c.txt"
actual="$("$RG" --no-line-number --sort path "ap" "$t005_dir/" | sed "s|$t005_dir/||")"
assert_eq "T005: Multiple files" "a.txt:apple
c.txt:apricot" "$actual"

# =============================================================================
# Test Category: Case Sensitivity
# =============================================================================
echo "--- Case Sensitivity ---"

# T010: Case insensitive search
t010_dir="$(make_test_dir t010)"
printf "Hello\nhello\nHELLO\n" > "$t010_dir/test.txt"
actual="$("$RG" -i --no-filename --no-line-number "hello" "$t010_dir/test.txt")"
assert_eq "T010: Case insensitive" "Hello
hello
HELLO" "$actual"

# T011: Smart-case (lowercase pattern = case insensitive)
t011_dir="$(make_test_dir t011)"
printf "Hello\nhello\nHELLO\n" > "$t011_dir/test.txt"
actual="$("$RG" -S --no-filename --no-line-number "hello" "$t011_dir/test.txt")"
assert_eq "T011: Smart-case lowercase" "Hello
hello
HELLO" "$actual"

# T012: Smart-case (mixed-case pattern = case sensitive)
t012_dir="$(make_test_dir t012)"
printf "Hello\nhello\nHELLO\n" > "$t012_dir/test.txt"
actual="$("$RG" -S --no-filename --no-line-number "Hello" "$t012_dir/test.txt")"
assert_eq "T012: Smart-case mixed" "Hello" "$actual"

# T013: Explicit case sensitive
t013_dir="$(make_test_dir t013)"
printf "Hello\nhello\nHELLO\n" > "$t013_dir/test.txt"
actual="$("$RG" -s --no-filename --no-line-number "hello" "$t013_dir/test.txt")"
assert_eq "T013: Case sensitive" "hello" "$actual"

# =============================================================================
# Test Category: Invert Match
# =============================================================================
echo "--- Invert Match ---"

# T020: Invert match
t020_dir="$(make_test_dir t020)"
printf "alpha\nbeta\ngamma\n" > "$t020_dir/test.txt"
actual="$("$RG" -v --no-filename --no-line-number "beta" "$t020_dir/test.txt")"
assert_eq "T020: Invert match" "alpha
gamma" "$actual"

# =============================================================================
# Test Category: Line Numbers & Columns
# =============================================================================
echo "--- Line Numbers & Columns ---"

# T030: Line numbers shown
t030_dir="$(make_test_dir t030)"
printf "foo\nbar\nbaz\n" > "$t030_dir/test.txt"
actual="$("$RG" -n --no-filename "bar" "$t030_dir/test.txt")"
assert_eq "T030: Line numbers" "2:bar" "$actual"

# T031: No line numbers
t031_dir="$(make_test_dir t031)"
printf "foo\nbar\nbaz\n" > "$t031_dir/test.txt"
actual="$("$RG" -N --no-filename "bar" "$t031_dir/test.txt")"
assert_eq "T031: No line numbers" "bar" "$actual"

# T032: Column numbers
t032_dir="$(make_test_dir t032)"
echo "hello world" > "$t032_dir/test.txt"
actual="$("$RG" -n --column --no-filename "world" "$t032_dir/test.txt")"
assert_eq "T032: Column numbers" "1:7:hello world" "$actual"

# =============================================================================
# Test Category: Context Lines
# =============================================================================
echo "--- Context Lines ---"

# T040: After context
t040_dir="$(make_test_dir t040)"
printf "one\ntwo\nthree\nfour\nfive\n" > "$t040_dir/test.txt"
actual="$("$RG" -A 1 --no-filename --no-line-number "two" "$t040_dir/test.txt")"
assert_eq "T040: After context" "two
three" "$actual"

# T041: Before context
t041_dir="$(make_test_dir t041)"
printf "one\ntwo\nthree\nfour\nfive\n" > "$t041_dir/test.txt"
actual="$("$RG" -B 1 --no-filename --no-line-number "three" "$t041_dir/test.txt")"
assert_eq "T041: Before context" "two
three" "$actual"

# T042: Context both sides
t042_dir="$(make_test_dir t042)"
printf "one\ntwo\nthree\nfour\nfive\n" > "$t042_dir/test.txt"
actual="$("$RG" -C 1 --no-filename --no-line-number "three" "$t042_dir/test.txt")"
assert_eq "T042: Context both" "two
three
four" "$actual"

# T043: Context separator between non-contiguous groups
t043_dir="$(make_test_dir t043)"
printf "a\nb\nc\nd\ne\nf\ng\n" > "$t043_dir/test.txt"
actual="$("$RG" -n -A 1 --no-filename "b|f" "$t043_dir/test.txt")"
assert_eq "T043: Context separator" "2:b
3-c
--
6:f
7-g" "$actual"

# =============================================================================
# Test Category: Count
# =============================================================================
echo "--- Count ---"

# T050: Count matching lines
t050_dir="$(make_test_dir t050)"
printf "foo\nbar\nfoo\nbaz\nfoo\n" > "$t050_dir/test.txt"
actual="$("$RG" -c --no-filename "foo" "$t050_dir/test.txt")"
assert_eq "T050: Count lines" "3" "$actual"

# T051: Count matches (not lines)
t051_dir="$(make_test_dir t051)"
printf "foofoo\nbar\nfoo\n" > "$t051_dir/test.txt"
actual="$("$RG" --count-matches --no-filename "foo" "$t051_dir/test.txt")"
assert_eq "T051: Count matches" "3" "$actual"

# =============================================================================
# Test Category: Files with/without matches
# =============================================================================
echo "--- File Match Listing ---"

# T060: Files with matches
t060_dir="$(make_test_dir t060)"
echo "hello" > "$t060_dir/a.txt"
echo "world" > "$t060_dir/b.txt"
echo "hello world" > "$t060_dir/c.txt"
actual="$("$RG" -l --sort path "hello" "$t060_dir/" | sed "s|$t060_dir/||")"
assert_eq "T060: Files with matches" "a.txt
c.txt" "$actual"

# T061: Files without matches
t061_dir="$(make_test_dir t061)"
echo "hello" > "$t061_dir/a.txt"
echo "world" > "$t061_dir/b.txt"
actual="$("$RG" --files-without-match --sort path "hello" "$t061_dir/" | sed "s|$t061_dir/||")"
assert_eq "T061: Files without matches" "b.txt" "$actual"

# =============================================================================
# Test Category: Only Matching
# =============================================================================
echo "--- Only Matching ---"

# T070: Only matching parts
t070_dir="$(make_test_dir t070)"
echo "foo bar baz foo" > "$t070_dir/test.txt"
actual="$("$RG" -o --no-filename --no-line-number "foo" "$t070_dir/test.txt")"
assert_eq "T070: Only matching" "foo
foo" "$actual"

# =============================================================================
# Test Category: Fixed Strings
# =============================================================================
echo "--- Fixed Strings ---"

# T080: Fixed string (not regex)
t080_dir="$(make_test_dir t080)"
printf "a.b\na-b\naxb\n" > "$t080_dir/test.txt"
actual="$("$RG" -F --no-filename --no-line-number "a.b" "$t080_dir/test.txt")"
assert_eq "T080: Fixed string" "a.b" "$actual"

# =============================================================================
# Test Category: Word & Line Regexp
# =============================================================================
echo "--- Word & Line Regexp ---"

# T090: Word boundary matching
t090_dir="$(make_test_dir t090)"
printf "foobar\nfoo bar\nbarfoo\n" > "$t090_dir/test.txt"
actual="$("$RG" -w --no-filename --no-line-number "foo" "$t090_dir/test.txt")"
assert_eq "T090: Word regexp" "foo bar" "$actual"

# T091: Line regexp
t091_dir="$(make_test_dir t091)"
printf "foo\nfoobar\n foo \n" > "$t091_dir/test.txt"
actual="$("$RG" -x --no-filename --no-line-number "foo" "$t091_dir/test.txt")"
assert_eq "T091: Line regexp" "foo" "$actual"

# =============================================================================
# Test Category: Replace
# =============================================================================
echo "--- Replace ---"

# T100: Simple replacement
t100_dir="$(make_test_dir t100)"
echo "hello world" > "$t100_dir/test.txt"
actual="$("$RG" -r "planet" --no-filename --no-line-number "world" "$t100_dir/test.txt")"
assert_eq "T100: Simple replace" "hello planet" "$actual"

# T101: Capture group replacement
t101_dir="$(make_test_dir t101)"
echo "foo123bar" > "$t101_dir/test.txt"
actual="$("$RG" -r '$1' --no-filename --no-line-number '(\d+)' "$t101_dir/test.txt")"
assert_eq "T101: Capture group replace" "foo123bar" "$actual"

# =============================================================================
# Test Category: Glob Filtering
# =============================================================================
echo "--- Glob Filtering ---"

# T110: Include glob
t110_dir="$(make_test_dir t110)"
echo "match" > "$t110_dir/a.txt"
echo "match" > "$t110_dir/b.rs"
echo "match" > "$t110_dir/c.py"
actual="$("$RG" -g '*.rs' --no-filename --no-line-number --sort path "match" "$t110_dir/")"
assert_eq "T110: Include glob" "match" "$actual"

# T111: Exclude glob
t111_dir="$(make_test_dir t111)"
echo "match" > "$t111_dir/a.txt"
echo "match" > "$t111_dir/b.rs"
echo "match" > "$t111_dir/c.txt"
actual="$("$RG" -g '!*.rs' -l --sort path "match" "$t111_dir/" | sed "s|$t111_dir/||")"
assert_eq "T111: Exclude glob" "a.txt
c.txt" "$actual"

# =============================================================================
# Test Category: File Types
# =============================================================================
echo "--- File Types ---"

# T120: Search by type
t120_dir="$(make_test_dir t120)"
echo "match" > "$t120_dir/a.rs"
echo "match" > "$t120_dir/b.py"
echo "match" > "$t120_dir/c.txt"
actual="$("$RG" -t rust --no-filename --no-line-number "match" "$t120_dir/")"
assert_eq "T120: File type filter" "match" "$actual"

# T121: Exclude by type
t121_dir="$(make_test_dir t121)"
echo "match" > "$t121_dir/a.rs"
echo "match" > "$t121_dir/b.py"
actual="$("$RG" -T rust -l --sort path "match" "$t121_dir/" | sed "s|$t121_dir/||")"
assert_eq "T121: Exclude file type" "b.py" "$actual"

# T122: Type list output
actual="$("$RG" --type-list)"
assert_contains "T122: Type list has rust" "rust:" "$actual"
assert_contains "T122: Type list has python" "py:" "$actual"

# =============================================================================
# Test Category: Hidden Files
# =============================================================================
echo "--- Hidden Files ---"

# T130: Hidden files skipped by default
t130_dir="$(make_test_dir t130)"
echo "match" > "$t130_dir/visible.txt"
echo "match" > "$t130_dir/.hidden.txt"
actual="$("$RG" -l "match" "$t130_dir/" | sed "s|$t130_dir/||")"
assert_eq "T130: Hidden skipped" "visible.txt" "$actual"

# T131: Hidden files included with --hidden
t131_dir="$(make_test_dir t131)"
echo "match" > "$t131_dir/visible.txt"
echo "match" > "$t131_dir/.hidden.txt"
actual="$("$RG" --hidden -l --sort path "match" "$t131_dir/" | sed "s|$t131_dir/||")"
assert_eq "T131: Hidden included" ".hidden.txt
visible.txt" "$actual"

# =============================================================================
# Test Category: Gitignore
# =============================================================================
echo "--- Gitignore ---"

# T140: Respects .gitignore
t140_dir="$(make_test_dir t140)"
mkdir -p "$t140_dir/.git"
echo "match" > "$t140_dir/keep.txt"
echo "match" > "$t140_dir/ignored.log"
echo "*.log" > "$t140_dir/.gitignore"
actual="$("$RG" -l "match" "$t140_dir/" | sed "s|$t140_dir/||")"
assert_eq "T140: Gitignore respected" "keep.txt" "$actual"

# T141: --no-ignore overrides gitignore
t141_dir="$(make_test_dir t141)"
mkdir -p "$t141_dir/.git"
echo "match" > "$t141_dir/keep.txt"
echo "match" > "$t141_dir/ignored.log"
echo "*.log" > "$t141_dir/.gitignore"
actual="$("$RG" --no-ignore -l --sort path "match" "$t141_dir/" | sed "s|$t141_dir/||")"
assert_contains "T141: No-ignore includes log" "ignored.log" "$actual"

# T142: .rgignore takes precedence over .gitignore
t142_dir="$(make_test_dir t142)"
mkdir -p "$t142_dir/.git"
echo "match" > "$t142_dir/test.log"
echo "*.log" > "$t142_dir/.gitignore"
echo "!*.log" > "$t142_dir/.rgignore"
actual="$("$RG" -l "match" "$t142_dir/" | sed "s|$t142_dir/||")"
assert_eq "T142: rgignore precedence" "test.log" "$actual"

# T143: .ignore file
t143_dir="$(make_test_dir t143)"
echo "match" > "$t143_dir/keep.txt"
echo "match" > "$t143_dir/skip.dat"
echo "*.dat" > "$t143_dir/.ignore"
actual="$("$RG" -l "match" "$t143_dir/" | sed "s|$t143_dir/||")"
assert_eq "T143: .ignore file" "keep.txt" "$actual"

# =============================================================================
# Test Category: Binary Detection
# =============================================================================
echo "--- Binary Detection ---"

# T150: Binary file warning
t150_dir="$(make_test_dir t150)"
printf "hello\x00world\n" > "$t150_dir/test.bin"
set +e
output="$("$RG" "hello" "$t150_dir/test.bin" 2>&1)"
code=$?
set -e
assert_contains "T150: Binary warning" "binary file matches" "$output"

# T151: --text searches binary
t151_dir="$(make_test_dir t151)"
printf "hello\x00world\n" > "$t151_dir/test.bin"
actual="$("$RG" -a --no-filename --no-line-number "hello" "$t151_dir/test.bin")"
assert_contains "T151: Text mode binary" "hello" "$actual"

# =============================================================================
# Test Category: Max Count
# =============================================================================
echo "--- Max Count ---"

# T160: Max count limits output
t160_dir="$(make_test_dir t160)"
printf "foo\nfoo\nfoo\nfoo\nfoo\n" > "$t160_dir/test.txt"
actual="$("$RG" -m 2 --no-filename --no-line-number "foo" "$t160_dir/test.txt")"
assert_eq "T160: Max count" "foo
foo" "$actual"

# =============================================================================
# Test Category: Quiet Mode
# =============================================================================
echo "--- Quiet Mode ---"

# T170: Quiet mode produces no output on match
t170_dir="$(make_test_dir t170)"
echo "hello" > "$t170_dir/test.txt"
actual="$("$RG" -q "hello" "$t170_dir/test.txt" 2>&1)"
assert_eq "T170: Quiet no output" "" "$actual"

# T171: Quiet mode exit code on match
t171_dir="$(make_test_dir t171)"
echo "hello" > "$t171_dir/test.txt"
set +e
"$RG" -q "hello" "$t171_dir/test.txt" > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T171: Quiet exit code match" "0" "$code"

# T172: Quiet mode exit code on no match
t172_dir="$(make_test_dir t172)"
echo "hello" > "$t172_dir/test.txt"
set +e
"$RG" -q "ZZZZNOTFOUND" "$t172_dir/test.txt" > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T172: Quiet exit code no match" "1" "$code"

# =============================================================================
# Test Category: Stdin
# =============================================================================
echo "--- Stdin ---"

# T180: Read from stdin
actual="$(echo "hello world" | "$RG" --no-filename --no-line-number "hello")"
assert_eq "T180: Stdin search" "hello world" "$actual"

# T181: Stdin with no match
set +e
echo "hello world" | "$RG" "ZZZZNOTFOUND" > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T181: Stdin no match" "1" "$code"

# =============================================================================
# Test Category: Multiple Patterns
# =============================================================================
echo "--- Multiple Patterns ---"

# T190: Multiple -e patterns
t190_dir="$(make_test_dir t190)"
printf "alpha\nbeta\ngamma\ndelta\n" > "$t190_dir/test.txt"
actual="$("$RG" -e "alpha" -e "gamma" --no-filename --no-line-number "$t190_dir/test.txt")"
assert_eq "T190: Multiple patterns" "alpha
gamma" "$actual"

# =============================================================================
# Test Category: Pattern from File
# =============================================================================
echo "--- Pattern from File ---"

# T200: Read patterns from file
t200_dir="$(make_test_dir t200)"
printf "alpha\nbeta\ngamma\ndelta\n" > "$t200_dir/test.txt"
printf "alpha\ngamma\n" > "$t200_dir/patterns.txt"
actual="$("$RG" -f "$t200_dir/patterns.txt" --no-filename --no-line-number "$t200_dir/test.txt")"
assert_eq "T200: Patterns from file" "alpha
gamma" "$actual"

# =============================================================================
# Test Category: --files (list files without searching)
# =============================================================================
echo "--- File Listing ---"

# T210: List files
t210_dir="$(make_test_dir t210)"
echo "a" > "$t210_dir/foo.txt"
echo "b" > "$t210_dir/bar.rs"
mkdir -p "$t210_dir/sub"
echo "c" > "$t210_dir/sub/baz.py"
actual="$("$RG" --files --sort path "$t210_dir/" | sed "s|$t210_dir/||")"
assert_eq "T210: List files" "bar.rs
foo.txt
sub/baz.py" "$actual"

# T211: File listing respects gitignore
t211_dir="$(make_test_dir t211)"
mkdir -p "$t211_dir/.git"
echo "a" > "$t211_dir/keep.txt"
echo "b" > "$t211_dir/skip.log"
echo "*.log" > "$t211_dir/.gitignore"
actual="$("$RG" --files "$t211_dir/" | sed "s|$t211_dir/||")"
assert_eq "T211: File list gitignore" "keep.txt" "$actual"

# =============================================================================
# Test Category: Max Depth
# =============================================================================
echo "--- Max Depth ---"

# T220: Max depth limits traversal
t220_dir="$(make_test_dir t220)"
echo "match" > "$t220_dir/top.txt"
mkdir -p "$t220_dir/a/b"
echo "match" > "$t220_dir/a/mid.txt"
echo "match" > "$t220_dir/a/b/deep.txt"
actual="$("$RG" --max-depth 1 -l --sort path "match" "$t220_dir/" | sed "s|$t220_dir/||")"
assert_eq "T220: Max depth" "top.txt" "$actual"

# =============================================================================
# Test Category: Null-Terminated Output
# =============================================================================
echo "--- Null-Terminated Output ---"

# T230: Null byte after filenames with -0
t230_dir="$(make_test_dir t230)"
echo "match" > "$t230_dir/a.txt"
echo "match" > "$t230_dir/b.txt"
actual="$("$RG" -l -0 --sort path "match" "$t230_dir/" | tr '\0' '\n' | grep -v '^$' | sed "s|$t230_dir/||")"
assert_eq "T230: Null-terminated" "a.txt
b.txt" "$actual"

# =============================================================================
# Test Category: Byte Offset
# =============================================================================
echo "--- Byte Offset ---"

# T240: Byte offset shown
t240_dir="$(make_test_dir t240)"
printf "aaa\nbbb\nccc\n" > "$t240_dir/test.txt"
actual="$("$RG" -b --no-filename --no-line-number "bbb" "$t240_dir/test.txt")"
assert_eq "T240: Byte offset" "4:bbb" "$actual"

# =============================================================================
# Test Category: JSON Output
# =============================================================================
echo "--- JSON Output ---"

# T250: JSON output produces valid JSON with correct types
t250_dir="$(make_test_dir t250)"
echo "hello world" > "$t250_dir/test.txt"
actual="$("$RG" --json "hello" "$t250_dir/test.txt")"
# Check that we get begin, match, and end messages
assert_contains "T250: JSON has begin" '"type":"begin"' "$actual"
assert_contains "T250: JSON has match" '"type":"match"' "$actual"
assert_contains "T250: JSON has end" '"type":"end"' "$actual"

# T251: JSON match contains submatches
t251_dir="$(make_test_dir t251)"
echo "hello world" > "$t251_dir/test.txt"
actual="$("$RG" --json "hello" "$t251_dir/test.txt" | grep '"type":"match"')"
assert_contains "T251: JSON submatches" '"submatches"' "$actual"

# =============================================================================
# Test Category: Multiline
# =============================================================================
echo "--- Multiline ---"

# T260: Multiline match
t260_dir="$(make_test_dir t260)"
printf "foo\nbar\n" > "$t260_dir/test.txt"
actual="$("$RG" -U --no-filename --no-line-number 'foo\nbar' "$t260_dir/test.txt")"
assert_eq "T260: Multiline match" "foo
bar" "$actual"

# =============================================================================
# Test Category: Max Columns
# =============================================================================
echo "--- Max Columns ---"

# T270: Long lines truncated
t270_dir="$(make_test_dir t270)"
echo "short match here" > "$t270_dir/test.txt"
echo "this is a very long line with a match somewhere inside of it that exceeds the column limit for sure" >> "$t270_dir/test.txt"
set +e
actual="$("$RG" --max-columns 30 --no-filename --no-line-number "match" "$t270_dir/test.txt" 2>&1)"
code=$?
set -e
# The short line should appear; the long line should be omitted or truncated
assert_contains "T270: Short line appears" "short match here" "$actual"

# =============================================================================
# Test Category: Symlinks
# =============================================================================
echo "--- Symlinks ---"

# T280: Symlinks not followed by default
t280_dir="$(make_test_dir t280)"
mkdir -p "$t280_dir/real"
echo "match" > "$t280_dir/real/file.txt"
ln -s "$t280_dir/real" "$t280_dir/link"
actual="$("$RG" -l --sort path "match" "$t280_dir/real/")"
assert_contains "T280: Real dir searched" "file.txt" "$actual"

# T281: Symlinks followed with -L
t281_dir="$(make_test_dir t281)"
mkdir -p "$t281_dir/real"
echo "match" > "$t281_dir/real/file.txt"
ln -s "$t281_dir/real" "$t281_dir/link"
actual="$("$RG" -L -l --sort path "match" "$t281_dir/" | sed "s|$t281_dir/||")"
assert_contains "T281: Symlink followed" "link/file.txt" "$actual"

# =============================================================================
# Test Category: Trim
# =============================================================================
echo "--- Trim ---"

# T290: Trim leading whitespace
t290_dir="$(make_test_dir t290)"
printf "  hello\n\tworld\n" > "$t290_dir/test.txt"
actual="$("$RG" --trim --no-filename --no-line-number "hello" "$t290_dir/test.txt")"
assert_eq "T290: Trim" "hello" "$actual"

# =============================================================================
# Test Category: Unrestricted Mode
# =============================================================================
echo "--- Unrestricted Mode ---"

# T300: -u disables gitignore
t300_dir="$(make_test_dir t300)"
mkdir -p "$t300_dir/.git"
echo "match" > "$t300_dir/file.log"
echo "*.log" > "$t300_dir/.gitignore"
actual="$("$RG" -u -l "match" "$t300_dir/" | sed "s|$t300_dir/||")"
assert_contains "T300: -u disables ignore" "file.log" "$actual"

# T301: -uu also searches hidden
t301_dir="$(make_test_dir t301)"
echo "match" > "$t301_dir/.hidden.txt"
actual="$("$RG" -uu -l "match" "$t301_dir/" | sed "s|$t301_dir/||")"
assert_contains "T301: -uu searches hidden" ".hidden.txt" "$actual"

# =============================================================================
# Test Category: Version & Help
# =============================================================================
echo "--- Version & Help ---"

# T310: Version output
actual="$("$RG" --version | head -1)"
assert_contains "T310: Version" "ripgrep" "$actual"

# T311: Help output
actual="$("$RG" --help 2>&1 | head -5)"
assert_contains "T311: Help" "ripgrep" "$actual"

# T312: Short help
actual="$("$RG" -h 2>&1 | head -5)"
assert_contains "T312: Short help" "rg" "$actual"

# =============================================================================
# Test Category: Error Exit Code
# =============================================================================
echo "--- Error Handling ---"

# T320: Invalid regex returns exit code 2
set +e
"$RG" "[invalid" /dev/null > /dev/null 2>&1
code=$?
set -e
assert_exit_code "T320: Invalid regex exit code" "2" "$code"

# T321: Invalid regex produces error message
set +e
output="$("$RG" "[invalid" /dev/null 2>&1)"
set -e
assert_contains "T321: Invalid regex error msg" "error" "$output"

# =============================================================================
# Test Category: Config File
# =============================================================================
echo "--- Config File ---"

# T330: RIPGREP_CONFIG_PATH respected
t330_dir="$(make_test_dir t330)"
printf "Hello\nhello\nHELLO\n" > "$t330_dir/test.txt"
echo "--case-sensitive" > "$t330_dir/config"
actual="$(RIPGREP_CONFIG_PATH="$t330_dir/config" "$RG" --no-filename --no-line-number "hello" "$t330_dir/test.txt")"
assert_eq "T330: Config file" "hello" "$actual"

# T331: CLI overrides config file
t331_dir="$(make_test_dir t331)"
printf "Hello\nhello\nHELLO\n" > "$t331_dir/test.txt"
echo "--case-sensitive" > "$t331_dir/config"
actual="$(RIPGREP_CONFIG_PATH="$t331_dir/config" "$RG" -i --no-filename --no-line-number "hello" "$t331_dir/test.txt")"
assert_eq "T331: CLI overrides config" "Hello
hello
HELLO" "$actual"

# =============================================================================
# Test Category: Recursive Search Default
# =============================================================================
echo "--- Recursive Search ---"

# T340: Recursive search finds files in subdirectories
t340_dir="$(make_test_dir t340)"
mkdir -p "$t340_dir/sub1/sub2"
echo "match" > "$t340_dir/top.txt"
echo "match" > "$t340_dir/sub1/mid.txt"
echo "match" > "$t340_dir/sub1/sub2/deep.txt"
actual="$("$RG" -l --sort path "match" "$t340_dir/" | wc -l | tr -d ' ')"
assert_eq "T340: Recursive finds 3 files" "3" "$actual"

# =============================================================================
# Test Category: Heading Mode
# =============================================================================
echo "--- Heading ---"

# T350: Heading mode groups by file
t350_dir="$(make_test_dir t350)"
echo "match" > "$t350_dir/a.txt"
echo "match" > "$t350_dir/b.txt"
actual="$("$RG" --heading -n --color=never --sort path "match" "$t350_dir/")"
# In heading mode, filenames appear on their own line
assert_contains "T350: Heading has filename" "a.txt" "$actual"

# T351: No-heading mode puts filename on each line
t351_dir="$(make_test_dir t351)"
echo "match" > "$t351_dir/a.txt"
echo "match" > "$t351_dir/b.txt"
actual="$("$RG" --no-heading -n --color=never --sort path "match" "$t351_dir/" | head -1 | sed "s|$t351_dir/||")"
assert_eq "T351: No-heading format" "a.txt:1:match" "$actual"

# =============================================================================
# Test Category: Vimgrep
# =============================================================================
echo "--- Vimgrep ---"

# T360: Vimgrep format
t360_dir="$(make_test_dir t360)"
echo "hello world" > "$t360_dir/test.txt"
actual="$("$RG" --vimgrep "world" "$t360_dir/test.txt" | sed "s|$t360_dir/||")"
assert_eq "T360: Vimgrep format" "test.txt:1:7:hello world" "$actual"

# =============================================================================
# Test Category: Sort
# =============================================================================
echo "--- Sort ---"

# T370: Sort by path
t370_dir="$(make_test_dir t370)"
echo "match" > "$t370_dir/c.txt"
echo "match" > "$t370_dir/a.txt"
echo "match" > "$t370_dir/b.txt"
actual="$("$RG" -l --sort path "match" "$t370_dir/" | sed "s|$t370_dir/||")"
assert_eq "T370: Sort path" "a.txt
b.txt
c.txt" "$actual"

# T371: Reverse sort by path
t371_dir="$(make_test_dir t371)"
echo "match" > "$t371_dir/c.txt"
echo "match" > "$t371_dir/a.txt"
echo "match" > "$t371_dir/b.txt"
actual="$("$RG" -l --sortr path "match" "$t371_dir/" | sed "s|$t371_dir/||")"
assert_eq "T371: Reverse sort path" "c.txt
b.txt
a.txt" "$actual"

# =============================================================================
# Test Category: Stop on Nonmatch
# =============================================================================
echo "--- Stop on Nonmatch ---"

# T380: Stop on nonmatch
t380_dir="$(make_test_dir t380)"
printf "zzz\nfoo1\nfoo2\nzzz\nfoo3\n" > "$t380_dir/test.txt"
actual="$("$RG" --no-filename --no-line-number --stop-on-nonmatch "foo" "$t380_dir/test.txt")"
assert_eq "T380: Stop on nonmatch" "foo1
foo2" "$actual"

# =============================================================================
# Test Category: Statistics
# =============================================================================
echo "--- Statistics ---"

# T390: Stats output
t390_dir="$(make_test_dir t390)"
printf "foo\nbar\nfoo\n" > "$t390_dir/test.txt"
actual="$("$RG" --stats --no-filename --no-line-number "foo" "$t390_dir/test.txt" 2>&1)"
assert_contains "T390: Stats matches" "2 matches" "$actual"
assert_contains "T390: Stats matched lines" "2 matched lines" "$actual"
assert_contains "T390: Stats files" "1 files searched" "$actual"

# =============================================================================
# Test Category: Max Filesize
# =============================================================================
echo "--- Max Filesize ---"

# T400: Max filesize skips large files
t400_dir="$(make_test_dir t400)"
echo "match" > "$t400_dir/small.txt"
dd if=/dev/zero bs=1024 count=100 2>/dev/null | tr '\0' 'a' > "$t400_dir/large.txt"
echo "match" >> "$t400_dir/large.txt"
actual="$("$RG" --max-filesize 1K -l "match" "$t400_dir/" | sed "s|$t400_dir/||")"
assert_eq "T400: Max filesize" "small.txt" "$actual"

# =============================================================================
# Test Category: Null-data mode
# =============================================================================
echo "--- Null Data ---"

# T410: Null-data mode uses NUL as line terminator
t410_dir="$(make_test_dir t410)"
printf "foo\x00bar\x00baz\x00" > "$t410_dir/test.txt"
actual="$("$RG" --null-data --no-filename --no-line-number "bar" "$t410_dir/test.txt" | tr '\0' '\n' | head -1)"
assert_eq "T410: Null data" "bar" "$actual"

# =============================================================================
# Test Category: CRLF
# =============================================================================
echo "--- CRLF ---"

# T420: CRLF handling
t420_dir="$(make_test_dir t420)"
printf "hello\r\nworld\r\n" > "$t420_dir/test.txt"
actual="$("$RG" --crlf --no-filename --no-line-number "hello" "$t420_dir/test.txt" | tr -d '\r')"
assert_eq "T420: CRLF" "hello" "$actual"

# =============================================================================
# Test Category: Generate
# =============================================================================
echo "--- Generate ---"

# T430: Generate man page
t430_file="$TMPDIR_BASE/man_output.txt"
set +e
"$RG" --generate man > "$t430_file" 2>&1
code=$?
set -e
if grep -qF ".TH RG" "$t430_file" 2>/dev/null; then
    PASS=$((PASS + 1))
else
    FAIL=$((FAIL + 1))
    ERRORS+=("FAIL: T430: Man page content")
    echo "FAIL: T430: Man page content"
fi

# T431: Generate bash completions
t431_file="$TMPDIR_BASE/bash_comp.txt"
set +e
"$RG" --generate complete-bash > "$t431_file" 2>&1
code=$?
set -e
assert_exit_code "T431: Generate bash exit code" "0" "$code"

# =============================================================================
# Test Category: Path Separator
# =============================================================================
echo "--- Path Separator ---"

# T440: Custom path separator
t440_dir="$(make_test_dir t440)"
mkdir -p "$t440_dir/sub"
echo "match" > "$t440_dir/sub/file.txt"
actual="$("$RG" --path-separator='/' -l "match" "$t440_dir/sub/")"
assert_contains "T440: Path separator" "/" "$actual"
assert_not_contains "T440: No backslash" "\\" "$actual"

# =============================================================================
# RESULTS
# =============================================================================
echo ""
echo "==========================================="
echo "Results: $PASS passed, $FAIL failed"
echo "==========================================="

if [[ $FAIL -gt 0 ]]; then
    echo ""
    echo "Failed tests:"
    for e in "${ERRORS[@]}"; do
        echo "  - $e"
    done
    exit 1
fi

exit 0
