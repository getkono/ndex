//! Length-prefixed framing and the magic preamble (PRD §12.2).
//!
//! Wire format: a big-endian `u32` length followed by that many payload bytes. Frames
//! larger than [`MAX_FRAME_BYTES`] are rejected. The server writes [`MAGIC_PREAMBLE`]
//! before any frames so the client can skip shell-startup stdout contamination.

use std::io::{Read, Write};

use ndex_core::constants::{MAGIC_PREAMBLE, MAX_FRAME_BYTES, MAX_PREAMBLE_SCAN_BYTES};
use ndex_core::error::{NdexError, Result};

/// Writes length-prefixed frames to an underlying writer.
pub struct FrameWriter<W: Write> {
    inner: W,
}

impl<W: Write> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Write the magic preamble (server side, once, before any frames).
    pub fn write_preamble(&mut self) -> Result<()> {
        self.inner.write_all(MAGIC_PREAMBLE)?;
        self.inner.flush()?;
        Ok(())
    }

    /// Write one frame. Errors if `payload` exceeds [`MAX_FRAME_BYTES`].
    pub fn write_frame(&mut self, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_FRAME_BYTES {
            return Err(NdexError::Protocol(format!(
                "frame size {} exceeds {MAX_FRAME_BYTES} byte limit",
                payload.len()
            )));
        }
        let len = u32::try_from(payload.len())
            .map_err(|_| NdexError::Protocol("frame length overflow".into()))?;
        self.inner.write_all(&len.to_be_bytes())?;
        self.inner.write_all(payload)?;
        self.inner.flush()?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// Reads length-prefixed frames from an underlying reader.
pub struct FrameReader<R: Read> {
    inner: R,
}

impl<R: Read> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// Read one frame. Errors if the advertised length exceeds [`MAX_FRAME_BYTES`].
    pub fn read_frame(&mut self) -> Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.inner.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(NdexError::Protocol(format!(
                "frame size {len} exceeds {MAX_FRAME_BYTES} byte limit"
            )));
        }
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Scan forward for [`MAGIC_PREAMBLE`], discarding up to [`MAX_PREAMBLE_SCAN_BYTES`]
    /// of leading garbage (shell-startup stdout). Errors if not found within the budget.
    pub fn scan_preamble(&mut self) -> Result<()> {
        let magic = MAGIC_PREAMBLE;
        let mut matched = 0usize;
        let mut consumed = 0usize;
        let mut byte = [0u8; 1];
        loop {
            self.inner
                .read_exact(&mut byte)
                .map_err(|e| NdexError::Protocol(format!("reading preamble: {e}")))?;
            consumed += 1;
            if byte[0] == magic[matched] {
                matched += 1;
                if matched == magic.len() {
                    return Ok(());
                }
            } else {
                // MAGIC_PREAMBLE has no self-overlap, so a single-byte restart is correct.
                matched = usize::from(byte[0] == magic[0]);
            }
            if consumed > MAX_PREAMBLE_SCAN_BYTES + magic.len() {
                return Err(NdexError::Protocol(
                    "protocol preamble not found; server stdout may be contaminated by shell \
                     startup output (see PRD §12.2)"
                        .into(),
                ));
            }
        }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        {
            let mut w = FrameWriter::new(&mut buf);
            w.write_frame(b"hello").unwrap();
            w.write_frame(b"world").unwrap();
        }
        let mut r = FrameReader::new(Cursor::new(buf));
        assert_eq!(r.read_frame().unwrap(), b"hello");
        assert_eq!(r.read_frame().unwrap(), b"world");
    }

    #[test]
    fn oversize_frame_is_rejected() {
        // Forge a length prefix above the cap without allocating the payload.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(u32::MAX).to_be_bytes());
        let mut r = FrameReader::new(Cursor::new(bytes));
        assert!(r.read_frame().is_err());
    }

    #[test]
    fn preamble_scan_skips_garbage() {
        let mut stream = Vec::new();
        stream.extend_from_slice(b"motd: welcome to nas\n");
        stream.extend_from_slice(MAGIC_PREAMBLE);
        {
            let mut w = FrameWriter::new(&mut stream);
            w.write_frame(b"after").unwrap();
        }
        let mut r = FrameReader::new(Cursor::new(stream));
        r.scan_preamble().unwrap();
        assert_eq!(r.read_frame().unwrap(), b"after");
    }

    #[test]
    fn preamble_scan_fails_on_contaminated_stream() {
        let garbage = vec![b'x'; MAX_PREAMBLE_SCAN_BYTES + 100];
        let mut r = FrameReader::new(Cursor::new(garbage));
        assert!(r.scan_preamble().is_err());
    }
}
