use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use grep_printer::{ColorSpecs, HyperlinkFormat, StandardBuilder, SummaryBuilder, SummaryKind, JSONBuilder};
use grep_searcher::Searcher;
use ignore::walk::{DirEntry, WalkState};

use crate::args::{Args, Mode};

pub fn run_search(args: &Args) -> Result<()> {
    let matched = if args.threads.unwrap_or(0) == 1 || args.sort.is_some() || args.sort_reverse.is_some() {
        search_single_threaded(args)?
    } else {
        search_multi_threaded(args)?
    };

    if matched {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

pub fn run_files(args: &Args) -> Result<()> {
    let walker = args.walker();
    let stdout = io::stdout();
    let mut handle = BufWriter::new(stdout.lock());

    for result in walker {
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                if !args.no_messages {
                    eprintln!("{}", err);
                }
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }

        let path = entry.path();
        if args.null_path {
            write!(handle, "{}\0", path.display())?;
        } else {
            writeln!(handle, "{}", path.display())?;
        }
    }

    Ok(())
}

fn search_single_threaded(args: &Args) -> Result<bool> {
    let matcher = args.matcher()?;
    let mut searcher = args.searcher();
    let mut matched = false;

    let stdout = io::stdout();
    let color_choice = args.color.to_termcolor();
    let color_specs = ColorSpecs::from_specs(&args.color_specs).unwrap_or_default();

    let mut std_printer = if args.json {
        None
    } else if args.count || args.count_matches || args.files_with_matches || args.files_without_match || args.quiet {
        None
    } else {
        let mut builder = StandardBuilder::new();
        builder.color_specs(color_specs.clone());
        if let Some(h) = args.heading {
            builder.heading(h);
        }
        if let Some(n) = args.line_number {
            builder.line_number(n);
        }
        if args.column {
            builder.column(true);
        }
        if args.byte_offset {
            builder.byte_offset(true);
        }
        if args.only_matching {
            builder.only_matching(true);
        }
        if let Some(ref rep) = args.replacement {
            builder.replacement(rep.clone());
        }
        if let Some(mc) = args.max_columns {
            builder.max_columns(Some(mc));
        }
        if args.max_columns_preview {
            builder.max_columns_preview(true);
        }
        if args.trim {
            builder.trim_ascii(true);
        }
        if args.null_path {
            builder.null_path(true);
        }
        if let Some(ref hl) = args.hyperlink_format {
            if let Ok(fmt) = HyperlinkFormat::from_str(hl) {
                builder.hyperlink(Some(fmt));
            }
        }

        Some(builder.build(termcolor::StandardStream::stdout(color_choice)))
    };

    let mut json_printer = if args.json {
        Some(JSONBuilder::new().build(termcolor::StandardStream::stdout(color_choice)))
    } else {
        None
    };

    let mut summary_printer = if args.count
        || args.count_matches
        || args.files_with_matches
        || args.files_without_match
        || args.quiet
    {
        let kind = if args.quiet {
            SummaryKind::Quiet
        } else if args.count_matches {
            SummaryKind::CountMatches
        } else if args.count {
            SummaryKind::Count
        } else if args.files_with_matches {
            SummaryKind::PathWithMatch
        } else {
            SummaryKind::PathWithoutMatch
        };
        Some(SummaryBuilder::new().kind(kind).build(termcolor::StandardStream::stdout(color_choice)))
    } else {
        None
    };

    let walker = args.walker();
    for result in walker {
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                if !args.no_messages {
                    eprintln!("{}", err);
                }
                continue;
            }
        };

        if entry.is_dir() {
            continue;
        }

        let path = entry.path();

        let file_matched = if let Some(ref mut printer) = std_printer {
            let mut sink = printer.sink_with_path(&matcher, path);
            let res = if entry.is_stdin() {
                searcher.search_reader(&matcher, io::stdin(), &mut sink)
            } else if let Ok(file) = File::open(path) {
                searcher.search_file(&matcher, &file, &mut sink)
            } else {
                continue;
            };
            res.is_ok() && printer.has_written()
        } else if let Some(ref mut printer) = json_printer {
            let mut sink = printer.sink_with_path(&matcher, path);
            let res = if entry.is_stdin() {
                searcher.search_reader(&matcher, io::stdin(), &mut sink)
            } else if let Ok(file) = File::open(path) {
                searcher.search_file(&matcher, &file, &mut sink)
            } else {
                continue;
            };
            res.is_ok()
        } else if let Some(ref mut printer) = summary_printer {
            let mut sink = printer.sink_with_path(&matcher, path);
            let _ = if entry.is_stdin() {
                searcher.search_reader(&matcher, io::stdin(), &mut sink)
            } else if let Ok(file) = File::open(path) {
                searcher.search_file(&matcher, &file, &mut sink)
            } else {
                Ok(())
            };
            printer.has_matches()
        } else {
            false
        };

        if file_matched {
            matched = true;
            if args.quiet {
                return Ok(true);
            }
        }
    }

    Ok(matched)
}

