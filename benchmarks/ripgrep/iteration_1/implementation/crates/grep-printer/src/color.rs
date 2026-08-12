/*!
Color configuration for search output.

This module provides [`ColorSpecs`] which configures colors for different
output elements (file paths, line numbers, column numbers, and matched text).
Colors can be configured programmatically or parsed from ripgrep-style color
specification strings.

# Color Specification Format

Specs have the form `TYPE:ATTR:VALUE` where:

- `TYPE` is one of: `match`, `path`, `line`, `column`
- `ATTR` is one of: `fg`, `bg`, `style`
- `VALUE` is a color name (`red`, `green`, etc.), an RGB triple (`R,G,B`),
  or a style keyword (`bold`, `nobold`, `italic`, `noitalic`, `underline`,
  `nounderline`, `intense`, `nointense`).
*/

use termcolor::{Color, ColorSpec};

/// Color specifications for different output elements.
#[derive(Clone, Debug)]
pub struct ColorSpecs {
    /// Color for file path labels.
    pub path: ColorSpec,
    /// Color for line number labels.
    pub line: ColorSpec,
    /// Color for column number labels.
    pub column: ColorSpec,
    /// Color for matched text highlights.
    pub matched: ColorSpec,
}

impl Default for ColorSpecs {
    fn default() -> Self {
        let mut path = ColorSpec::new();
        path.set_fg(Some(Color::Magenta));

        let mut line = ColorSpec::new();
        line.set_fg(Some(Color::Green));

        let mut column = ColorSpec::new();
        column.set_fg(Some(Color::Green));

        let mut matched = ColorSpec::new();
        matched.set_fg(Some(Color::Red)).set_bold(true);

        ColorSpecs {
            path,
            line,
            column,
            matched,
        }
    }
}

impl ColorSpecs {
    /// Creates a new `ColorSpecs` with default colors.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses color specifications from ripgrep-style spec strings.
    ///
    /// Each string must have the form `TYPE:ATTR:VALUE`. See the module
    /// documentation for the complete format.
    ///
    /// # Errors
    ///
    /// Returns an error if any spec string is malformed or contains an
    /// unknown type, attribute, color name, or style keyword.
    pub fn from_specs(specs: &[String]) -> Result<ColorSpecs, String> {
        let mut colors = ColorSpecs::default();
        for spec in specs {
            let parts: Vec<&str> = spec.split(':').collect();
            if parts.len() != 3 {
                return Err(format!("invalid color spec: {}", spec));
            }
            let color_spec = match parts[0] {
                "match" => &mut colors.matched,
                "path" => &mut colors.path,
                "line" => &mut colors.line,
                "column" => &mut colors.column,
                _ => return Err(format!("unknown color type: {}", parts[0])),
            };
            match parts[1] {
                "fg" => {
                    color_spec.set_fg(Some(parse_color(parts[2])?));
                }
                "bg" => {
                    color_spec.set_bg(Some(parse_color(parts[2])?));
                }
                "style" => {
                    apply_style(color_spec, parts[2])?;
                }
                _ => return Err(format!("unknown attribute: {}", parts[1])),
            }
        }
        Ok(colors)
    }
}

/// Parses a color name or RGB triple into a `termcolor::Color`.
fn parse_color(s: &str) -> Result<Color, String> {
    match s.to_lowercase().as_str() {
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "blue" => Ok(Color::Blue),
        "yellow" => Ok(Color::Yellow),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "white" => Ok(Color::White),
        "black" => Ok(Color::Black),
        _ => {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() == 3 {
                let r = parts[0]
                    .trim()
                    .parse::<u8>()
                    .map_err(|e| e.to_string())?;
                let g = parts[1]
                    .trim()
                    .parse::<u8>()
                    .map_err(|e| e.to_string())?;
                let b = parts[2]
                    .trim()
                    .parse::<u8>()
                    .map_err(|e| e.to_string())?;
                Ok(Color::Rgb(r, g, b))
            } else {
                Err(format!("unknown color: {}", s))
            }
        }
    }
}

/// Applies a style keyword to a `ColorSpec`.
fn apply_style(spec: &mut ColorSpec, style: &str) -> Result<(), String> {
    match style {
        "bold" => {
            spec.set_bold(true);
        }
        "nobold" => {
            spec.set_bold(false);
        }
        "italic" => {
            spec.set_italic(true);
        }
        "noitalic" => {
            spec.set_italic(false);
        }
        "underline" => {
            spec.set_underline(true);
        }
        "nounderline" => {
            spec.set_underline(false);
        }
        "intense" => {
            spec.set_intense(true);
        }
        "nointense" => {
            spec.set_intense(false);
        }
        _ => return Err(format!("unknown style: {}", style)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_colors() {
        let specs = ColorSpecs::default();
        assert_eq!(specs.path.fg(), Some(&Color::Magenta));
        assert_eq!(specs.line.fg(), Some(&Color::Green));
        assert_eq!(specs.column.fg(), Some(&Color::Green));
        assert_eq!(specs.matched.fg(), Some(&Color::Red));
        assert!(specs.matched.bold());
    }

    #[test]
    fn test_from_specs_fg() {
        let specs =
            ColorSpecs::from_specs(&["path:fg:blue".to_string()]).unwrap();
        assert_eq!(specs.path.fg(), Some(&Color::Blue));
    }

    #[test]
    fn test_from_specs_bg() {
        let specs =
            ColorSpecs::from_specs(&["match:bg:yellow".to_string()]).unwrap();
        assert_eq!(specs.matched.bg(), Some(&Color::Yellow));
    }

    #[test]
    fn test_from_specs_style() {
        let specs = ColorSpecs::from_specs(&[
            "line:style:bold".to_string(),
            "line:style:underline".to_string(),
        ])
        .unwrap();
        assert!(specs.line.bold());
        assert!(specs.line.underline());
    }

    #[test]
    fn test_from_specs_rgb() {
        let specs = ColorSpecs::from_specs(&["match:fg:255,128,0".to_string()])
            .unwrap();
        assert_eq!(specs.matched.fg(), Some(&Color::Rgb(255, 128, 0)));
    }

    #[test]
    fn test_from_specs_invalid_format() {
        let result = ColorSpecs::from_specs(&["invalid".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_specs_unknown_type() {
        let result =
            ColorSpecs::from_specs(&["unknown:fg:red".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_specs_unknown_attribute() {
        let result =
            ColorSpecs::from_specs(&["path:foobar:red".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_specs_unknown_color() {
        let result =
            ColorSpecs::from_specs(&["path:fg:rainbow".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_specs_unknown_style() {
        let result =
            ColorSpecs::from_specs(&["path:style:strikethrough".to_string()]);
        assert!(result.is_err());
    }
}
