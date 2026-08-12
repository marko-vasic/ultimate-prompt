/*!
A line-oriented buffer for reading from byte sources.

This module provides [`LineBufferReader`], which wraps an [`io::Read`] source
and reads content into an internal buffer. The buffer can then be consumed
as a single byte slice for use with `search_slice`.
*/

use std::io::{self, Read};

/// Default buffer capacity (8 KiB).
const DEFAULT_CAPACITY: usize = 8 * 1024;

/// A buffer that reads an entire source into memory.
///
/// This is the simplest strategy: read everything into a `Vec<u8>` so that
/// the search can operate on a contiguous `&[u8]`.
#[derive(Clone, Debug)]
pub struct LineBuffer {
    /// The internal buffer.
    buf: Vec<u8>,
    /// The configured capacity hint.
    capacity: usize,
}

impl LineBuffer {
    /// Creates a new `LineBuffer` with the default capacity.
    pub fn new() -> Self {
        LineBuffer {
            buf: Vec::with_capacity(DEFAULT_CAPACITY),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// Creates a new `LineBuffer` with the given initial capacity.
    pub fn with_capacity(cap: usize) -> Self {
        LineBuffer {
            buf: Vec::with_capacity(cap),
            capacity: cap,
        }
    }

    /// Clears the buffer, discarding all previously-read content.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Returns the buffered bytes.
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// Returns the number of bytes currently in the buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Reads all bytes from `rdr` into the internal buffer.
    ///
    /// On success, returns the number of bytes read.
    pub fn read_all<R: Read>(&mut self, mut rdr: R) -> io::Result<usize> {
        self.buf.clear();
        let n = rdr.read_to_end(&mut self.buf)?;
        Ok(n)
    }
}

impl Default for LineBuffer {
    fn default() -> Self {
        LineBuffer::new()
    }
}

/// A convenience wrapper that pairs a [`LineBuffer`] with a reader.
pub struct LineBufferReader<R> {
    rdr: R,
    buf: LineBuffer,
}

impl<R: Read> LineBufferReader<R> {
    /// Creates a new `LineBufferReader` wrapping the given reader.
    pub fn new(rdr: R) -> Self {
        LineBufferReader {
            rdr,
            buf: LineBuffer::new(),
        }
    }

    /// Reads the entire contents of the reader into the buffer, then returns
    /// the buffered bytes.
    pub fn fill(&mut self) -> io::Result<&[u8]> {
        self.buf.read_all(&mut self.rdr)?;
        Ok(self.buf.buffer())
    }

    /// Returns the buffered bytes (without reading more).
    pub fn buffer(&self) -> &[u8] {
        self.buf.buffer()
    }

    /// Returns a reference to the underlying `LineBuffer`.
    pub fn line_buffer(&self) -> &LineBuffer {
        &self.buf
    }

    /// Consumes this reader, returning the underlying `LineBuffer`.
    pub fn into_line_buffer(self) -> LineBuffer {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_buffer_read_all() {
        let data = b"hello\nworld\n";
        let mut buf = LineBuffer::new();
        let n = buf.read_all(&data[..]).unwrap();
        assert_eq!(n, data.len());
        assert_eq!(buf.buffer(), data);
    }

    #[test]
    fn test_line_buffer_clear() {
        let mut buf = LineBuffer::new();
        buf.read_all(&b"test"[..]).unwrap();
        assert!(!buf.is_empty());
        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_line_buffer_reader() {
        let data = b"line1\nline2\n";
        let mut reader = LineBufferReader::new(&data[..]);
        let bytes = reader.fill().unwrap();
        assert_eq!(bytes, data);
    }
}
