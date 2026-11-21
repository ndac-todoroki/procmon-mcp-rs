//! Per-class detail structures: the Path column and the Detail fields we use.
//!
//! Layouts follow procmon-parser's stream_logs_detail_format.py. Pointer-sized fields use
//! the system bitness from the header.

use super::PmlError;
use super::bytes::ByteReader;
use super::event::{Detail, EventClass};

// Operation codes used below (procmon-parser consts.py).
const FS_READ_FILE: u16 = 23;
const FS_WRITE_FILE: u16 = 24;
const PROCESS_DEFINED: u16 = 0;
const PROCESS_CREATE: u16 = 1;
const LOAD_IMAGE: u16 = 5;

pub(crate) struct DetailInput<'a> {
    pub class: EventClass,
    pub operation: u16,
    pub is_64bit: bool,
    pub details: &'a [u8],
    /// The optional extra detail block (already stripped of its `u16` size prefix).
    pub extra: Option<&'a [u8]>,
}

/// Extracts the path and detail. A malformed detail block yields `None` / `Detail::None`
/// rather than an error, so one odd event does not abort a whole summary.
pub(crate) fn parse(input: &DetailInput<'_>) -> (Option<String>, Detail) {
    let parsed = match input.class {
        EventClass::FileSystem | EventClass::Ipc => filesystem(input),
        EventClass::Registry => registry(input).map(|p| (Some(p), Detail::None)),
        EventClass::Process => Ok((process_path(input), Detail::None)),
        _ => Ok((None, Detail::None)),
    };
    parsed.unwrap_or((None, Detail::None))
}

fn pointer_size(input: &DetailInput<'_>) -> usize {
    if input.is_64bit { 8 } else { 4 }
}

fn filesystem(input: &DetailInput<'_>) -> Result<(Option<String>, Detail), PmlError> {
    let p = pointer_size(input);
    // u8 sub-operation, 3 bytes padding, then a block of 5 pointers and 0x14 bytes.
    let block_start = 4;
    let block_len = p * 5 + 0x14;
    let mut d = ByteReader::new(input.details);
    d.seek(block_start + block_len)?;
    let info = d.u16()?;
    d.skip(2)?;
    let path = detail_string(&mut d, info)?;

    let detail = match input.operation {
        FS_READ_FILE | FS_WRITE_FILE => {
            let mut b = ByteReader::new(input.details);
            b.seek(block_start + 0x0C)?;
            let mut length = b.u32()?;
            // The offset sits after pointer-size-dependent padding.
            b.seek(block_start + if p == 8 { 0x1C } else { 0x14 })?;
            let offset = b.i64()?;
            // When present, the extra block holds the length actually transferred.
            if let Some(extra) = input.extra {
                length = ByteReader::new(extra).u32()?;
            }
            Detail::ReadWrite { offset, length }
        }
        _ => Detail::None,
    };
    Ok((Some(path), detail))
}

fn registry(input: &DetailInput<'_>) -> Result<String, PmlError> {
    let mut d = ByteReader::new(input.details);
    let info = d.u16()?;
    // Where the path starts depends on which fixed fields precede it.
    let path_start = match input.operation {
        0 | 1 => 0x08,     // RegOpenKey, RegCreateKey
        3 | 5 => 0x0C,     // RegQueryKey, RegQueryValue
        4 | 6..=8 => 0x10, // RegSetValue, RegEnumValue, RegEnumKey, RegSetInfoKey
        12 | 14 => 0x04,   // RegLoadKey, RegRenameKey
        _ => 0x02,
    };
    d.seek(path_start)?;
    detail_string(&mut d, info)
}

