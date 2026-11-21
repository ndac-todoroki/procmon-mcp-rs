use super::PmlError;

/// The fixed-size header at the start of a PML file.
#[derive(Debug, Clone)]
pub struct Header {}

impl Header {
    pub fn parse(bytes: &[u8]) -> Result<Header, PmlError> {
        if !bytes.starts_with(b"PML_") {
            return Err(PmlError::NotPml);
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != 9 {
            return Err(PmlError::UnsupportedVersion(version));
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
    fn rejects_versions_other_than_9_as_unsupported() {
        let bytes = header_bytes(10);
        assert!(matches!(
            Header::parse(&bytes),
            Err(PmlError::UnsupportedVersion(10))
        ));
    }
}
