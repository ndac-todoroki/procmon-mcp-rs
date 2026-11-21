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
pub enum PmcError {
    /// The input ends inside the structure that starts at `offset`.
    Truncated { offset: usize },
}

impl fmt::Display for PmcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PmcError::Truncated { offset } => {
                write!(f, "PMC data ends inside the structure at offset {offset}")
            }
        }
    }
}

impl std::error::Error for PmcError {}

impl Configuration {
    /// Serializes the records in order.
    ///
    /// # Panics
    /// If a record is 4 GiB or larger, which the format cannot express.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for record in &self.records {
            let mut name: Vec<u8> = record
                .name
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            let header_and_name_size = size_u32(HEADER_SIZE + name.len());
            let data_size = size_u32(record.data.len());
            let record_size = header_and_name_size
                .checked_add(data_size)
                .expect("PMC record must be smaller than 4 GiB");
            out.extend_from_slice(&record_size.to_le_bytes());
            out.extend_from_slice(&size_u32(HEADER_SIZE).to_le_bytes());
            out.extend_from_slice(&header_and_name_size.to_le_bytes());
            out.extend_from_slice(&data_size.to_le_bytes());
            out.append(&mut name);
            out.extend_from_slice(&record.data);
        }
        out
    }
}

fn size_u32(size: usize) -> u32 {
    u32::try_from(size).expect("PMC record must be smaller than 4 GiB")
}

/// Parses a whole PMC file.
pub fn parse(bytes: &[u8]) -> Result<Configuration, PmcError> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes.len() - offset < HEADER_SIZE {
            return Err(PmcError::Truncated { offset });
        }
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let record_size = u32_at(offset) as usize;
        let header_and_name_size = u32_at(offset + 8) as usize;
        let data_size = u32_at(offset + 12) as usize;
        let name_units: Vec<u16> = bytes[offset + HEADER_SIZE..offset + header_and_name_size]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let name = String::from_utf16(&name_units[..name_units.len() - 1]).unwrap();
        let data_start = offset + header_and_name_size;
        let data = bytes[data_start..data_start + data_size].to_vec();
        records.push(Record { name, data });
        offset += record_size;
    }
    Ok(Configuration { records })
}

const HEADER_SIZE: usize = 0x10;

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one record by hand, independently of `to_bytes`.
    fn record_bytes(name: &str, data: &[u8]) -> Vec<u8> {
        let mut name_bytes: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        name_bytes.extend_from_slice(&[0, 0]);
        let header_and_name = 0x10 + name_bytes.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(&(header_and_name + data.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x10u32.to_le_bytes());
        out.extend_from_slice(&header_and_name.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&name_bytes);
        out.extend_from_slice(data);
        out
    }

    fn record(name: &str, data: &[u8]) -> Record {
        Record {
            name: name.to_string(),
            data: data.to_vec(),
        }
    }

    #[test]
    fn empty_input_is_a_configuration_without_records() {
        assert_eq!(parse(&[]), Ok(Configuration::default()));
    }

    #[test]
    fn single_record_yields_its_name_without_nul_and_raw_data() {
        let bytes = record_bytes("Profiling", &[1, 0, 0, 0]);

        let config = parse(&bytes).unwrap();

        assert_eq!(config.records, vec![record("Profiling", &[1, 0, 0, 0])]);
    }

    #[test]
    fn records_keep_file_order_including_empty_data() {
        let mut bytes = record_bytes("Logfile", &[]);
        bytes.extend(record_bytes("Columns", &[0x32, 0]));

        let config = parse(&bytes).unwrap();

        assert_eq!(
            config.records,
            vec![record("Logfile", &[]), record("Columns", &[0x32, 0])]
        );
    }

    #[test]
    fn to_bytes_writes_records_back_in_the_same_layout() {
        let mut bytes = record_bytes("Logfile", &[]);
        bytes.extend(record_bytes("Profiling", &[1, 0, 0, 0]));
        let config = Configuration {
            records: vec![record("Logfile", &[]), record("Profiling", &[1, 0, 0, 0])],
        };

        assert_eq!(config.to_bytes(), bytes);
    }

    #[test]
    fn input_ending_inside_a_record_header_is_truncated() {
        let mut bytes = record_bytes("Logfile", &[]);
        let second_record = bytes.len();
        bytes.extend_from_slice(&[0x20, 0, 0, 0, 0x10, 0, 0, 0]);

        assert_eq!(
            parse(&bytes),
            Err(PmcError::Truncated {
                offset: second_record
            })
        );
    }
}
