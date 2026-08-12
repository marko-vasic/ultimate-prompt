//! Error types for the `ignore` crate.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// An error that can occur in the ignore crate.
///
/// This covers IO errors, glob parsing errors, and gitignore parse errors.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    /// An IO error with optional path context.
    Io {
        path: Option<PathBuf>,
        err: io::Error,
    },
    /// A glob pattern error.
    Glob {
        glob: Option<String>,
        err: String,
    },
    /// A gitignore parse error.
    Parse {
        path: Option<PathBuf>,
        line: Option<u64>,
        msg: String,
    },
    /// Multiple errors.
    Multi(Vec<Error>),
    /// A walkdir error.
    WithDepth {
        depth: usize,
        err: Box<Error>,
    },
    /// A loop was detected during directory traversal.
    Loop {
        ancestor: PathBuf,
        child: PathBuf,
    },
}

impl Error {
    /// Create an IO error with path context.
    pub(crate) fn io(path: &Path, err: io::Error) -> Error {
        Error {
            kind: ErrorKind::Io {
                path: Some(path.to_path_buf()),
                err,
            },
        }
    }

    /// Create an IO error without path context.
    pub(crate) fn io_plain(err: io::Error) -> Error {
        Error {
            kind: ErrorKind::Io {
                path: None,
                err,
            },
        }
    }

    /// Create a glob error.
    pub(crate) fn glob(msg: &str) -> Error {
        Error {
            kind: ErrorKind::Glob {
                glob: None,
                err: msg.to_string(),
            },
        }
    }

    /// Create a glob error with the pattern.
    pub(crate) fn glob_with(glob: &str, msg: &str) -> Error {
        Error {
            kind: ErrorKind::Glob {
                glob: Some(glob.to_string()),
                err: msg.to_string(),
            },
        }
    }

    /// Create a parse error.
    pub(crate) fn parse(path: Option<&Path>, line: Option<u64>, msg: String) -> Error {
        Error {
            kind: ErrorKind::Parse {
                path: path.map(|p| p.to_path_buf()),
                line,
                msg,
            },
        }
    }

    /// Create a multi-error.
    pub(crate) fn multi(errs: Vec<Error>) -> Error {
        Error {
            kind: ErrorKind::Multi(errs),
        }
    }

    /// Create a depth-annotated error.
    pub(crate) fn with_depth(depth: usize, err: Error) -> Error {
        Error {
            kind: ErrorKind::WithDepth {
                depth,
                err: Box::new(err),
            },
        }
    }

    /// Create a loop error.
    pub(crate) fn _loop(ancestor: &Path, child: &Path) -> Error {
        Error {
            kind: ErrorKind::Loop {
                ancestor: ancestor.to_path_buf(),
                child: child.to_path_buf(),
            },
        }
    }

    /// Returns true if this is an IO error.
    pub fn is_io(&self) -> bool {
        matches!(self.kind, ErrorKind::Io { .. })
    }

    /// Returns the depth at which this error occurred, if available.
    pub fn depth(&self) -> Option<usize> {
        match &self.kind {
            ErrorKind::WithDepth { depth, .. } => Some(*depth),
            _ => None,
        }
    }

    /// Returns the IO error, if this is one.
    pub fn io_error(&self) -> Option<&io::Error> {
        match &self.kind {
            ErrorKind::Io { err, .. } => Some(err),
            ErrorKind::WithDepth { err, .. } => err.io_error(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Io { path: Some(p), err } => {
                write!(f, "{}: {}", p.display(), err)
            }
            ErrorKind::Io { path: None, err } => {
                write!(f, "{}", err)
            }
            ErrorKind::Glob {
                glob: Some(g),
                err,
            } => {
                write!(f, "glob '{}': {}", g, err)
            }
            ErrorKind::Glob { glob: None, err } => {
                write!(f, "glob: {}", err)
            }
            ErrorKind::Parse {
                path: Some(p),
                line: Some(l),
                msg,
            } => {
                write!(f, "{}:{}: {}", p.display(), l, msg)
            }
            ErrorKind::Parse {
                path: Some(p),
                line: None,
                msg,
            } => {
                write!(f, "{}: {}", p.display(), msg)
            }
            ErrorKind::Parse {
                path: None,
                line: Some(l),
                msg,
            } => {
                write!(f, "line {}: {}", l, msg)
            }
            ErrorKind::Parse {
                path: None,
                line: None,
                msg,
            } => {
                write!(f, "{}", msg)
            }
            ErrorKind::Multi(errs) => {
                for (i, err) in errs.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", err)?;
                }
                Ok(())
            }
            ErrorKind::WithDepth { err, .. } => {
                write!(f, "{}", err)
            }
            ErrorKind::Loop { ancestor, child } => {
                write!(
                    f,
                    "file system loop found: {} points to {}",
                    child.display(),
                    ancestor.display()
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Io { err, .. } => Some(err),
            ErrorKind::WithDepth { err, .. } => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error {
            kind: ErrorKind::Io {
                path: None,
                err,
            },
        }
    }
}

impl From<globset::Error> for Error {
    fn from(err: globset::Error) -> Error {
        Error {
            kind: ErrorKind::Glob {
                glob: None,
                err: err.to_string(),
            },
        }
    }
}

impl From<walkdir::Error> for Error {
    fn from(err: walkdir::Error) -> Error {
        let depth = err.depth();
        let io_err = match err.into_io_error() {
            Some(e) => Error::io_plain(e),
            None => Error::glob("walkdir error"),
        };
        Error::with_depth(depth, io_err)
    }
}
