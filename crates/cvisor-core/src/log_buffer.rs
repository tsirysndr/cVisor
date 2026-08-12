//! Thread-safe in-memory output buffer.
//!
//! Guest writes to fd 1/2 that are not tracked in the FdTable land here rather
//! than the real kernel. The Node SDK `Stream` drains it via `read`. Entirely
//! in-memory for now — add disk buffering if this starts OOMing.

use std::sync::Mutex;

#[derive(Default)]
pub struct LogBuffer {
    inner: Mutex<Vec<u8>>,
}

impl LogBuffer {
    pub fn new() -> LogBuffer {
        LogBuffer {
            inner: Mutex::new(Vec::new()),
        }
    }

    /// Append data to the buffer.
    pub fn write(&self, data: &[u8]) {
        self.inner.lock().unwrap().extend_from_slice(data);
    }

    /// Return a copy of accumulated data and clear the buffer.
    pub fn read(&self) -> Vec<u8> {
        let mut buf = self.inner.lock().unwrap();
        std::mem::take(&mut *buf)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let buf = LogBuffer::new();
        buf.write(b"hello");
        assert_eq!(buf.len(), 5);

        let data = buf.read();
        assert_eq!(data, b"hello");
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn read_returns_independent_copy() {
        let buf = LogBuffer::new();
        buf.write(b"before");
        let copy = buf.read();
        buf.write(b" after");
        assert_eq!(copy, b"before");
    }

    #[test]
    fn successive_drains() {
        let buf = LogBuffer::new();
        buf.write(b"first");
        assert_eq!(buf.read(), b"first");
        buf.write(b"second");
        assert_eq!(buf.read(), b"second");
    }
}
