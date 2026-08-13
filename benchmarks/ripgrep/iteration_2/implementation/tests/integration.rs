use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// Get the path to the compiled `rg` binary.
fn rg_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // pop test exe name
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("rg");
    if !path.exists() {
        // Fall back to target/debug/rg
        path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("rg");
    }
    path
}

/// Helper to run `rg` with arguments in a given working directory.
fn run_rg(args: &[&str], cwd: Option<&Path>) -> (i32, String, String) {
    let mut cmd = Command::new(rg_bin());
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().expect("failed to execute rg");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

/// Helper to run `rg` with stdin input.
fn run_rg_stdin(args: &[&str], stdin_data: &str) -> (i32, String, String) {
    let mut cmd = Command::new(rg_bin());
    cmd.args(args);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn rg");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_data.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().expect("failed to read rg output");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

// ---------------------------------------------------------------------------
// Basic Search Tests
// ---------------------------------------------------------------------------

#[test]
fn test_basic_search() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "apple\nbanana\ncherry\napple pie\n").unwrap();

    let (code, stdout, _) = run_rg(&["apple", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("apple"));
    assert!(stdout.contains("apple pie"));
    assert!(!stdout.contains("banana"));
}

#[test]
fn test_no_match_exit_code() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "apple\nbanana\ncherry\n").unwrap();

    let (code, stdout, _) = run_rg(&["orange", file.to_str().unwrap()], None);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
}

#[test]
fn test_line_number() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "first\nsecond\nthird match\nfourth\n").unwrap();

    let (code, stdout, _) = run_rg(&["-n", "third", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("3:third match") || stdout.contains("3:"));
}

#[test]
fn test_no_line_number() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "first\nsecond match\n").unwrap();

    let (code, stdout, _) = run_rg(&["-N", "second", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(!stdout.contains("2:"));
    assert!(stdout.contains("second match"));
}

// ---------------------------------------------------------------------------
// Case Sensitivity & Smart Case Tests
// ---------------------------------------------------------------------------

#[test]
fn test_smart_case_default() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Foo\nfoo\nFOO\nbar\n").unwrap();

    // All-lowercase pattern -> case-insensitive (smart-case)
    let (code, stdout, _) = run_rg(&["foo", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("foo"));
    assert!(stdout.contains("FOO"));

    // Pattern with uppercase -> case-sensitive
    let (code, stdout, _) = run_rg(&["Foo", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("Foo"));
    assert!(!stdout.contains("foo\n"));
}

#[test]
fn test_explicit_ignore_case() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Apple\napple\nAPPLE\n").unwrap();

    let (code, stdout, _) = run_rg(&["-i", "Apple", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("Apple"));
    assert!(stdout.contains("apple"));
    assert!(stdout.contains("APPLE"));
}

#[test]
fn test_explicit_case_sensitive() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "apple\nApple\nAPPLE\n").unwrap();

    let (code, stdout, _) = run_rg(&["-s", "apple", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("apple"));
    assert!(!stdout.contains("Apple"));
    assert!(!stdout.contains("APPLE"));
}

// ---------------------------------------------------------------------------
// Pattern Modes (-w, -x, -F, -v)
// ---------------------------------------------------------------------------

#[test]
fn test_word_regexp() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "cat\nconcatenate\ncategory\na cat sits\n").unwrap();

    let (code, stdout, _) = run_rg(&["-w", "-s", "cat", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("cat\n") || stdout.contains(":cat\n"));
    assert!(stdout.contains("a cat sits"));
    assert!(!stdout.contains("concatenate"));
    assert!(!stdout.contains("category"));
}

#[test]
fn test_line_regexp() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\nhello world\nsay hello\n").unwrap();

    let (code, stdout, _) = run_rg(&["-x", "-s", "hello", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("hello"));
    assert!(!stdout.contains("hello world"));
    assert!(!stdout.contains("say hello"));
}

#[test]
fn test_fixed_strings() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "a.b\naxb\na*b\n").unwrap();

    let (code, stdout, _) = run_rg(&["-F", "-s", "a.b", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("a.b"));
    assert!(!stdout.contains("axb"));
}

#[test]
fn test_invert_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "include1\nskip this\ninclude2\n").unwrap();

    let (code, stdout, _) = run_rg(&["-v", "-s", "skip", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("include1"));
    assert!(stdout.contains("include2"));
    assert!(!stdout.contains("skip this"));
}

