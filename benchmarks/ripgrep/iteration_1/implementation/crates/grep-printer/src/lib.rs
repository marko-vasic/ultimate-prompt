/*!
Output formatting for grep-like programs.

This crate provides printers for displaying search results produced by
[`grep-searcher`]. Three printer types are available:

- [`Standard`] — traditional grep-style text output with colored match
  highlighting, line numbers, context, and file paths.
- [`Summary`] — aggregate output (count, list of matching/non-matching
  files, or quiet mode).
- [`JSON`] — machine-readable newline-delimited JSON output.

Each printer has an associated builder and sink type. The sink implements
[`grep_searcher::Sink`], so it can be passed directly to
[`Searcher::search_slice`](grep_searcher::Searcher::search_slice) and
similar search methods.

# Supporting types

- [`ColorSpecs`] — configure terminal colors for output elements.
- [`HyperlinkFormat`] — configure clickable hyperlinks for terminal
  emulators.
- [`Stats`] — aggregate statistics for search runs.

# Example

```rust,no_run
use grep_printer::{StandardBuilder, ColorSpecs};
use termcolor::NoColor;

let mut printer = StandardBuilder::new()
    .line_number(true)
    .build(NoColor::new(Vec::<u8>::new()));
```
*/

pub mod color;
pub mod hyperlink;
pub mod json;
pub mod standard;
pub mod stats;
pub mod summary;

// Re-export primary types for convenience.
pub use color::ColorSpecs;
pub use hyperlink::HyperlinkFormat;
pub use json::{JSON, JSONBuilder, JSONSink};
pub use standard::{Standard, StandardBuilder, StandardSink};
pub use stats::Stats;
pub use summary::{Summary, SummaryBuilder, SummaryKind, SummarySink};