fn process_path(input: &DetailInput<'_>) -> Option<String> {
    let p = pointer_size(input);
    let mut d = ByteReader::new(input.details);
    let read = |d: &mut ByteReader<'_>| -> Result<String, PmlError> {
        match input.operation {
            PROCESS_DEFINED | PROCESS_CREATE => {
                d.seek(0x2C)?;
                let skip1 = usize::from(d.u8()?);
                let skip2 = usize::from(d.u8()?);
                let info = d.u16()?;
                d.seek(0x34 + skip1 + skip2)?;
                detail_string(d, info)
            }
            LOAD_IMAGE => {
                d.seek(4 + p)?;
                let info = d.u16()?;
                d.skip(2)?;
                detail_string(d, info)
            }
            _ => Err(PmlError::Truncated),
        }
    };
    read(&mut d).ok()
}

/// Strings in detail blocks are prefixed by a `u16`: bit 15 set means 8-bit ASCII,
/// the low 15 bits are the character count.
fn detail_string(d: &mut ByteReader<'_>, info: u16) -> Result<String, PmlError> {
    let is_ascii = info & 0x8000 != 0;
    let chars = usize::from(info & 0x7FFF);
    if is_ascii {
        Ok(d.bytes(chars)?.iter().map(|&b| char::from(b)).collect())
    } else {
        Ok(super::bytes::utf16_to_string(d.bytes(chars * 2)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs_details(is_64bit: bool, path: &str, ascii: bool) -> Vec<u8> {
        let p = if is_64bit { 8 } else { 4 };
        let mut b = vec![0u8; 4 + p * 5 + 0x14];
        let count = path.chars().count() as u16;
        b.extend((if ascii { count | 0x8000 } else { count }).to_le_bytes());
        b.extend([0, 0]);
        if ascii {
            b.extend(path.bytes());
        } else {
            b.extend(path.encode_utf16().flat_map(u16::to_le_bytes));
        }
        b
    }

    fn input(class: EventClass, operation: u16, is_64bit: bool, details: &[u8]) -> DetailInput<'_> {
        DetailInput {
            class,
            operation,
            is_64bit,
            details,
            extra: None,
        }
    }

    #[test]
    fn reads_unicode_and_ascii_file_paths_for_both_bitnesses() {
        for is_64bit in [false, true] {
            for ascii in [false, true] {
                let d = fs_details(is_64bit, r"C:\x\y.txt", ascii);
                let (path, _) = parse(&input(EventClass::FileSystem, 20, is_64bit, &d));
                assert_eq!(
                    path.as_deref(),
                    Some(r"C:\x\y.txt"),
                    "64bit={is_64bit} ascii={ascii}"
                );
            }
        }
    }

    #[test]
    fn read_length_comes_from_extra_block_when_present() {
        let mut d = fs_details(true, "f", false);
        d[4 + 0x0C..4 + 0x10].copy_from_slice(&4096u32.to_le_bytes());
        d[4 + 0x1C..4 + 0x24].copy_from_slice(&8192i64.to_le_bytes());
        let extra = 612u32.to_le_bytes();

        let mut inp = input(EventClass::FileSystem, FS_READ_FILE, true, &d);
        assert_eq!(
            parse(&inp).1,
            Detail::ReadWrite {
                offset: 8192,
                length: 4096
            }
        );
        inp.extra = Some(&extra);
        assert_eq!(
            parse(&inp).1,
            Detail::ReadWrite {
                offset: 8192,
                length: 612
            }
        );
    }

    #[test]
    fn malformed_details_yield_no_path_instead_of_an_error() {
        let (path, detail) = parse(&input(
            EventClass::FileSystem,
            FS_READ_FILE,
            true,
            &[0u8; 10],
        ));
        assert_eq!(path, None);
        assert_eq!(detail, Detail::None);
    }

    #[test]
    fn registry_path_position_depends_on_operation() {
        let path = r"HKLM\Software";
        let mut d = vec![0u8; 0x0C];
        d[0..2].copy_from_slice(&(path.len() as u16).to_le_bytes());
        d.extend(path.encode_utf16().flat_map(u16::to_le_bytes));
        // RegQueryValue has length and information class before the path.
        let (p, _) = parse(&input(EventClass::Registry, 5, true, &d));
        assert_eq!(p.as_deref(), Some(path));
    }
}
