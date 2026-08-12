/// Command reader utilities.
///
/// This module provides a `CommandReader` that wraps a child process and
/// reads from its stdout, implementing the `Read` trait.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};

/// A reader that wraps a spawned child process and reads from its stdout.
///
/// When the `CommandReader` is dropped, it waits for the child process to
/// exit to avoid leaving zombie processes.
///
/// # Examples
///
/// ```no_run
/// use std::io::Read;
/// use std::process::Command;
/// use grep_cli::CommandReader;
///
/// let mut cmd = Command::new("echo");
/// cmd.arg("hello");
/// let mut reader = CommandReader::new(&mut cmd).unwrap();
///
/// let mut output = String::new();
/// reader.read_to_string(&mut output).unwrap();
/// assert_eq!(output.trim(), "hello");
/// ```
pub struct CommandReader {
    child: Child,
}

impl CommandReader {
    /// Spawn the given command and return a reader for its stdout.
    ///
    /// The command's stdout is set to `Stdio::piped()` so it can be read.
    /// Stdin is set to `Stdio::null()` and stderr is inherited.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    pub fn new(cmd: &mut Command) -> io::Result<CommandReader> {
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(CommandReader { child })
    }

    /// Consume this reader and return the underlying child process.
    ///
    /// After calling this, the caller is responsible for waiting on the
    /// child process.
    pub fn into_inner(self) -> Child {
        // Use ManuallyDrop to prevent our Drop impl from running.
        let mut this = std::mem::ManuallyDrop::new(self);
        // SAFETY: We're consuming self, and ManuallyDrop prevents double-free.
        // We move the child out.
        unsafe { std::ptr::read(&mut this.child) }
    }
}

impl Read for CommandReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.child
            .stdout
            .as_mut()
            .expect("child stdout should be piped")
            .read(buf)
    }
}

impl Drop for CommandReader {
    fn drop(&mut self) {
        // Kill the child process if it's still running, then wait.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
