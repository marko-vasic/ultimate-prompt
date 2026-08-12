/*!
Search statistics tracking.

This module provides [`Stats`], an aggregate counter for search operations.
Stats can be accumulated across multiple searches and used to produce a
summary at the end of a run.
*/

use std::time::Duration;

/// Aggregate statistics for search operations.
///
/// This tracks counts of matches, matched lines, files searched, bytes
/// processed, and total search duration. Instances can be combined with
/// [`add`](Stats::add) for multi-file summaries.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Total number of individual matches found.
    pub matches: u64,
    /// Total number of lines containing at least one match.
    pub matched_lines: u64,
    /// Number of files that had at least one match.
    pub files_with_matches: u64,
    /// Total number of files searched.
    pub files_searched: u64,
    /// Total number of bytes searched across all files.
    pub bytes_searched: u64,
    /// Total number of bytes printed as output.
    pub bytes_printed: u64,
    /// Total wall-clock time spent searching.
    pub search_duration: Duration,
}

impl Stats {
    /// Creates a new zeroed `Stats`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the counts from `other` into `self`.
    pub fn add(&mut self, other: &Stats) {
        self.matches += other.matches;
        self.matched_lines += other.matched_lines;
        self.files_with_matches += other.files_with_matches;
        self.files_searched += other.files_searched;
        self.bytes_searched += other.bytes_searched;
        self.bytes_printed += other.bytes_printed;
        self.search_duration += other.search_duration;
    }

    /// Returns `true` if no matches were recorded.
    pub fn is_empty(&self) -> bool {
        self.matches == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_default() {
        let s = Stats::new();
        assert_eq!(s.matches, 0);
        assert_eq!(s.matched_lines, 0);
        assert_eq!(s.files_with_matches, 0);
        assert_eq!(s.files_searched, 0);
        assert_eq!(s.bytes_searched, 0);
        assert_eq!(s.bytes_printed, 0);
        assert_eq!(s.search_duration, Duration::ZERO);
        assert!(s.is_empty());
    }

    #[test]
    fn test_stats_add() {
        let mut a = Stats::new();
        a.matches = 5;
        a.matched_lines = 3;
        a.files_with_matches = 1;
        a.files_searched = 2;
        a.bytes_searched = 1000;
        a.bytes_printed = 100;
        a.search_duration = Duration::from_millis(50);

        let mut b = Stats::new();
        b.matches = 10;
        b.matched_lines = 7;
        b.files_with_matches = 2;
        b.files_searched = 3;
        b.bytes_searched = 2000;
        b.bytes_printed = 200;
        b.search_duration = Duration::from_millis(100);

        a.add(&b);
        assert_eq!(a.matches, 15);
        assert_eq!(a.matched_lines, 10);
        assert_eq!(a.files_with_matches, 3);
        assert_eq!(a.files_searched, 5);
        assert_eq!(a.bytes_searched, 3000);
        assert_eq!(a.bytes_printed, 300);
        assert_eq!(a.search_duration, Duration::from_millis(150));
        assert!(!a.is_empty());
    }
}
