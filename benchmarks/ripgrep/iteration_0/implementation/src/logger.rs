//! Debug/trace logging configuration for rg.
//!
//! This module provides a simple function to configure the global logger
//! based on the `--debug` and `--trace` flags.

/// Configure the global logger.
///
/// # Levels
///
/// - `trace = true` → `LevelFilter::Trace`
/// - `debug = true` → `LevelFilter::Debug`
/// - Otherwise → `LevelFilter::Warn`
pub fn configure(debug: bool, trace: bool) {
    let level = if trace {
        log::LevelFilter::Trace
    } else if debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    };
    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp(None)
        .init();
}
