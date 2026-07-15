//! Search pipeline orchestration for `rg`.
//!
//! This module wires together the matcher, searcher, printer, and walker
//! to perform the actual search. It handles the various modes of operation:
//! stdin search, file listing, single-threaded search, and multi-threaded
//! search.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use ignore::{WalkState, ParallelVisitor, ParallelVisitorBuilder};
use termcolor::BufferWriter;

use crate::args::{Args, OutputMode};

/// Run the search and return `true` if at least one match was found.
pub fn run(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    // Handle --type-list mode.
    if args.type_list {
        return run_type_list(args);
    }

    // Handle --files mode (just list files).
    if args.files_mode {
        return run_files_mode(args);
    }

    // Handle stdin search.
    if args.is_stdin_search() {
        return search_stdin(args);
    }

    // If sorting is enabled, always use single-threaded search.
    if args.sort_mode != crate::args::SortMode::None {
        return search_single_threaded(args);
    }

    // Choose single vs multi-threaded.
    if args.threads <= 1 {
        search_single_threaded(args)
    } else {
        search_multi_threaded(args)
    }
}

/// List all known file types and exit.
fn run_type_list(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    use ignore::TypesBuilder;

    let mut builder = TypesBuilder::new();
    builder.add_defaults();

    // Also add any user-specified types.
    for (name, glob) in &args.type_add {
        let _ = builder.add(name, glob);
    }

    // Collect type definitions.
    // We build the types and then print the definitions.
    // Since TypesBuilder doesn't expose definitions directly, we'll print
    // the known types from the defaults.
    let defaults: &[(&str, &[&str])] = &[
        ("bash", &["*.bash"]),
        ("c", &["*.c", "*.h"]),
        ("clojure", &["*.clj", "*.cljs"]),
        ("cmake", &["CMakeLists.txt", "*.cmake"]),
        ("cpp", &["*.cpp", "*.hpp", "*.cc", "*.hh", "*.cxx"]),
        ("css", &["*.css"]),
        ("csv", &["*.csv"]),
        ("dart", &["*.dart"]),
        ("docker", &["Dockerfile", "*.dockerfile"]),
        ("elisp", &["*.el"]),
        ("erlang", &["*.erl"]),
        ("go", &["*.go"]),
        ("haskell", &["*.hs"]),
        ("html", &["*.html", "*.htm"]),
        ("java", &["*.java"]),
        ("js", &["*.js"]),
        ("javascript", &["*.js"]),
        ("json", &["*.json"]),
        ("kotlin", &["*.kt"]),
        ("kt", &["*.kt"]),
        ("lua", &["*.lua"]),
        ("make", &["Makefile", "makefile", "*.mk"]),
        ("markdown", &["*.md"]),
        ("md", &["*.md"]),
        ("ocaml", &["*.ml", "*.mli"]),
        ("perl", &["*.pl", "*.pm"]),
        ("php", &["*.php"]),
        ("proto", &["*.proto"]),
        ("py", &["*.py"]),
        ("python", &["*.py"]),
        ("r", &["*.r", "*.R"]),
        ("rb", &["*.rb"]),
        ("ruby", &["*.rb"]),
        ("rust", &["*.rs"]),
        ("scala", &["*.scala"]),
        ("sh", &["*.sh"]),
        ("sql", &["*.sql"]),
        ("swift", &["*.swift"]),
        ("tex", &["*.tex"]),
        ("toml", &["*.toml"]),
        ("ts", &["*.ts"]),
        ("txt", &["*.txt"]),
        ("typescript", &["*.ts"]),
        ("vim", &["*.vim"]),
        ("xml", &["*.xml"]),
        ("yaml", &["*.yaml", "*.yml"]),
        ("zig", &["*.zig"]),
    ];

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for (name, globs) in defaults {
        write!(out, "{}: ", name)?;
        for (i, g) in globs.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "{}", g)?;
        }
        writeln!(out)?;
    }

    // Print user-added types.
    for (name, glob) in &args.type_add {
        writeln!(out, "{}: {}", name, glob)?;
    }

    Ok(false) // no matches in type-list mode
}

/// List files that would be searched.
fn run_files_mode(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    let builder = args.walk_builder()?;
    let walker = builder.build();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut found = false;

    for result in walker {
        match result {
            Ok(entry) => {
                if entry.is_dir() {
                    continue;
                }
                found = true;
                let path = entry.path();
                out.write_all(path.to_string_lossy().as_bytes())?;
                if args.null {
                    out.write_all(b"\x00")?;
                } else {
                    out.write_all(b"\n")?;
                }
            }
            Err(err) => {
                if !args.no_messages {
                    eprintln!("rg: {}", err);
                }
            }
        }
    }

    Ok(found)
}

