use std::fmt;

use crate::Error;

/// Represents a parsed glob pattern token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A literal character.
    Literal(char),
    /// `*` — matches anything except path separator.
    Star,
    /// `**` — matches anything including path separators.
    DoubleStar,
    /// `?` — matches any single character except path separator.
    QuestionMark,
    /// A character class like `[abc]`.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    /// Start of an alternation group `{`.
    AlternateStart,
    /// Separator within alternation `,`.
    AlternateSep,
    /// End of an alternation group `}`.
    AlternateEnd,
    /// A path separator `/`.
    Separator,
}

/// Parse a glob pattern string into a sequence of tokens.
fn parse_glob(pattern: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut chars = pattern.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            '*' => {
                chars.next();
                if chars.peek() == Some(&'*') {
                    chars.next();
                    tokens.push(Token::DoubleStar);
                } else {
                    tokens.push(Token::Star);
                }
            }
            '?' => {
                chars.next();
                tokens.push(Token::QuestionMark);
            }
            '[' => {
                chars.next();
                let negated = match chars.peek() {
                    Some(&'!') | Some(&'^') => {
                        chars.next();
                        true
                    }
                    _ => false,
                };
                let mut ranges = Vec::new();
                // Allow `]` as first char in class
                if chars.peek() == Some(&']') {
                    chars.next();
                    ranges.push((']', ']'));
                }
                loop {
                    match chars.next() {
                        None => {
                            return Err(Error::glob(
                                pattern,
                                "unclosed character class '['",
                            ));
                        }
                        Some(']') => break,
                        Some(c1) => {
                            if chars.peek() == Some(&'-') {
                                chars.next(); // consume '-'
                                match chars.next() {
                                    None => {
                                        return Err(Error::glob(
                                            pattern,
                                            "unclosed character class '['",
                                        ));
                                    }
                                    Some(']') => {
                                        // e.g. [a-] => 'a' and '-'
                                        ranges.push((c1, c1));
                                        ranges.push(('-', '-'));
                                        break;
                                    }
                                    Some(c2) => {
                                        ranges.push((c1, c2));
                                    }
                                }
                            } else {
                                ranges.push((c1, c1));
                            }
                        }
                    }
                }
                tokens.push(Token::Class { negated, ranges });
            }
            '{' => {
                chars.next();
                tokens.push(Token::AlternateStart);
            }
            ',' => {
                chars.next();
                tokens.push(Token::AlternateSep);
            }
            '}' => {
                chars.next();
                tokens.push(Token::AlternateEnd);
            }
            '\\' => {
                chars.next();
                match chars.next() {
                    Some(escaped) => tokens.push(Token::Literal(escaped)),
                    None => {
                        return Err(Error::glob(
                            pattern,
                            "dangling escape character '\\'",
                        ));
                    }
                }
            }
            '/' => {
                chars.next();
                tokens.push(Token::Separator);
            }
            other => {
                chars.next();
                tokens.push(Token::Literal(other));
            }
        }
    }

    Ok(tokens)
}

/// Escape a character for use inside a regex pattern.
fn regex_escape(c: char) -> String {
    match c {
        '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\'
        | '!' | '#' | '&' | '-' | '~' => {
            format!("\\{c}")
        }
        _ => c.to_string(),
    }
}