// ---------------------------------------------------------------------------
// Multiple Patterns (-e, -f)
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_e_flags() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "dog\ncat\nbird\nfish\n").unwrap();

    let (code, stdout, _) = run_rg(&["-e", "dog", "-e", "fish", "-s", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("dog"));
    assert!(stdout.contains("fish"));
    assert!(!stdout.contains("cat"));
    assert!(!stdout.contains("bird"));
}

#[test]
fn test_patterns_from_file() {
    let dir = TempDir::new().unwrap();
    let pfile = dir.path().join("patterns.txt");
    fs::write(&pfile, "apple\ncherry\n").unwrap();

    let file = dir.path().join("test.txt");
    fs::write(&file, "apple\nbanana\ncherry\n").unwrap();

    let (code, stdout, _) = run_rg(&["-f", pfile.to_str().unwrap(), "-s", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("apple"));
    assert!(stdout.contains("cherry"));
    assert!(!stdout.contains("banana"));
}

// ---------------------------------------------------------------------------
// Output Modes (-c, -l, --files-without-match, -o, -q)
// ---------------------------------------------------------------------------

#[test]
fn test_count_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "match 1\nother\nmatch 2\nmatch 3\n").unwrap();

    let (code, stdout, _) = run_rg(&["-c", "-s", "match", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("3"));
}

#[test]
fn test_files_with_matches() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("has_match.txt");
    let f2 = dir.path().join("no_match.txt");
    fs::write(&f1, "target here\n").unwrap();
    fs::write(&f2, "other content\n").unwrap();

    let (code, stdout, _) = run_rg(&["-l", "-s", "target"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("has_match.txt"));
    assert!(!stdout.contains("no_match.txt"));
}

#[test]
fn test_files_without_match() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("has_match.txt");
    let f2 = dir.path().join("no_match.txt");
    fs::write(&f1, "target here\n").unwrap();
    fs::write(&f2, "other content\n").unwrap();

    let (code, stdout, _) = run_rg(&["--files-without-match", "-s", "target"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(!stdout.contains("has_match.txt"));
    assert!(stdout.contains("no_match.txt"));
}

#[test]
fn test_quiet_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "secret match\n").unwrap();

    let (code, stdout, _) = run_rg(&["-q", "secret", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());

    let (code, stdout, _) = run_rg(&["-q", "nomatch", file.to_str().unwrap()], None);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
}

// ---------------------------------------------------------------------------
// Context Lines and Separator Rules
// ---------------------------------------------------------------------------

#[test]
fn test_context_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nMATCH\nline4\nline5\n").unwrap();

    let (code, stdout, _) = run_rg(&["-C", "1", "-s", "MATCH", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("line2"));
    assert!(stdout.contains("MATCH"));
    assert!(stdout.contains("line4"));
    assert!(!stdout.contains("line1"));
    assert!(!stdout.contains("line5"));
}

#[test]
fn test_no_context_separator_when_no_context_requested() {
    // Critical prompt requirement: context separators (-- ) MUST NOT be emitted when context = 0.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "match1\nskip1\nskip2\nmatch2\n").unwrap();

    let (code, stdout, _) = run_rg(&["-s", "match", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("match1"));
    assert!(stdout.contains("match2"));
    assert!(!stdout.contains("--\n"), "Context separator was printed when context=0: {}", stdout);
}

#[test]
fn test_context_separator_emitted_when_context_requested() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "MATCH1\na\nb\nc\nd\ne\nMATCH2\n").unwrap();

    let (code, stdout, _) = run_rg(&["-C", "1", "-s", "MATCH", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("--\n") || stdout.contains("--\r\n"), "Expected context separator between disjoint groups");
}

// ---------------------------------------------------------------------------
// Ignore files (.gitignore, .ignore, .rgignore)
// ---------------------------------------------------------------------------

#[test]
fn test_gitignore_respected_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "hello from ignored\n").unwrap();
    fs::write(dir.path().join("tracked.txt"), "hello from tracked\n").unwrap();

    let (code, stdout, _) = run_rg(&["hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("tracked.txt"));
    assert!(!stdout.contains("ignored.txt"));
}

#[test]
fn test_no_ignore_flag() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "hello from ignored\n").unwrap();
    fs::write(dir.path().join("tracked.txt"), "hello from tracked\n").unwrap();

    let (code, stdout, _) = run_rg(&["--no-ignore", "hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("tracked.txt"));
    assert!(stdout.contains("ignored.txt"));
}

#[test]
fn test_rgignore_overrides_gitignore() {
    // A rule in .rgignore should take precedence over .gitignore
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".gitignore"), "target.txt\n").unwrap();
    // Un-ignore in .rgignore
    fs::write(dir.path().join(".rgignore"), "!target.txt\n").unwrap();
    fs::write(dir.path().join("target.txt"), "findme in target\n").unwrap();

    let (code, stdout, _) = run_rg(&["findme"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("target.txt"));
}

