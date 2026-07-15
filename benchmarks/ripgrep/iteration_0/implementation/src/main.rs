//! `rg` — a ripgrep implementation.
//!
//! This is the main entry point for the `rg` binary. It parses command-line
//! arguments, configures the logger, sets up the search pipeline, and runs
//! the search.
//!
//! # Exit Codes
//!
//! - `0` — At least one match was found.
//! - `1` — No match was found.
//! - `2` — An error occurred.

mod args;
mod logger;
mod search;

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match try_main() {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            // Ignore BrokenPipe errors — they're expected when piping
            // output to `head`, `less`, etc.
            if is_broken_pipe(&err) {
                return ExitCode::from(0);
            }
            // Attempt to write the error message. If that also fails with
            // broken pipe, silently exit.
            if let Err(io_err) = writeln!(io::stderr(), "rg: {}", err) {
                if io_err.kind() == io::ErrorKind::BrokenPipe {
                    return ExitCode::from(2);
                }
            }
            ExitCode::from(2)
        }
    }
}

fn try_main() -> Result<bool, Box<dyn std::error::Error>> {
    let parsed_args = args::parse().map_err(|e| -> Box<dyn std::error::Error> {
        Box::from(e)
    })?;

    // Configure the logger early so debug/trace flags take effect.
    logger::configure(parsed_args.debug, parsed_args.trace);

    log::debug!("parsed args: {:?}", parsed_args);

    // Handle --help.
    if parsed_args.help {
        args::print_help();
        return Ok(true);
    }

    // Handle --version.
    if parsed_args.version {
        args::print_version();
        return Ok(true);
    }

    // Handle --type-list (no pattern required).
    if parsed_args.type_list {
        return search::run(&parsed_args).map_err(|e| e);
    }

    // Handle --files (no pattern required).
    if parsed_args.files_mode {
        return search::run(&parsed_args).map_err(|e| e);
    }

    // Ensure we have at least one pattern.
    if parsed_args.patterns.is_empty() {
        return Err("no pattern given. Use 'rg -h' for help.".into());
    }

    // Run the search pipeline.
    search::run(&parsed_args)
}

/// Check if an error is a broken pipe error.
fn is_broken_pipe(err: &Box<dyn std::error::Error>) -> bool {
    // Try to downcast to io::Error.
    if let Some(io_err) = err.downcast_ref::<io::Error>() {
        return io_err.kind() == io::ErrorKind::BrokenPipe;
    }
    // Check if the Display representation mentions "Broken pipe".
    let msg = err.to_string();
    msg.contains("Broken pipe") || msg.contains("broken pipe")
}
