//! The string and process tables that events refer to by index.

use std::io::{Read, Seek};

use super::PmlError;
use super::bytes::{read_u32, read_vec, seek_to, utf16_to_string};

/// Reads the string table: `u32 count`, `count` offsets relative to the table start,
/// then at each offset `u32 byte_len` followed by UTF-16LE text.
pub(crate) fn read_strings<R: Read + Seek>(
    r: &mut R,
    table_offset: u64,
) -> Result<Vec<String>, PmlError> {
    seek_to(r, table_offset)?;
    let count = read_u32(r)?;
    let offsets = read_vec(r, u64::from(count) * 4)?;
    let mut strings = Vec::with_capacity(count as usize);
    for off in offsets.chunks_exact(4) {
        let off = u32::from_le_bytes(off.try_into().expect("chunk of 4"));
        seek_to(r, table_offset + u64::from(off))?;
        let len = read_u32(r)?;
        strings.push(utf16_to_string(&read_vec(r, u64::from(len))?));
    }
    Ok(strings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_strings_by_index_from_offsets_relative_to_table_start() {
        // 8 bytes of unrelated data, then a table of two strings stored in reverse order.
        let mut b = vec![0xEEu8; 8];
        let table = b.len() as u64;
        let first = utf16("C:\\a");
        let second = utf16("bc");
        b.extend(2u32.to_le_bytes());
        let off_second = 4 + 8;
        let off_first = off_second + 4 + second.len() as u32;
        b.extend(off_first.to_le_bytes());
        b.extend(off_second.to_le_bytes());
        b.extend((second.len() as u32).to_le_bytes());
        b.extend(&second);
        b.extend((first.len() as u32).to_le_bytes());
        b.extend(&first);

        let strings = read_strings(&mut Cursor::new(b), table).unwrap();
        assert_eq!(strings, ["C:\\a", "bc"]);
    }

    #[test]
    fn reports_truncation_when_a_string_runs_past_the_end() {
        let mut b = Vec::new();
        b.extend(1u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(100u32.to_le_bytes());
        b.extend(utf16("ab"));
        assert!(matches!(
            read_strings(&mut Cursor::new(b), 0),
            Err(PmlError::Truncated)
        ));
    }
}
