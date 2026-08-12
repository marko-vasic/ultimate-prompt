/// Output buffering utilities for writing colored output to stdout.
///
/// These functions provide different buffering strategies for writing
/// to stdout with optional color support via the `termcolor` crate.

use termcolor::{BufferWriter, BufferedStandardStream, ColorChoice, StandardStream};

/// Returns a `StandardStream` for stdout with the given color choice.
///
/// This uses line buffering by default, which flushes after every newline.
/// This is appropriate for interactive use where output should appear
/// promptly.
pub fn stdout(color_choice: ColorChoice) -> StandardStream {
    StandardStream::stdout(color_choice)
}

/// Returns a `BufferedStandardStream` for stdout with the given color choice.
///
/// This uses line buffering, which flushes the buffer after every newline.
/// This provides a balance between performance and responsiveness.
pub fn stdout_buffered_line(color_choice: ColorChoice) -> BufferedStandardStream {
    BufferedStandardStream::stdout(color_choice)
}

/// Returns a `BufferWriter` for stdout with the given color choice.
///
/// This uses block buffering, where output is accumulated in a buffer and
/// written all at once. This is appropriate for non-interactive use where
/// throughput is more important than latency.
pub fn stdout_buffered_block(color_choice: ColorChoice) -> BufferWriter {
    BufferWriter::stdout(color_choice)
}
