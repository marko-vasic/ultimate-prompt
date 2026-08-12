/*!
CLI utility functions for grep-like programs.

This crate provides a collection of utilities commonly needed when building
command-line search tools like ripgrep. It includes:

- **Terminal detection**: Determine if stdin/stdout/stderr are connected to a
  terminal ([`is_tty_stdout`], [`is_tty_stderr`], [`is_tty_stdin`]).
- **Output buffering**: Create stdout writers with different buffering
  strategies ([`stdout`], [`stdout_buffered_line`], [`stdout_buffered_block`]).
- **Size parsing**: Parse human-readable size strings like `1K`, `10MB`
  ([`parse_human_readable_size`]).
- **Hostname detection**: Get the machine's hostname ([`hostname`]).
- **Escape utilities**: Escape and unescape byte strings with `\xNN` notation
  ([`escape`], [`unescape`], [`escape_os`]).
- **Pattern reading**: Read search patterns from files, stdin, or any reader
  ([`patterns_from_reader`], [`patterns_from_path`], [`patterns_from_stdin`]).
- **Decompression**: Transparently decompress files using external tools
  ([`DecompressionReader`]).
- **Process reading**: Read from the stdout of spawned child processes
  ([`CommandReader`]).
*/

mod decompress;
mod escape;
mod hostname;
mod human;
mod pattern;
mod process;
mod tty;
mod wtr;

pub use crate::decompress::DecompressionReader;
pub use crate::escape::{escape, escape_os, unescape};
pub use crate::hostname::hostname;
pub use crate::human::parse_human_readable_size;
pub use crate::pattern::{patterns_from_path, patterns_from_reader, patterns_from_stdin};
pub use crate::process::CommandReader;
pub use crate::tty::{is_tty_stderr, is_tty_stdin, is_tty_stdout};
pub use crate::wtr::{stdout, stdout_buffered_block, stdout_buffered_line};
