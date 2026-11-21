//! Building the configuration that a capture run loads with `/LoadConfig`.

use super::defaults::default_filter_rules;
use super::filter::{Column, encode_filter_rules};
use super::record::{Configuration, Record};

/// Conditions of one capture. Unset conditions do not narrow the capture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureFilter {
    pub process_name: Option<String>,
    pub pid: Option<u32>,
    pub operations: Vec<String>,
    pub path_contains: Option<String>,
    pub result: Option<String>,
}

/// Why a [`CaptureFilter`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureFilterError {}

/// Builds the configuration for `filter`: the default filter plus its Include rules,
/// with filtered-out events dropped from the log.
///
/// Records other than `FilterRules` and `DestructiveFilter` mirror a Procmon 4.13 GUI
/// export with default settings, in the same order. If Procmon writes loaded settings
/// back to the user's profile, defaults are the least surprising values to leave there.
pub fn capture_configuration(_filter: &CaptureFilter) -> Result<Configuration, CaptureFilterError> {
    let rules = default_filter_rules();
    let records = vec![
        record("Columns", u16_array(&DEFAULT_COLUMN_WIDTHS, 64)),
        u32_record("ColumnCount", DEFAULT_COLUMNS.len() as u32),
        record("ColumnMap", u32_array(&DEFAULT_COLUMNS.map(|c| c.0), 64)),
        // Absolute path in an admin-only directory: an empty value would leave the
        // DLL lookup of the elevated Procmon to behavior we have not verified.
        record("DbgHelpPath", utf16(r"C:\WINDOWS\SYSTEM32\dbghelp.dll")),
        record("Logfile", Vec::new()),
        u32_record("HighlightFG", 0),
        u32_record("HighlightBG", 0x00ff_ff80),
        record("Theme", Vec::new()),
        record("LogFont", log_font(FW_NORMAL)),
        // "Boookmark" is how Procmon spells it.
        record("BoookmarkFont", log_font(FW_BOLD)),
        u32_record("AdvancedMode", 0),
        u32_record("Autoscroll", 0),
        u32_record("HistoryDepth", 18),
        u32_record("Profiling", 0),
        u32_record("DestructiveFilter", 1),
        u32_record("AlwaysOnTop", 0),
        u32_record("ResolveAddresses", 1),
        record("SourcePath", Vec::new()),
        record(
            "SymbolPath",
            utf16("srv*https://msdl.microsoft.com/download/symbols"),
        ),
        record("FilterRules", encode_filter_rules(&rules)),
        record("HighlightRules", encode_filter_rules(&[])),
        u32_record("FlightRecorder", 0),
        u32_record("RingBufferSize", 0),
        u32_record("RingBufferMin", 0),
    ];
    Ok(Configuration { records })
}

/// Procmon's default columns: Time of Day, Process Name, PID, Operation, Path, Result, Detail.
const DEFAULT_COLUMNS: [Column; 7] = [
    Column::TIME_OF_DAY,
    Column::PROCESS_NAME,
    Column::PID,
    Column::OPERATION,
    Column::PATH,
    Column::RESULT,
    Column::DETAIL,
];
const DEFAULT_COLUMN_WIDTHS: [u16; 7] = [50, 143, 65, 100, 412, 161, 1404];

const FW_NORMAL: i32 = 400;
const FW_BOLD: i32 = 700;

/// A `LOGFONTW` for the Yu Gothic UI font found in the reference export.
fn log_font(weight: i32) -> Vec<u8> {
    const SHIFTJIS_CHARSET: u8 = 0x80;
    const OUT_STROKE_PRECIS: u8 = 3;
    const CLIP_STROKE_PRECIS: u8 = 2;
    const DRAFT_QUALITY: u8 = 1;
    const VARIABLE_PITCH_FF_MODERN: u8 = 0x32;
    let mut out = Vec::with_capacity(92);
    for field in [12, 0, 0, 0, weight] {
        out.extend_from_slice(&i32::to_le_bytes(field));
    }
    out.extend_from_slice(&[
        0, // italic
        0, // underline
        0, // strike out
        SHIFTJIS_CHARSET,
        OUT_STROKE_PRECIS,
        CLIP_STROKE_PRECIS,
        DRAFT_QUALITY,
        VARIABLE_PITCH_FF_MODERN,
    ]);
    let face: Vec<u16> = "Yu Gothic UI".encode_utf16().collect();
    out.extend(u16_array(&face, 32));
    out
}

fn record(name: &str, data: Vec<u8>) -> Record {
    Record {
        name: name.to_string(),
        data,
    }
}

fn u32_record(name: &str, value: u32) -> Record {
    record(name, value.to_le_bytes().to_vec())
}

/// UTF-16LE without a terminator: path records are stored that way in the reference.
fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// `values` padded with zeros to `len` entries.
fn u16_array(values: &[u16], len: usize) -> Vec<u8> {
    let mut out: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    out.resize(len * 2, 0);
    out
}

/// `values` padded with zeros to `len` entries.
fn u32_array(values: &[u32], len: usize) -> Vec<u8> {
    let mut out: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    out.resize(len * 4, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmc::decode_filter_rules;

    fn u32_value(config: &Configuration, name: &str) -> u32 {
        let data = config
            .data(name)
            .unwrap_or_else(|| panic!("no {name} record"));
        u32::from_le_bytes(data.try_into().unwrap())
    }

    #[test]
    fn empty_filter_uses_the_default_rules_and_drops_filtered_events() {
        let config = capture_configuration(&CaptureFilter::default()).unwrap();

        let rules = decode_filter_rules(config.data("FilterRules").unwrap()).unwrap();
        assert_eq!(rules, default_filter_rules());
        assert_eq!(u32_value(&config, "DestructiveFilter"), 1);
        assert_eq!(u32_value(&config, "Profiling"), 0);
    }
}
