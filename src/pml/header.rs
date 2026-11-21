use super::PmlError;
use super::bytes::ByteReader;

/// The fixed-size header at the start of a PML file.
#[derive(Debug, Clone)]
pub struct Header {
    /// Bitness of the captured system. Pointer-sized fields everywhere follow this,
    /// not the bitness of the individual process.
    pub is_64bit: bool,
    pub number_of_events: u32,
    pub events_offsets_array_offset: u64,
    pub process_table_offset: u64,
    pub strings_table_offset: u64,
}

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
        let is_64bit = r.u32()? != 0;
        r.seek(0x234)?;
        let number_of_events = r.u32()?;
        r.seek(0x240)?;
        let events_offset = r.u64()?;
        let events_offsets_array_offset = r.u64()?;
        let process_table_offset = r.u64()?;
        let strings_table_offset = r.u64()?;
        let icon_table_offset = r.u64()?;
        if [
            events_offset,
            events_offsets_array_offset,
            process_table_offset,
            strings_table_offset,
            icon_table_offset,
        ]
        .contains(&0)
        {
            return Err(PmlError::NotClosedCleanly);
        }
        Ok(Header {
            is_64bit,
            number_of_events,
            events_offsets_array_offset,
            process_table_offset,
            strings_table_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header that passes validation; table offsets point just past the header.
    fn header_bytes(version: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x3A8];
        b[0..4].copy_from_slice(b"PML_");
        b[4..8].copy_from_slice(&version.to_le_bytes());
        for off in [0x240, 0x248, 0x250, 0x258, 0x260] {
            b[off..off + 8].copy_from_slice(&0x3A8u64.to_le_bytes());
        }
        b
    }

    #[test]
    fn rejects_log_whose_tables_were_never_written() {
        // Procmon fills in the table offsets only when it closes the log cleanly.
        let mut bytes = header_bytes(9);
        bytes[0x250..0x258].fill(0);
        assert!(matches!(
            Header::parse(&bytes),
            Err(PmlError::NotClosedCleanly)
        ));
    }

    #[test]
    fn accepts_a_well_formed_header() {
        assert!(Header::parse(&header_bytes(9)).is_ok());
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