/// Returns `true` if the given tokens contain a path separator (or a
/// `**` pattern which implies path-level matching).
fn has_path_sep(tokens: &[Token]) -> bool {
    for tok in tokens.iter() {
        match tok {
            Token::Separator => return true,
            Token::DoubleStar => {
                // `**` without being the whole pattern implies path matching
                if tokens.len() > 1 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns `true` if the pattern ends with a `/` separator (directory only).
fn ends_with_sep(tokens: &[Token]) -> bool {
    tokens.last() == Some(&Token::Separator)
}

/// Convert parsed glob tokens into a regex string.
///
/// If `basename_only` is true, the regex will match against only the
/// basename of a path. Otherwise it matches the full path.
fn tokens_to_regex(tokens: &[Token], basename_only: bool) -> String {
    let mut regex = String::new();

    if basename_only {
        // Match only the basename: anchor after the last `/` or at start
        regex.push_str("(?:^|/)");
    } else {
        regex.push('^');
    }

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Literal(c) => {
                regex.push_str(&regex_escape(*c));
            }
            Token::Star => {
                regex.push_str("[^/]*");
            }
            Token::DoubleStar => {
                // Handle `**/` prefix
                if i + 1 < tokens.len() && tokens[i + 1] == Token::Separator {
                    regex.push_str("(?:.*/)?");
                    i += 1; // skip the separator
                }
                // Handle `/**` suffix
                else if i > 0 && tokens[i - 1] == Token::Separator {
                    // We already emitted the separator, so we need to handle
                    // `/**` as matching `/anything` or nothing after the sep
                    // Actually, the separator was already written. We need to
                    // replace it. Let's handle differently.
                    regex.push_str(".*");
                }
                // Handle standalone `**`
                else if tokens.len() == 1 {
                    regex.push_str(".*");
                }
                // `**` in the middle: `a/**/b`
                else {
                    regex.push_str("(?:.*/)?");
                }
            }
            Token::QuestionMark => {
                regex.push_str("[^/]");
            }
            Token::Class { negated, ranges } => {
                regex.push('[');
                if *negated {
                    regex.push('^');
                }
                for (start, end) in ranges {
                    if start == end {
                        regex.push_str(&regex_class_escape(*start));
                    } else {
                        regex.push_str(&regex_class_escape(*start));
                        regex.push('-');
                        regex.push_str(&regex_class_escape(*end));
                    }
                }
                regex.push(']');
            }
            Token::AlternateStart => {
                regex.push_str("(?:");
            }
            Token::AlternateSep => {
                regex.push('|');
            }
            Token::AlternateEnd => {
                regex.push(')');
            }
            Token::Separator => {
                regex.push('/');
            }
        }
        i += 1;
    }

    regex.push('$');
    regex
}

/// Escape a character for use inside a regex character class `[...]`.
fn regex_class_escape(c: char) -> String {
    match c {
        ']' | '\\' | '^' | '-' => format!("\\{c}"),
        _ => c.to_string(),
    }
}

/// A parsed glob pattern.
///
/// A `Glob` is constructed from a glob pattern string and can then be
/// compiled into a `GlobMatcher` for matching against file paths.
///
/// # Example
///
/// ```
/// use globset::Glob;
///
/// let glob = Glob::new("*.rs").unwrap();
/// let matcher = glob.compile_matcher();
/// assert!(matcher.is_match("foo.rs"));
/// assert!(!matcher.is_match("foo.txt"));
/// ```
#[derive(Clone, Debug)]
pub struct Glob {
    /// The original glob pattern string.
    pattern: String,
    /// The compiled regex string.
    regex_str: String,
    /// Parsed token representation (retained for introspection).
    #[allow(dead_code)]
    tokens: Vec<Token>,
    /// Whether this glob pattern only matches against basenames.
    #[allow(dead_code)]
    is_basename_only: bool,
    /// Whether the pattern ends with a `/` (directory-only match).
    only_dir: bool,
}

impl Glob {
    /// Parse a glob pattern string.
    ///
    /// Returns an error if the pattern is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use globset::Glob;
    ///
    /// let glob = Glob::new("*.rs").unwrap();
    /// ```
    pub fn new(pattern: &str) -> Result<Glob, Error> {
        let tokens = parse_glob(pattern)?;
        let only_dir = ends_with_sep(&tokens);

        // Determine if this is a basename-only pattern.
        // A pattern is basename-only if it contains no path separators
        // and no `**` that implies path matching.
        let is_basename_only = !has_path_sep(&tokens);

        // Strip trailing separator for regex compilation (the only_dir
        // flag captures that information).
        let effective_tokens = if only_dir {
            &tokens[..tokens.len() - 1]
        } else {
            &tokens[..]
        };

        let regex_str = tokens_to_regex(effective_tokens, is_basename_only);

        // Validate that the regex compiles
        let _re = regex_automata::meta::Regex::new(&regex_str).map_err(|e| {
            Error::regex(&format!("error compiling regex '{}': {}", regex_str, e))
        })?;

        Ok(Glob {
            pattern: pattern.to_string(),
            regex_str,
            tokens,
            is_basename_only,
            only_dir,
        })
    }

    /// Returns the regex string this glob compiles to.
    pub fn regex(&self) -> &str {
        &self.regex_str
    }

    /// Compile this glob into a `GlobMatcher` for matching paths.
    ///
    /// # Example
    ///
    /// ```
    /// use globset::Glob;
    ///
    /// let glob = Glob::new("*.rs").unwrap();
    /// let matcher = glob.compile_matcher();
    /// assert!(matcher.is_match("src/main.rs"));
    /// ```
    pub fn compile_matcher(&self) -> crate::GlobMatcher {
        let re = regex_automata::meta::Regex::new(&self.regex_str)
            .expect("regex should already be validated");
        crate::GlobMatcher {
            glob: self.clone(),
            re,
        }
    }

    /// Returns `true` if this pattern should only match directories.
    ///
    /// A pattern matches only directories if it ends with a path separator `/`.
    pub fn is_only_dir(&self) -> bool {
        self.only_dir
    }

    /// Returns the original glob pattern string.
    pub fn glob(&self) -> &str {
        &self.pattern
    }

    /// Returns whether this glob matches against basenames only.
    #[allow(dead_code)]
    pub(crate) fn is_basename_only(&self) -> bool {
        self.is_basename_only
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_star() {
        let tokens = parse_glob("*.rs").unwrap();
        assert!(tokens.contains(&Token::Star));
    }

    #[test]
    fn test_parse_double_star() {
        let tokens = parse_glob("**/*.rs").unwrap();
        assert!(tokens.contains(&Token::DoubleStar));
    }

    #[test]
    fn test_parse_question_mark() {
        let tokens = parse_glob("?.rs").unwrap();
        assert!(tokens.contains(&Token::QuestionMark));
    }

    #[test]
    fn test_parse_character_class() {
        let tokens = parse_glob("[abc].rs").unwrap();
        assert!(matches!(
            tokens[0],
            Token::Class {
                negated: false,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_negated_class() {
        let tokens = parse_glob("[!abc].rs").unwrap();
        assert!(matches!(
            tokens[0],
            Token::Class { negated: true, .. }
        ));
    }

    #[test]
    fn test_parse_alternation() {
        let tokens = parse_glob("{a,b,c}").unwrap();
        assert!(tokens.contains(&Token::AlternateStart));
        assert!(tokens.contains(&Token::AlternateSep));
        assert!(tokens.contains(&Token::AlternateEnd));
    }

    #[test]
    fn test_parse_escape() {
        let tokens = parse_glob("\\*").unwrap();
        assert_eq!(tokens, vec![Token::Literal('*')]);
    }

    #[test]
    fn test_unclosed_class() {
        assert!(parse_glob("[abc").is_err());
    }

    #[test]
    fn test_dangling_escape() {
        assert!(parse_glob("foo\\").is_err());
    }

    #[test]
    fn test_has_path_sep() {
        let tokens = parse_glob("src/*.rs").unwrap();
        assert!(has_path_sep(&tokens));

        let tokens = parse_glob("*.rs").unwrap();
        assert!(!has_path_sep(&tokens));
    }

    #[test]
    fn test_only_dir() {
        let glob = Glob::new("src/").unwrap();
        assert!(glob.is_only_dir());

        let glob = Glob::new("src").unwrap();
        assert!(!glob.is_only_dir());
    }

    #[test]
    fn test_glob_regex_simple() {
        let glob = Glob::new("*.rs").unwrap();
        // Should be a basename-matching regex
        assert!(glob.regex().contains("(?:^|/)"));
    }

    #[test]
    fn test_glob_regex_path() {
        let glob = Glob::new("src/*.rs").unwrap();
        // Should be a full-path regex
        assert!(glob.regex().starts_with('^'));
    }
}
