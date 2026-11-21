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
    /// The size fields of the record at `offset` are inconsistent.
    InvalidSize { offset: usize },
}

impl fmt::Display for PmcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PmcError::Truncated { offset } => {
                write!(f, "PMC data ends inside the structure at offset {offset}")
            }
            PmcError::InvalidSize { offset } => {
                write!(
                    f,
                    "inconsistent size fields in the PMC record at offset {offset}"
                )
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
        let (record, record_size) = parse_record(&bytes[offset..], offset)?;
        records.push(record);
        offset += record_size;
    }
    Ok(Configuration { records })
}

/// Parses the record at the start of `rest`; `offset` is only used for error reporting.
fn parse_record(rest: &[u8], offset: usize) -> Result<(Record, usize), PmcError> {
    let truncated = PmcError::Truncated { offset };
    let header = rest.get(..HEADER_SIZE).ok_or(truncated.clone())?;
    let field = |index: usize| {
        let at = index * 4;
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]) as usize
    };
    let record_size = field(0);
    let header_size = field(1);
    let header_and_name_size = field(2);
    let data_size = field(3);
    if header_size != HEADER_SIZE
        || header_and_name_size < HEADER_SIZE
        || header_and_name_size.checked_add(data_size) != Some(record_size)
    {
        return Err(PmcError::InvalidSize { offset });
    }

    let name_bytes = rest
        .get(HEADER_SIZE..header_and_name_size)
        .ok_or(truncated.clone())?;
    let data_end = header_and_name_size
        .checked_add(data_size)
        .ok_or(truncated.clone())?;
    let data = rest
        .get(header_and_name_size..data_end)
        .ok_or(truncated)?
        .to_vec();

    let name_units: Vec<u16> = name_bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let name = String::from_utf16(&name_units[..name_units.len() - 1]).unwrap();
    Ok((Record { name, data }, record_size))
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

    #[test]
    fn record_whose_data_runs_past_the_end_is_truncated() {
        let mut bytes = record_bytes("Profiling", &[1, 0, 0, 0]);
        bytes.truncate(bytes.len() - 2);

        assert_eq!(parse(&bytes), Err(PmcError::Truncated { offset: 0 }));
    }

    fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn header_size_other_than_0x10_is_invalid() {
        let mut bytes = record_bytes("Profiling", &[0, 0, 0, 0]);
        set_u32(&mut bytes, 4, 0x14);

        assert_eq!(parse(&bytes), Err(PmcError::InvalidSize { offset: 0 }));
    }

    #[test]
    fn record_size_that_disagrees_with_name_and_data_sizes_is_invalid() {
        let mut bytes = record_bytes("Profiling", &[0, 0, 0, 0]);
        bytes.extend(record_bytes("Logfile", &[]));
        // Pointing inside the record would desynchronize the parser; zero would loop forever.
        for bad_size in [0, 4, 0x24 + 2] {
            set_u32(&mut bytes, 0, bad_size);

            assert_eq!(parse(&bytes), Err(PmcError::InvalidSize { offset: 0 }));
        }
    }

    #[test]
    fn header_and_name_size_smaller_than_the_header_is_invalid() {
        let mut bytes = record_bytes("Profiling", &[]);
        set_u32(&mut bytes, 8, 0x0e);

        assert_eq!(parse(&bytes), Err(PmcError::InvalidSize { offset: 0 }));
    }

    #[test]
    fn record_whose_name_runs_past_the_end_is_truncated() {
        let mut bytes = record_bytes("Profiling", &[]);
        bytes.truncate(HEADER_SIZE + 4);

        assert_eq!(parse(&bytes), Err(PmcError::Truncated { offset: 0 }));
    }
}
