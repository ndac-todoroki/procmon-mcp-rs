//! Generic record layer of a PMC file.
//!
//! Layout (procmon-parser `docs/PMC Format.md`, `RawRecordStruct`): a flat sequence of
//! records, each `u32 record_size, u32 0x10, u32 0x10 + name_bytes, u32 data_size`,
//! a NUL-terminated UTF-16LE name, then `data_size` bytes. All little-endian.

use std::fmt;

/// One named record. `data` is kept as raw bytes so unknown records round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Record name without the NUL terminator.
    pub name: String,
    /// Raw record data.
    pub data: Vec<u8>,
}

/// A whole configuration file: records in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configuration {
    pub records: Vec<Record>,
}

/// Error returned when PMC bytes are malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PmcError {}

impl fmt::Display for PmcError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for PmcError {}

/// Parses a whole PMC file.
pub fn parse(_bytes: &[u8]) -> Result<Configuration, PmcError> {
    Ok(Configuration::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_a_configuration_without_records() {
        assert_eq!(parse(&[]), Ok(Configuration::default()));
    }
}