/// Search standard input.
fn search_stdin(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    let matcher = args.regex_matcher()?;
    let mut searcher = args.searcher();
    let stdin = io::stdin();

    match args.output_mode {
        OutputMode::Standard => {
            let color = args.color_choice();
            let stdout = grep_cli::stdout(color);
            let mut printer = args.printer_standard(stdout);
            let mut sink = printer.sink(&matcher);
            searcher.search_reader(&matcher, stdin.lock(), &mut sink)
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            let count = sink.match_count();
            Ok(count > 0)
        }
        OutputMode::Count | OutputMode::CountMatches
        | OutputMode::FilesWithMatches | OutputMode::FilesWithoutMatch
        | OutputMode::Quiet => {
            let color = args.color_choice();
            let stdout = grep_cli::stdout(color);
            let mut printer = args.printer_summary(stdout);
            let mut sink = printer.sink(&matcher);
            searcher.search_reader(&matcher, stdin.lock(), &mut sink)
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            let count = sink.match_count();
            Ok(count > 0)
        }
        OutputMode::Json => {
            let stdout = io::stdout();
            let mut printer = args.printer_json(stdout.lock());
            let mut sink = printer.sink(&matcher);
            searcher.search_reader(&matcher, stdin.lock(), &mut sink)
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            let count = sink.match_count();
            Ok(count > 0)
        }
    }
}

/// Single-threaded search over all files.
fn search_single_threaded(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    let matcher = args.regex_matcher()?;
    let mut searcher = args.searcher();
    let builder = args.walk_builder()?;
    let walker = builder.build();

    let color = args.color_choice();
    let mut matched = false;
    let mut searched_any = false;

    match args.output_mode {
        OutputMode::Standard => {
            let stdout = grep_cli::stdout(color);
            let mut printer = args.printer_standard(stdout);

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(err) => {
                        if !args.no_messages {
                            eprintln!("rg: {}", err);
                        }
                        continue;
                    }
                };
                if entry.is_dir() {
                    continue;
                }
                searched_any = true;
                let path = entry.path();

                let mut sink = if args.use_filename() {
                    printer.sink_with_path(&matcher, path)
                } else {
                    printer.sink(&matcher)
                };

                let search_result = searcher.search_path(&matcher, path, &mut sink);
                if let Err(err) = search_result {
                    if !args.no_messages {
                        eprintln!("rg: {}: {}", path.display(), err);
                    }
                    continue;
                }

                if sink.match_count() > 0 {
                    matched = true;
                    if args.output_mode == OutputMode::Quiet {
                        return Ok(true);
                    }
                }

                // Flush after each file if using max_count
                if let Some(max) = args.max_count {
                    if sink.match_count() >= max {
                        // Already stopped by the searcher/sink
                    }
                }
            }
        }
        OutputMode::Count | OutputMode::CountMatches
        | OutputMode::FilesWithMatches | OutputMode::FilesWithoutMatch
        | OutputMode::Quiet => {
            let stdout = grep_cli::stdout(color);
            let mut printer = args.printer_summary(stdout);

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(err) => {
                        if !args.no_messages {
                            eprintln!("rg: {}", err);
                        }
                        continue;
                    }
                };
                if entry.is_dir() {
                    continue;
                }
                searched_any = true;
                let path = entry.path();

                let mut sink = if args.use_filename() {
                    printer.sink_with_path(&matcher, path)
                } else {
                    printer.sink(&matcher)
                };

                let search_result = searcher.search_path(&matcher, path, &mut sink);
                if let Err(err) = search_result {
                    if !args.no_messages {
                        eprintln!("rg: {}: {}", path.display(), err);
                    }
                    continue;
                }

                if sink.match_count() > 0 {
                    matched = true;
                    if args.output_mode == OutputMode::Quiet {
                        return Ok(true);
                    }
                }
            }
        }
        OutputMode::Json => {
            let stdout = io::stdout();
            let mut printer = args.printer_json(stdout.lock());

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(err) => {
                        if !args.no_messages {
                            eprintln!("rg: {}", err);
                        }
                        continue;
                    }
                };
                if entry.is_dir() {
                    continue;
                }
                searched_any = true;
                let path = entry.path();

                let mut sink = printer.sink_with_path(&matcher, path);
                let search_result = searcher.search_path(&matcher, path, &mut sink);
                if let Err(err) = search_result {
                    if !args.no_messages {
                        eprintln!("rg: {}: {}", path.display(), err);
                    }
                    continue;
                }

                if sink.match_count() > 0 {
                    matched = true;
                }
            }
        }
    }

    if !searched_any && args.paths.is_empty() {
        log::debug!(
            "No files were searched. Perhaps your current directory has \
             no files or all files are filtered. Use --debug for more info."
        );
    }

    Ok(matched)
}