#[test]
fn test_hidden_files_skipped_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".hidden.txt"), "hello in hidden\n").unwrap();
    fs::write(dir.path().join("visible.txt"), "hello in visible\n").unwrap();

    let (code, stdout, _) = run_rg(&["hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("visible.txt"));
    assert!(!stdout.contains(".hidden.txt"));

    // With --hidden
    let (code, stdout, _) = run_rg(&["--hidden", "hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("visible.txt"));
    assert!(stdout.contains(".hidden.txt"));
}

// ---------------------------------------------------------------------------
// Glob Overrides (-g, --iglob)
// ---------------------------------------------------------------------------

#[test]
fn test_glob_include() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.rs"), "fn hello() {}\n").unwrap();
    fs::write(dir.path().join("foo.py"), "def hello(): pass\n").unwrap();

    let (code, stdout, _) = run_rg(&["-g", "*.rs", "hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("foo.rs"));
    assert!(!stdout.contains("foo.py"));
}

#[test]
fn test_glob_exclude() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.rs"), "fn hello() {}\n").unwrap();
    fs::write(dir.path().join("foo.py"), "def hello(): pass\n").unwrap();

    let (code, stdout, _) = run_rg(&["-g", "!*.py", "hello"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("foo.rs"));
    assert!(!stdout.contains("foo.py"));
}

// ---------------------------------------------------------------------------
// File Types (-t, -T, --type-list)
// ---------------------------------------------------------------------------

#[test]
fn test_type_filtering() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "findme in rust\n").unwrap();
    fs::write(dir.path().join("code.py"), "findme in python\n").unwrap();
    fs::write(dir.path().join("code.js"), "findme in javascript\n").unwrap();

    let (code, stdout, _) = run_rg(&["-t", "rust", "findme"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("code.rs"));
    assert!(!stdout.contains("code.py"));
    assert!(!stdout.contains("code.js"));
}

#[test]
fn test_type_not_filtering() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "findme in rust\n").unwrap();
    fs::write(dir.path().join("code.py"), "findme in python\n").unwrap();

    let (code, stdout, _) = run_rg(&["-T", "py", "findme"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("code.rs"));
    assert!(!stdout.contains("code.py"));
}

#[test]
fn test_type_list() {
    let (code, stdout, _) = run_rg(&["--type-list"], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("rust:"));
    assert!(stdout.contains("py:"));
    assert!(stdout.contains("c:"));
    assert!(stdout.contains("cpp:"));
    assert!(stdout.contains("json:"));
}

// ---------------------------------------------------------------------------
// Stdin Searching
// ---------------------------------------------------------------------------

#[test]
fn test_search_stdin() {
    let input = "first line\nneedle in haystack\nlast line\n";
    let (code, stdout, _) = run_rg_stdin(&["needle"], input);
    assert_eq!(code, 0);
    assert!(stdout.contains("needle in haystack"));
    assert!(!stdout.contains("first line"));
}

#[test]
fn test_search_stdin_no_match() {
    let input = "first line\nsecond line\n";
    let (code, stdout, _) = run_rg_stdin(&["needle"], input);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
}

// ---------------------------------------------------------------------------
// Special Modes (--version, --help, --files)
// ---------------------------------------------------------------------------

#[test]
fn test_version() {
    let (code, stdout, _) = run_rg(&["--version"], None);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("ripgrep 14.1.1") || stdout.contains("14.1.1"));
}

#[test]
fn test_help() {
    let (code, stdout, _) = run_rg(&["--help"], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage:"));
}

#[test]
fn test_list_files_mode() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "").unwrap();
    fs::write(dir.path().join("b.py"), "").unwrap();
    fs::write(dir.path().join(".ignored.txt"), "").unwrap();

    let (code, stdout, _) = run_rg(&["--files"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("b.py"));
    assert!(!stdout.contains(".ignored.txt"));
}

// ---------------------------------------------------------------------------
// Multiline Search (-U)
// ---------------------------------------------------------------------------

#[test]
fn test_multiline_search() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "first\nstart\nmiddle\nend\nlast\n").unwrap();

    let (code, stdout, _) = run_rg(&["-U", r"start\nmiddle\nend", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("start"));
}

