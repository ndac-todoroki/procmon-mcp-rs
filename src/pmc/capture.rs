//! Building the configuration that a capture run loads with `/LoadConfig`.

use super::defaults::default_filter_rules;
use super::filter::{Action, Column, FilterRule, Relation, encode_filter_rules};
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
pub enum CaptureFilterError {
    /// The text in `field` is longer than [`MAX_TEXT_UNITS`] UTF-16 units.
    TooLong { field: &'static str },
    /// The text in `field` contains NUL or another control character.
    ControlCharacter { field: &'static str },
}

/// Longest accepted condition text, in UTF-16 units (Windows' `MAX_PATH`).
pub const MAX_TEXT_UNITS: usize = 260;

/// Builds the configuration for `filter`: the default filter plus its Include rules,
/// with filtered-out events dropped from the log.
///
/// Records other than `FilterRules` and `DestructiveFilter` mirror a Procmon 4.13 GUI
/// export with default settings, in the same order. If Procmon writes loaded settings
/// back to the user's profile, defaults are the least surprising values to leave there.
pub fn capture_configuration(filter: &CaptureFilter) -> Result<Configuration, CaptureFilterError> {
    validate(filter)?;
    let mut rules = default_filter_rules();
    rules.extend(include_rules(filter));
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

/// The configuration is consumed by an elevated Procmon, so only plain, bounded text
/// reaches it.
fn validate(filter: &CaptureFilter) -> Result<(), CaptureFilterError> {
    let texts = [
        ("process_name", filter.process_name.as_slice()),
        ("operations", filter.operations.as_slice()),
        ("path_contains", filter.path_contains.as_slice()),
        ("result", filter.result.as_slice()),
    ];
    for (field, values) in texts {
        for value in values {
            if value.encode_utf16().count() > MAX_TEXT_UNITS {
                return Err(CaptureFilterError::TooLong { field });
            }
            // char::is_control covers NUL, the rest of C0, DEL and C1.
            if value.chars().any(char::is_control) {
                return Err(CaptureFilterError::ControlCharacter { field });
            }
        }
    }
    Ok(())
}

fn include_rules(filter: &CaptureFilter) -> Vec<FilterRule> {
    let include = |column, relation, value: &str| FilterRule {
        column,
        relation,
        action: Action::Include,
        value: value.to_string(),
    };
    let mut rules = Vec::new();
    if let Some(name) = &filter.process_name {
        rules.push(include(Column::PROCESS_NAME, Relation::Is, name));
    }
    if let Some(pid) = filter.pid {
        rules.push(include(Column::PID, Relation::Is, &pid.to_string()));
    }
    for operation in &filter.operations {
        rules.push(include(Column::OPERATION, Relation::Is, operation));
    }
    if let Some(path) = &filter.path_contains {
        rules.push(include(Column::PATH, Relation::Contains, path));
    }
    if let Some(result) = &filter.result {
        rules.push(include(Column::RESULT, Relation::Is, result));
    }
    rules
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

    fn filter_rules(config: &Configuration) -> Vec<FilterRule> {
        decode_filter_rules(config.data("FilterRules").unwrap()).unwrap()
    }

    fn include(column: Column, relation: Relation, value: &str) -> FilterRule {
        FilterRule {
            column,
            relation,
            action: Action::Include,
            value: value.to_string(),
        }
    }

    #[test]
    fn each_condition_appends_an_include_rule_after_the_defaults() {
        let filter = CaptureFilter {
            process_name: Some("HuionTablet.exe".to_string()),
            pid: Some(1234),
            operations: vec!["ReadFile".to_string(), "CreateFile".to_string()],
            path_contains: Some("user.cfg".to_string()),
            result: Some("NAME NOT FOUND".to_string()),
        };

        let config = capture_configuration(&filter).unwrap();

        let mut expected = default_filter_rules();
        expected.extend([
            include(Column::PROCESS_NAME, Relation::Is, "HuionTablet.exe"),
            include(Column::PID, Relation::Is, "1234"),
            include(Column::OPERATION, Relation::Is, "ReadFile"),
            include(Column::OPERATION, Relation::Is, "CreateFile"),
            include(Column::PATH, Relation::Contains, "user.cfg"),
            include(Column::RESULT, Relation::Is, "NAME NOT FOUND"),
        ]);
        assert_eq!(filter_rules(&config), expected);
    }

    /// A filter with `value` placed in the field named `field`.
    fn filter_with(field: &str, value: &str) -> CaptureFilter {
        let value = value.to_string();
        match field {
            "process_name" => CaptureFilter {
                process_name: Some(value),
                ..CaptureFilter::default()
            },
            // Second position: every operation is checked, not just the first.
            "operations" => CaptureFilter {
                operations: vec!["ReadFile".to_string(), value],
                ..CaptureFilter::default()
            },
            "path_contains" => CaptureFilter {
                path_contains: Some(value),
                ..CaptureFilter::default()
            },
            "result" => CaptureFilter {
                result: Some(value),
                ..CaptureFilter::default()
            },
            _ => unreachable!("{field}"),
        }
    }

    const TEXT_FIELDS: [&str; 4] = ["process_name", "operations", "path_contains", "result"];

    #[test]
    fn text_up_to_260_utf16_units_is_accepted_and_longer_is_rejected() {
        // U+1D11E takes two UTF-16 units, so the limit is not a char or byte count.
        let at_limit = "\u{1d11e}".repeat(130);
        let over_limit = format!("{at_limit}a");
        for field in TEXT_FIELDS {
            assert!(capture_configuration(&filter_with(field, &at_limit)).is_ok());
            assert_eq!(
                capture_configuration(&filter_with(field, &over_limit)).err(),
                Some(CaptureFilterError::TooLong { field }),
            );
        }
    }

    #[test]
    fn text_with_nul_or_other_control_characters_is_rejected() {
        // NUL would end the value early in Procmon's C-string view of it.
        for bad in ["a\0b", "a\nb", "\t", "a\u{1b}", "a\u{7f}", "a\u{85}"] {
            for field in TEXT_FIELDS {
                assert_eq!(
                    capture_configuration(&filter_with(field, bad)).err(),
                    Some(CaptureFilterError::ControlCharacter { field }),
                    "{bad:?}"
                );
            }
        }
    }

    #[test]
    fn printable_non_ascii_text_is_accepted() {
        for field in TEXT_FIELDS {
            assert!(capture_configuration(&filter_with(field, "C:\\ユーザー\\ä ö")).is_ok());
        }
    }

    #[test]
    fn records_follow_the_order_of_a_gui_export() {
        let config = capture_configuration(&CaptureFilter::default()).unwrap();

        let names: Vec<&str> = config.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Columns",
                "ColumnCount",
                "ColumnMap",
                "DbgHelpPath",
                "Logfile",
                "HighlightFG",
                "HighlightBG",
                "Theme",
                "LogFont",
                "BoookmarkFont",
                "AdvancedMode",
                "Autoscroll",
                "HistoryDepth",
                "Profiling",
                "DestructiveFilter",
                "AlwaysOnTop",
                "ResolveAddresses",
                "SourcePath",
                "SymbolPath",
                "FilterRules",
                "HighlightRules",
                "FlightRecorder",
                "RingBufferSize",
                "RingBufferMin",
            ]
        );
    }

    #[test]
    fn generated_bytes_parse_back_to_the_same_configuration() {
        let config = capture_configuration(&CaptureFilter::default()).unwrap();

        assert_eq!(crate::pmc::parse(&config.to_bytes()), Ok(config));
    }
}
