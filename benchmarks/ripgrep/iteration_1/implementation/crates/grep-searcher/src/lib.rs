/*!
Search orchestration for grep-like programs.

This crate provides the [`Searcher`] type, which drives line-oriented searches
over byte sources, and the [`Sink`] trait, which consumers implement to receive
search results.

Key types:

- [`Searcher`] — configures and runs searches.
- [`SearcherBuilder`] — builder for constructing a [`Searcher`].
- [`Sink`] — trait for receiving search events (matches, context, etc.).
- [`SinkMatch`] — data about a matching line.
- [`SinkContext`] — data about a context line.
- [`SinkContextKind`] — the kind of context (before, after, or other).
- [`SinkFinish`] — summary data sent after a search completes.
*/

pub mod line_buffer;
pub mod lines;
pub mod searcher;
pub mod sink;

// Re-export commonly used items from sub-modules.
pub use line_buffer::{LineBuffer, LineBufferReader};
pub use lines::{LineIter, locate, preceding, without_terminator};
pub use searcher::{
    BinaryDetection, Encoding, MmapChoice, Searcher, SearcherBuilder,
};
pub use sink::{Sink, SinkContext, SinkContextKind, SinkFinish, SinkMatch};