// ---------------------------------------------------------------------------
// Max Count (-m)
// ---------------------------------------------------------------------------

#[test]
fn test_max_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "match 1\nmatch 2\nmatch 3\nmatch 4\n").unwrap();

    let (code, stdout, _) = run_rg(&["-m", "2", "-s", "match", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("match 1"));
    assert!(stdout.contains("match 2"));
    assert!(!stdout.contains("match 3"));
    assert!(!stdout.contains("match 4"));
}

// ---------------------------------------------------------------------------
// JSON Output (--json)
// ---------------------------------------------------------------------------

#[test]
fn test_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\ngoodbye\n").unwrap();

    let (code, stdout, _) = run_rg(&["--json", "hello", file.to_str().unwrap()], None);
    assert_eq!(code, 0);

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(lines.len() >= 2);

    // Each line should be valid JSON
    for line in &lines {
        let val: serde_json::Value = serde_json::from_str(line).expect("must be valid JSON");
        assert!(val.get("type").is_some());
    }
}

// ---------------------------------------------------------------------------
// Stats Output (--stats)
// ---------------------------------------------------------------------------

#[test]
fn test_stats_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "match one\nmatch two\n").unwrap();

    let (code, stdout, _) = run_rg(&["--stats", "-s", "match", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("matches"));
    assert!(stdout.contains("matched lines"));
    assert!(stdout.contains("files searched"));
}

// ---------------------------------------------------------------------------
// Binary File Detection (-a / --text)
// ---------------------------------------------------------------------------

#[test]
fn test_binary_detection_default() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    let data = b"hello\x00world\n".to_vec();
    fs::write(&file, &data).unwrap();

    let (code, stdout, stderr) = run_rg(&["world", file.to_str().unwrap()], None);
    // In ripgrep, matching binary file prints message to stderr/stdout and returns exit code 0
    assert_eq!(code, 0);
    assert!(stderr.contains("Binary file") || stdout.contains("Binary file"));
}

#[test]
fn test_binary_text_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    let data = b"hello\x00world\n".to_vec();
    fs::write(&file, &data).unwrap();

    let (code, stdout, _) = run_rg(&["-a", "world", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("world"));
}

// ---------------------------------------------------------------------------
// Column and Byte Offset (-b, --column)
// ---------------------------------------------------------------------------

#[test]
fn test_byte_offset() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "abcdef\n123456\n").unwrap();

    let (code, stdout, _) = run_rg(&["-b", "-s", "123", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    // Byte offset of second line is 7
    assert!(stdout.contains("7:") || stdout.contains(":7:"));
}

#[test]
fn test_column_number() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar baz\n").unwrap();

    let (code, stdout, _) = run_rg(&["--column", "-s", "bar", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    // "bar" starts at 1-based column 5
    assert!(stdout.contains(":5:") || stdout.contains("1:5:"));
}

// ---------------------------------------------------------------------------
// Max Columns (-M)
// ---------------------------------------------------------------------------

#[test]
fn test_max_columns() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    let long_line = "a".repeat(200) + " match\n";
    fs::write(&file, &long_line).unwrap();

    let (code, stdout, _) = run_rg(&["-M", "50", "-s", "match", file.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("omitted") || stdout.lines().all(|l| l.len() <= 100));
}

// ---------------------------------------------------------------------------
// Directory Depth (--max-depth)
// ---------------------------------------------------------------------------

#[test]
fn test_max_depth() {
    let dir = TempDir::new().unwrap();
    let sub1 = dir.path().join("level1");
    let sub2 = sub1.join("level2");
    fs::create_dir_all(&sub2).unwrap();

    fs::write(dir.path().join("root.txt"), "depth 0 target\n").unwrap();
    fs::write(sub1.join("sub1.txt"), "depth 1 target\n").unwrap();
    fs::write(sub2.join("sub2.txt"), "depth 2 target\n").unwrap();

    let (code, stdout, _) = run_rg(&["--max-depth", "1", "target"], Some(dir.path()));
    assert_eq!(code, 0);
    assert!(stdout.contains("root.txt"));
    assert!(!stdout.contains("sub2.txt"));
}

// ---------------------------------------------------------------------------
// Invalid Regex Exit Code (exit code 2)
// ---------------------------------------------------------------------------

#[test]
fn test_invalid_regex_exit_code_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "test\n").unwrap();

    let (code, _, stderr) = run_rg(&["[unclosed", file.to_str().unwrap()], None);
    assert_eq!(code, 2, "Expected exit code 2 on regex compilation error");
    assert!(!stderr.is_empty());
}

