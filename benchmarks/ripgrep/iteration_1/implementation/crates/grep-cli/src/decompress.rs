/// Decompression reader utilities.
///
/// This module provides a `DecompressionReader` that can transparently
/// decompress files by spawning the appropriate decompression command
/// based on the file extension.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A reader that decompresses data from a file by spawning an external
/// decompression command.
///
/// The decompression command is chosen based on the file extension:
///
/// | Extension | Command |
/// |-----------|---------|
/// | `.gz`     | `gzip -d` |
/// | `.bz2`    | `bzip2 -d` |
/// | `.xz`     | `xz -d` |
/// | `.lz4`    | `lz4 -d` |
/// | `.lzma`   | `xz --format=lzma -d` |
/// | `.zst`    | `zstd -dq` |
/// | `.Z`      | `uncompress` |
/// | `.br`     | `brotli -d` |
///
/// The decompression command reads from the given file and writes to
/// stdout, which is captured by this reader.
pub struct DecompressionReader {
    child: Child,
    /// Retained for diagnostics/debugging.
    _path: PathBuf,
}

impl DecompressionReader {
    /// Create a new `DecompressionReader` for the given file path.
    ///
    /// The appropriate decompression command is determined by the file's
    /// extension. The command is spawned immediately, and its stdout is
    /// available for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file extension is not recognized
    /// - The decompression tool cannot be found or spawned
    pub fn new(path: &Path) -> io::Result<DecompressionReader> {
        let (program, args) = decompression_command(path)?;

        let child = Command::new(&program)
            .args(&args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "failed to spawn decompression command '{}': {}",
                        program, e
                    ),
                )
            })?;

        Ok(DecompressionReader {
            child,
            _path: path.to_path_buf(),
        })
    }
}

impl Read for DecompressionReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.child
            .stdout
            .as_mut()
            .expect("child stdout should be piped")
            .read(buf)
    }
}

impl Drop for DecompressionReader {
    fn drop(&mut self) {
        // Attempt to wait for the child process to avoid zombies.
        let _ = self.child.wait();
    }
}

/// Determine the decompression command and arguments for a given file path
/// based on its extension.
fn decompression_command(path: &Path) -> io::Result<(String, Vec<String>)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cannot determine decompression command: \
                     '{}' has no file extension",
                    path.display()
                ),
            )
        })?;

    let (program, args): (&str, &[&str]) = match ext {
        "gz" => ("gzip", &["-d", "-c"]),
        "bz2" => ("bzip2", &["-d", "-c"]),
        "xz" => ("xz", &["-d", "-c"]),
        "lz4" => ("lz4", &["-d", "-c"]),
        "lzma" => ("xz", &["--format=lzma", "-d", "-c"]),
        "zst" | "zstd" => ("zstd", &["-dq", "-c"]),
        "Z" => ("uncompress", &["-c"]),
        "br" => ("brotli", &["-d", "-c"]),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "unrecognized file extension '{}' for decompression \
                     in path '{}'",
                    ext,
                    path.display()
                ),
            ));
        }
    };

    Ok((
        program.to_string(),
        args.iter().map(|s| s.to_string()).collect(),
    ))
}
