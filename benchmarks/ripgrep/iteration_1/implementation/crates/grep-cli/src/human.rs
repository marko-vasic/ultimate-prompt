/// Human-readable size parsing utilities.
///
/// This module provides a function for parsing human-readable size strings
/// like `1K`, `10MB`, `1G`, etc., into their corresponding byte values.

/// Parse a human-readable size string into a byte count.
///
/// The following suffixes are supported (case-insensitive):
///
/// - `K` or `KB` — multiply by 1024
/// - `M` or `MB` — multiply by 1048576 (1024²)
/// - `G` or `GB` — multiply by 1073741824 (1024³)
///
/// If no suffix is provided, the string is parsed as a plain byte count.
///
/// # Examples
///
/// ```
/// use grep_cli::parse_human_readable_size;
///
/// assert_eq!(parse_human_readable_size("1K").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("1KB").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("10M").unwrap(), 10 * 1048576);
/// assert_eq!(parse_human_readable_size("1G").unwrap(), 1073741824);
/// assert_eq!(parse_human_readable_size("1024").unwrap(), 1024);
/// assert_eq!(parse_human_readable_size("500kb").unwrap(), 500 * 1024);
/// ```
///
/// # Errors
///
/// Returns an error if the string cannot be parsed, if the numeric portion
/// is invalid, or if the suffix is unrecognized.
pub fn parse_human_readable_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty string is not a valid size".to_string());
    }

    // Find the boundary between digits and suffix.
    let digit_end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());

    if digit_end == 0 {
        return Err(format!(
            "invalid size '{}': no numeric component found",
            s
        ));
    }

    let number_str = &s[..digit_end];
    let suffix = s[digit_end..].trim();

    let number: u64 = number_str.parse().map_err(|e| {
        format!("invalid numeric value '{}': {}", number_str, e)
    })?;

    let multiplier = match suffix.to_ascii_uppercase().as_str() {
        "" => 1u64,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        _ => {
            return Err(format!(
                "unrecognized size suffix '{}'. Expected one of: \
                 K, KB, M, MB, G, GB (case-insensitive)",
                suffix
            ));
        }
    };

    number.checked_mul(multiplier).ok_or_else(|| {
        format!("size '{}' overflows u64", s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_number() {
        assert_eq!(parse_human_readable_size("1024").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("0").unwrap(), 0);
        assert_eq!(parse_human_readable_size("1").unwrap(), 1);
    }

    #[test]
    fn test_kilobytes() {
        assert_eq!(parse_human_readable_size("1K").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1KB").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1k").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("1kb").unwrap(), 1024);
        assert_eq!(parse_human_readable_size("500KB").unwrap(), 500 * 1024);
    }

    #[test]
    fn test_megabytes() {
        assert_eq!(parse_human_readable_size("1M").unwrap(), 1048576);
        assert_eq!(parse_human_readable_size("1MB").unwrap(), 1048576);
        assert_eq!(parse_human_readable_size("10M").unwrap(), 10 * 1048576);
        assert_eq!(parse_human_readable_size("10mb").unwrap(), 10 * 1048576);
    }

    #[test]
    fn test_gigabytes() {
        assert_eq!(parse_human_readable_size("1G").unwrap(), 1073741824);
        assert_eq!(parse_human_readable_size("1GB").unwrap(), 1073741824);
        assert_eq!(parse_human_readable_size("2g").unwrap(), 2 * 1073741824);
    }

    #[test]
    fn test_whitespace_trimming() {
        assert_eq!(parse_human_readable_size("  1K  ").unwrap(), 1024);
    }

    #[test]
    fn test_invalid_inputs() {
        assert!(parse_human_readable_size("").is_err());
        assert!(parse_human_readable_size("K").is_err());
        assert!(parse_human_readable_size("abc").is_err());
        assert!(parse_human_readable_size("1T").is_err());
        assert!(parse_human_readable_size("1TB").is_err());
    }
}