/// Multi-threaded search over all files using WalkParallel.
fn search_multi_threaded(args: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    let matcher = args.regex_matcher()?;
    let walk_builder = args.walk_builder()?;
    let walker = walk_builder.build_parallel();

    let color = args.color_choice();
    let buffer_writer = BufferWriter::stdout(color);
    let matched = AtomicBool::new(false);
    let no_messages = args.no_messages;

    let factory = SearchVisitorBuilder {
        args,
        matcher: &matcher,
        buffer_writer: &buffer_writer,
        matched: &matched,
        no_messages,
    };

    walker.run(&factory);

    Ok(matched.load(Ordering::SeqCst))
}

/// Builder for parallel search visitors.
struct SearchVisitorBuilder<'a> {
    args: &'a Args,
    matcher: &'a RegexMatcher,
    buffer_writer: &'a BufferWriter,
    matched: &'a AtomicBool,
    no_messages: bool,
}

/// A per-thread search worker.
struct SearchVisitor<'a> {
    args: &'a Args,
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    buffer_writer: &'a BufferWriter,
    matched: &'a AtomicBool,
    no_messages: bool,
}

impl<'a> ParallelVisitorBuilder<'a> for SearchVisitorBuilder<'a> {
    type Visitor = SearchVisitor<'a>;

    fn build(&'a self) -> SearchVisitor<'a> {
        SearchVisitor {
            args: self.args,
            matcher: self.matcher,
            searcher: self.args.searcher(),
            buffer_writer: self.buffer_writer,
            matched: self.matched,
            no_messages: self.no_messages,
        }
    }
}

impl<'a> ParallelVisitor for SearchVisitor<'a> {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                if !self.no_messages {
                    eprintln!("rg: {}", err);
                }
                return WalkState::Continue;
            }
        };

        if entry.is_dir() {
            return WalkState::Continue;
        }

        let path = entry.path();

        match self.args.output_mode {
            OutputMode::Standard => {
                let mut buf = self.buffer_writer.buffer();
                {
                    let mut printer = self.args.printer_standard(&mut buf);
                    let mut sink = if self.args.use_filename() {
                        printer.sink_with_path(self.matcher, path)
                    } else {
                        printer.sink(self.matcher)
                    };

                    let result = self.searcher.search_path(
                        self.matcher,
                        path,
                        &mut sink,
                    );
                    if let Err(err) = result {
                        if !self.no_messages {
                            eprintln!("rg: {}: {}", path.display(), err);
                        }
                        return WalkState::Continue;
                    }

                    if sink.match_count() > 0 {
                        self.matched.store(true, Ordering::SeqCst);
                    }
                }
                // Print the buffer atomically.
                if let Err(err) = self.buffer_writer.print(&buf) {
                    if err.kind() == io::ErrorKind::BrokenPipe {
                        return WalkState::Quit;
                    }
                    if !self.no_messages {
                        eprintln!("rg: write error: {}", err);
                    }
                }
            }
            OutputMode::Count | OutputMode::CountMatches
            | OutputMode::FilesWithMatches | OutputMode::FilesWithoutMatch
            | OutputMode::Quiet => {
                let mut buf = self.buffer_writer.buffer();
                {
                    let mut printer = self.args.printer_summary(&mut buf);
                    let mut sink = if self.args.use_filename() {
                        printer.sink_with_path(self.matcher, path)
                    } else {
                        printer.sink(self.matcher)
                    };

                    let result = self.searcher.search_path(
                        self.matcher,
                        path,
                        &mut sink,
                    );
                    if let Err(err) = result {
                        if !self.no_messages {
                            eprintln!("rg: {}: {}", path.display(), err);
                        }
                        return WalkState::Continue;
                    }

                    if sink.match_count() > 0 {
                        self.matched.store(true, Ordering::SeqCst);
                        if self.args.output_mode == OutputMode::Quiet {
                            return WalkState::Quit;
                        }
                    }
                }
                if let Err(err) = self.buffer_writer.print(&buf) {
                    if err.kind() == io::ErrorKind::BrokenPipe {
                        return WalkState::Quit;
                    }
                }
            }
            OutputMode::Json => {
                // JSON mode with parallel output: each file's JSON goes
                // to a buffer, then printed atomically.
                let mut buf = Vec::new();
                {
                    let mut printer = self.args.printer_json(&mut buf);
                    let mut sink = printer.sink_with_path(self.matcher, path);

                    let result = self.searcher.search_path(
                        self.matcher,
                        path,
                        &mut sink,
                    );
                    if let Err(err) = result {
                        if !self.no_messages {
                            eprintln!("rg: {}: {}", path.display(), err);
                        }
                        return WalkState::Continue;
                    }

                    if sink.match_count() > 0 {
                        self.matched.store(true, Ordering::SeqCst);
                    }
                }
                if !buf.is_empty() {
                    let stdout = io::stdout();
                    let mut out = stdout.lock();
                    if let Err(err) = out.write_all(&buf) {
                        if err.kind() == io::ErrorKind::BrokenPipe {
                            return WalkState::Quit;
                        }
                    }
                }
            }
        }

        WalkState::Continue
    }
}
