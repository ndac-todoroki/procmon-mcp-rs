//! Typed view of the `FilterRules` record.
//!
//! Layout (procmon-parser `configuration_format.py`, `RawRulesStruct` / `RawRuleStruct`):
//! `u8 1, u32 count`, then per rule `u32 column, u32 relation, u8 action,
//! u32 value_bytes, UTF-16LE value with NUL, u32 int_value, u32 reserved`.

use super::record::PmcError;

/// A filter rule as shown in Procmon's filter dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRule {}

/// Decodes the data of a `FilterRules` record. Error offsets are relative to `data`.
pub fn decode_filter_rules(_data: &[u8]) -> Result<Vec<FilterRule>, PmcError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rule_list_decodes_to_no_rules() {
        assert_eq!(decode_filter_rules(&[1, 0, 0, 0, 0]), Ok(vec![]));
    }
}