fn search_multi_threaded(args: &Args) -> Result<bool> {
    let matcher = args.matcher()?;
    let matched = Arc::new(AtomicBool::new(false));
    let color_specs = ColorSpecs::from_specs(&args.color_specs).unwrap_or_default();

    let json = args.json;
    let count = args.count;
    let count_matches = args.count_matches;
    let files_with_matches = args.files_with_matches;
    let files_without_match = args.files_without_match;
    let quiet = args.quiet;
    let heading = args.heading;
    let line_number = args.line_number;
    let column = args.column;
    let byte_offset = args.byte_offset;
    let only_matching = args.only_matching;
    let replacement = args.replacement.clone();
    let max_columns = args.max_columns;
    let max_columns_preview = args.max_columns_preview;
    let trim = args.trim;
    let null_path = args.null_path;

    let walker = args.walker_parallel();
    walker.run(|| {
        let matcher = matcher.clone();
        let matched = matched.clone();
        let mut searcher = args.searcher();
        let color_specs = color_specs.clone();
        let replacement = replacement.clone();

        Box::new(move |result: Result<DirEntry, ignore::Error>| {
            let entry = match result {
                Ok(entry) => entry,
                Err(_) => return WalkState::Continue,
            };

            if entry.is_dir() {
                return WalkState::Continue;
            }

            let path = entry.path();
            let mut buffer = termcolor::Buffer::no_color();

            let file_matched = if json {
                let mut printer = JSONBuilder::new().build(&mut buffer);
                let mut sink = printer.sink_with_path(&matcher, path);
                let res = if let Ok(file) = File::open(path) {
                    searcher.search_file(&matcher, &file, &mut sink)
                } else {
                    return WalkState::Continue;
                };
                res.is_ok()
            } else if count
                || count_matches
                || files_with_matches
                || files_without_match
                || quiet
            {
                let kind = if quiet {
                    SummaryKind::Quiet
                } else if count_matches {
                    SummaryKind::CountMatches
                } else if count {
                    SummaryKind::Count
                } else if files_with_matches {
                    SummaryKind::PathWithMatch
                } else {
                    SummaryKind::PathWithoutMatch
                };
                let mut printer = SummaryBuilder::new().kind(kind).build(&mut buffer);
                let mut sink = printer.sink_with_path(&matcher, path);
                let _ = if let Ok(file) = File::open(path) {
                    searcher.search_file(&matcher, &file, &mut sink)
                } else {
                    return WalkState::Continue;
                };
                printer.has_matches()
            } else {
                let mut builder = StandardBuilder::new();
                builder.color_specs(color_specs.clone());
                if let Some(h) = heading {
                    builder.heading(h);
                }
                if let Some(n) = line_number {
                    builder.line_number(n);
                }
                if column {
                    builder.column(true);
                }
                if byte_offset {
                    builder.byte_offset(true);
                }
                if only_matching {
                    builder.only_matching(true);
                }
                if let Some(ref rep) = replacement {
                    builder.replacement(rep.clone());
                }
                if let Some(mc) = max_columns {
                    builder.max_columns(Some(mc));
                }
                if max_columns_preview {
                    builder.max_columns_preview(true);
                }
                if trim {
                    builder.trim_ascii(true);
                }
                if null_path {
                    builder.null_path(true);
                }
                let mut printer = builder.build(&mut buffer);
                let mut sink = printer.sink_with_path(&matcher, path);
                let res = if let Ok(file) = File::open(path) {
                    searcher.search_file(&matcher, &file, &mut sink)
                } else {
                    return WalkState::Continue;
                };
                res.is_ok() && printer.has_written()
            };

            if file_matched {
                matched.store(true, Ordering::SeqCst);
            }

            if !buffer.as_slice().is_empty() {
                let mut stdout = io::stdout().lock();
                let _ = stdout.write_all(buffer.as_slice());
            }

            WalkState::Continue
        })
    });

    Ok(matched.load(Ordering::SeqCst))
}
