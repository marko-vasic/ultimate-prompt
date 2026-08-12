/*!
Hyperlink formatting for terminal emulators.

This module provides [`HyperlinkFormat`] for generating clickable hyperlinks
in terminal output using the OSC 8 escape sequence. Predefined formats are
available for common editors (VS Code, JetBrains, TextMate, MacVim) and
custom templates can be supplied.

# Template Variables

Templates may contain the following placeholders:

- `{path}` — absolute file path
- `{host}` — hostname
- `{line}` — 1-based line number
- `{column}` — 1-based column number
*/

use std::path::Path;

/// A hyperlink format template.
///
/// This can be one of several predefined formats (e.g. `file`, `vscode`) or
/// a custom template string. A "none" format disables hyperlink output.
#[derive(Clone, Debug)]
pub struct HyperlinkFormat {
    template: String,
}

impl HyperlinkFormat {
    /// Parses a hyperlink format from a string.
    ///
    /// Recognized preset names:
    ///
    /// - `"none"` or `""` — disable hyperlinks.
    /// - `"file"` — `file://{host}{path}`
    /// - `"vscode"` — `vscode://file{path}:{line}:{column}`
    /// - `"vscode-insiders"` — VS Code Insiders format
    /// - `"jetbrains"` — JetBrains IDE format
    /// - `"macvim"` — MacVim format
    /// - `"textmate"` — TextMate format
    ///
    /// Any other string is treated as a custom template.
    pub fn from_str(s: &str) -> Result<Self, String> {
        let template = match s {
            "none" | "" => return Ok(HyperlinkFormat { template: String::new() }),
            "file" => "file://{host}{path}".to_string(),
            "vscode" => "vscode://file{path}:{line}:{column}".to_string(),
            "vscode-insiders" => {
                "vscode-insiders://file{path}:{line}:{column}".to_string()
            }
            "jetbrains" => {
                "jetbrains://open?file={path}&line={line}&column={column}"
                    .to_string()
            }
            "macvim" => {
                "mvim://open?url=file://{path}&line={line}&column={column}"
                    .to_string()
            }
            "textmate" => {
                "txmt://open?url=file://{path}&line={line}&column={column}"
                    .to_string()
            }
            other => other.to_string(),
        };
        Ok(HyperlinkFormat { template })
    }

    /// Returns `true` if this format is disabled (i.e., "none").
    pub fn is_none(&self) -> bool {
        self.template.is_empty()
    }

    /// Renders this hyperlink for the given path, line, and column.
    ///
    /// Returns `None` if the format is disabled.
    pub fn render(
        &self,
        path: &Path,
        line: u64,
        column: u64,
    ) -> Option<String> {
        if self.is_none() {
            return None;
        }
        let host = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path_str = path.to_string_lossy();
        let result = self
            .template
            .replace("{path}", &path_str)
            .replace("{host}", &host)
            .replace("{line}", &line.to_string())
            .replace("{column}", &column.to_string());
        Some(result)
    }

    /// Returns the raw template string.
    pub fn template(&self) -> &str {
        &self.template
    }
}

/// Attempts to get the hostname. This is a best-effort helper that doesn't
/// depend on external crates; it falls back to an empty string.
mod hostname {
    use std::ffi::OsString;

    pub fn get() -> Result<OsString, ()> {
        // Use the HOSTNAME environment variable as a portable fallback.
        if let Ok(val) = std::env::var("HOSTNAME") {
            return Ok(OsString::from(val));
        }
        // Try reading /etc/hostname on Linux.
        if let Ok(contents) = std::fs::read_to_string("/etc/hostname") {
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                return Ok(OsString::from(trimmed));
            }
        }
        Ok(OsString::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_format() {
        let fmt = HyperlinkFormat::from_str("none").unwrap();
        assert!(fmt.is_none());
        assert!(fmt.render(Path::new("/foo"), 1, 1).is_none());
    }

    #[test]
    fn test_empty_format() {
        let fmt = HyperlinkFormat::from_str("").unwrap();
        assert!(fmt.is_none());
    }

    #[test]
    fn test_file_format() {
        let fmt = HyperlinkFormat::from_str("file").unwrap();
        assert!(!fmt.is_none());
        assert!(fmt.template().contains("{host}"));
        assert!(fmt.template().contains("{path}"));
    }

    #[test]
    fn test_vscode_format() {
        let fmt = HyperlinkFormat::from_str("vscode").unwrap();
        let rendered = fmt.render(Path::new("/tmp/test.rs"), 10, 5);
        assert!(rendered.is_some());
        let rendered = rendered.unwrap();
        assert!(rendered.contains("/tmp/test.rs"));
        assert!(rendered.contains(":10:"));
        assert!(rendered.contains(":5"));
    }

    #[test]
    fn test_custom_format() {
        let fmt = HyperlinkFormat::from_str("myide://{path}#{line}").unwrap();
        let rendered = fmt.render(Path::new("/foo/bar.rs"), 42, 1).unwrap();
        assert_eq!(rendered, "myide:///foo/bar.rs#42");
    }
}
