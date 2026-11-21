use super::PmlError;

/// The fixed-size header at the start of a PML file.
#[derive(Debug, Clone)]
pub struct Header {}

impl Header {
    pub fn parse(bytes: &[u8]) -> Result<Header, PmlError> {
        if !bytes.starts_with(b"PML_") {
            return Err(PmlError::NotPml);
        }
        Ok(Header {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_file_without_pml_signature() {
        let bytes = vec![0u8; 0x3A8];
        assert!(matches!(Header::parse(&bytes), Err(PmlError::NotPml)));
    }
}
