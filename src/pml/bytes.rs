use std::io::{Read, Seek, SeekFrom};

use super::PmlError;

/// Reads exactly `len` bytes. Lengths come from the file itself, so the buffer grows only
/// as data actually arrives instead of trusting a possibly corrupt length up front.
pub(crate) fn read_vec<R: Read>(r: &mut R, len: u64) -> Result<Vec<u8>, PmlError> {
    let mut v = Vec::new();
    r.take(len).read_to_end(&mut v)?;
    if v.len() as u64 != len {
        return Err(PmlError::Truncated);
    }
    Ok(v)
}

pub(crate) fn read_u32<R: Read>(r: &mut R) -> Result<u32, PmlError> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(eof_as_truncated)?;
    Ok(u32::from_le_bytes(b))
}

pub(crate) fn seek_to<R: Seek>(r: &mut R, pos: u64) -> Result<(), PmlError> {
    r.seek(SeekFrom::Start(pos))?;
    Ok(())
}

fn eof_as_truncated(e: std::io::Error) -> PmlError {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        PmlError::Truncated
    } else {
        PmlError::Io(e)
    }
}

/// Decodes UTF-16LE, stopping at the first NUL if any.
pub(crate) fn utf16_to_string(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Little-endian reader over a byte slice that reports truncation instead of panicking.
#[derive(Debug, Clone)]
pub(crate) struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        ByteReader { buf, pos: 0 }
    }

    pub(crate) fn bytes(&mut self, len: usize) -> Result<&'a [u8], PmlError> {
        let end = self.pos.checked_add(len).ok_or(PmlError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(PmlError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PmlError> {
        Ok(self.bytes(N)?.try_into().expect("slice has length N"))
    }

    pub(crate) fn u8(&mut self) -> Result<u8, PmlError> {
        Ok(self.bytes(1)?[0])
    }

    pub(crate) fn i64(&mut self) -> Result<i64, PmlError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    pub(crate) fn skip(&mut self, len: usize) -> Result<(), PmlError> {
        self.bytes(len).map(|_| ())
    }

    pub(crate) fn u16(&mut self) -> Result<u16, PmlError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, PmlError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, PmlError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn seek(&mut self, pos: usize) -> Result<(), PmlError> {
        if pos > self.buf.len() {
            return Err(PmlError::Truncated);
        }
        self.pos = pos;
        Ok(())
    }
}
