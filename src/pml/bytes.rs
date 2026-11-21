use super::PmlError;

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

    pub(crate) fn u32(&mut self) -> Result<u32, PmlError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
}
