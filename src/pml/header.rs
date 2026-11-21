use super::PmlError;
use super::bytes::ByteReader;

/// The fixed-size header at the start of a PML file.
#[derive(Debug, Clone)]
pub struct Header {}

impl Header {
    /// Size of the header in version 9.
    pub const SIZE: usize = 0x3A8;

    pub fn parse(bytes: &[u8]) -> Result<Header, PmlError> {
        let mut r = ByteReader::new(bytes);
        if r.bytes(4).map_err(|_| PmlError::NotPml)? != b"PML_" {
            return Err(PmlError::NotPml);
        }
        let version = r.u32()?;
        if version != 9 {
            return Err(PmlError::UnsupportedVersion(version));
        }
        if bytes.len() < Self::SIZE {
            return Err(PmlError::Truncated);
        }
        Ok(Header {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(version: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x3A8];
        b[0..4].copy_from_slice(b"PML_");
        b[4..8].copy_from_slice(&version.to_le_bytes());
        b
    }

    #[test]
    fn rejects_file_without_pml_signature() {
        let bytes = vec![0u8; 0x3A8];
        assert!(matches!(Header::parse(&bytes), Err(PmlError::NotPml)));
    }

    #[test]
    fn reports_truncation_instead_of_panicking_on_short_header() {
        let mut bytes = header_bytes(9);
        bytes.truncate(0x100);
        assert!(matches!(Header::parse(&bytes), Err(PmlError::Truncated)));
        assert!(matches!(Header::parse(b"PML_"), Err(PmlError::Truncated)));
    }

    #[test]
    fn rejects_versions_other_than_9_as_unsupported() {
        let bytes = header_bytes(10);
        assert!(matches!(
            Header::parse(&bytes),
            Err(PmlError::UnsupportedVersion(10))
        ));
    }
}
