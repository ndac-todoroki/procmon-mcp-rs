//! Typed view of the `FilterRules` record.
//!
//! Layout (procmon-parser `configuration_format.py`, `RawRulesStruct` / `RawRuleStruct`):
//! `u8 1, u32 count`, then per rule `u32 column, u32 relation, u8 action,
//! u32 value_bytes, UTF-16LE value with NUL, u32 int_value, u32 reserved`.

use super::record::{PmcError, decode_utf16z, encode_utf16z};

/// Event column a rule tests. Kept as the raw number so unknown columns round-trip.
///
/// Values from procmon-parser `consts.py` (`Column`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Column(pub u32);

impl Column {
    pub const NONE: Column = Column(0);
    pub const DATE_AND_TIME: Column = Column(40052);
    pub const PROCESS_NAME: Column = Column(40053);
    pub const PID: Column = Column(40054);
    pub const OPERATION: Column = Column(40055);
    pub const RESULT: Column = Column(40056);
    pub const DETAIL: Column = Column(40057);
    pub const SEQUENCE: Column = Column(40058);
    pub const COMPANY: Column = Column(40064);
    pub const DESCRIPTION: Column = Column(40065);
    pub const COMMAND_LINE: Column = Column(40066);
    pub const USER: Column = Column(40067);
    pub const IMAGE_PATH: Column = Column(40068);
    pub const SESSION: Column = Column(40069);
    pub const PATH: Column = Column(40071);
    pub const TID: Column = Column(40072);
    pub const RELATIVE_TIME: Column = Column(40076);
    pub const DURATION: Column = Column(40077);
    pub const TIME_OF_DAY: Column = Column(40078);
    pub const VERSION: Column = Column(40081);
    pub const EVENT_CLASS: Column = Column(40082);
    pub const AUTHENTICATION_ID: Column = Column(40083);
    pub const VIRTUALIZED: Column = Column(40084);
    pub const INTEGRITY: Column = Column(40085);
    pub const CATEGORY: Column = Column(40086);
    pub const PARENT_PID: Column = Column(40087);
    pub const ARCHITECTURE: Column = Column(40088);
    pub const COMPLETION_TIME: Column = Column(40164);
    pub const PROCESS_START: Column = Column(40178);
}

/// How a rule compares the column value with its own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    Is = 0,
    IsNot = 1,
    LessThan = 2,
    MoreThan = 3,
    BeginsWith = 4,
    EndsWith = 5,
    Contains = 6,
    Excludes = 7,
}

/// What happens to an event that matches the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Exclude = 0,
    Include = 1,
}

/// A filter rule as shown in Procmon's filter dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRule {
    pub column: Column,
    pub relation: Relation,
    pub action: Action,
    pub value: String,
}

/// Decodes the data of a `FilterRules` record. Error offsets are relative to `data`.
pub fn decode_filter_rules(data: &[u8]) -> Result<Vec<FilterRule>, PmcError> {
    let mut reader = Reader { data, offset: 0 };
    if reader.u8()? != RULES_MARKER {
        return Err(PmcError::InvalidValue { offset: 0 });
    }
    let count = reader.u32()?;
    let mut rules = Vec::new();
    for _ in 0..count {
        let column = Column(reader.u32()?);
        let relation_offset = reader.offset;
        let relation = match reader.u32()? {
            0 => Relation::Is,
            1 => Relation::IsNot,
            2 => Relation::LessThan,
            3 => Relation::MoreThan,
            4 => Relation::BeginsWith,
            5 => Relation::EndsWith,
            6 => Relation::Contains,
            7 => Relation::Excludes,
            _ => {
                return Err(PmcError::InvalidValue {
                    offset: relation_offset,
                });
            }
        };
        let action_offset = reader.offset;
        let action = match reader.u8()? {
            0 => Action::Exclude,
            1 => Action::Include,
            _ => {
                return Err(PmcError::InvalidValue {
                    offset: action_offset,
                });
            }
        };
        let value_size = reader.u32()? as usize;
        let value_offset = reader.offset;
        let value = decode_utf16z(reader.bytes(value_size)?).ok_or(PmcError::InvalidString {
            offset: value_offset,
        })?;
        reader.u32()?;
        reader.u32()?;
        rules.push(FilterRule {
            column,
            relation,
            action,
            value,
        });
    }
    if reader.offset != data.len() {
        return Err(PmcError::InvalidSize {
            offset: reader.offset,
        });
    }
    Ok(rules)
}

/// Encodes rules as the data of a `FilterRules` record.
///
/// # Panics
/// If there are 2^32 or more rules or a value is 4 GiB or larger.
pub fn encode_filter_rules(rules: &[FilterRule]) -> Vec<u8> {
    let mut out = vec![RULES_MARKER];
    out.extend_from_slice(&size_u32(rules.len()).to_le_bytes());
    for rule in rules {
        let value = encode_utf16z(&rule.value);
        out.extend_from_slice(&rule.column.0.to_le_bytes());
        out.extend_from_slice(&(rule.relation as u32).to_le_bytes());
        out.push(rule.action as u8);
        out.extend_from_slice(&size_u32(value.len()).to_le_bytes());
        out.extend_from_slice(&value);
        out.extend_from_slice(&rule_int_value(rule.column, &rule.value).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    }
    out
}

/// The `int_value` field, following procmon-parser `get_rule_integer_value`.
///
/// Digit strings that overflow u32 give 0 instead of failing. procmon-parser would raise
/// there, but such a value cannot match any numeric column, so an error path in the
/// encoder is not worth it.
fn rule_int_value(column: Column, value: &str) -> u32 {
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse().unwrap_or(0);
    }
    if column == Column::ARCHITECTURE {
        return if value.contains("32") { 32 } else { 64 };
    }
    0
}

fn size_u32(size: usize) -> u32 {
    u32::try_from(size).expect("FilterRules data must be smaller than 4 GiB")
}

/// Constant first byte of the rule list (procmon-parser: `Const(1, Int8ul)`, meaning unknown).
const RULES_MARKER: u8 = 1;

/// Bounds-checked little-endian cursor.
struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], PmcError> {
        let slice = self
            .offset
            .checked_add(len)
            .and_then(|end| self.data.get(self.offset..end))
            .ok_or(PmcError::Truncated {
                offset: self.offset,
            })?;
        self.offset += len;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, PmcError> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, PmcError> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes one rule by hand, independently of the encoder under test.
    fn rule_bytes(column: u32, relation: u32, action: u8, value: &str, int_value: u32) -> Vec<u8> {
        let value: Vec<u8> = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut out = Vec::new();
        out.extend_from_slice(&column.to_le_bytes());
        out.extend_from_slice(&relation.to_le_bytes());
        out.push(action);
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(&value);
        out.extend_from_slice(&int_value.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    fn rule_list(rules: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![1];
        out.extend_from_slice(&(rules.len() as u32).to_le_bytes());
        for rule in rules {
            out.extend_from_slice(rule);
        }
        out
    }

    #[test]
    fn empty_rule_list_decodes_to_no_rules() {
        assert_eq!(decode_filter_rules(&[1, 0, 0, 0, 0]), Ok(vec![]));
    }

    #[test]
    fn rule_decodes_column_relation_action_and_value_without_nul() {
        let data = rule_list(&[rule_bytes(40053, 0, 0, "Procmon.exe", 0)]);

        assert_eq!(
            decode_filter_rules(&data),
            Ok(vec![FilterRule {
                column: Column::PROCESS_NAME,
                relation: Relation::Is,
                action: Action::Exclude,
                value: "Procmon.exe".to_string(),
            }])
        );
    }

    fn rule(column: Column, relation: Relation, action: Action, value: &str) -> FilterRule {
        FilterRule {
            column,
            relation,
            action,
            value: value.to_string(),
        }
    }

    #[test]
    fn encoding_writes_the_marker_count_and_each_rule() {
        let rules = [
            rule(
                Column::PROCESS_NAME,
                Relation::Is,
                Action::Exclude,
                "System",
            ),
            rule(Column::PATH, Relation::Contains, Action::Include, "$Extend"),
        ];

        assert_eq!(
            encode_filter_rules(&rules),
            rule_list(&[
                rule_bytes(40053, 0, 0, "System", 0),
                rule_bytes(40071, 6, 1, "$Extend", 0),
            ])
        );
    }

    #[test]
    fn int_value_is_the_number_when_the_value_is_all_ascii_digits() {
        let rules = [rule(Column::PID, Relation::Is, Action::Include, "1234")];

        assert_eq!(
            encode_filter_rules(&rules),
            rule_list(&[rule_bytes(40054, 0, 1, "1234", 1234)])
        );
    }

    #[test]
    fn int_value_is_zero_for_values_that_are_not_plain_u32_digits() {
        // "+5" parses as a Rust integer but is not all digits; the last one overflows u32.
        for value in ["", "+5", "12a", "4294967296"] {
            let rules = [rule(Column::PID, Relation::Is, Action::Include, value)];

            assert_eq!(
                encode_filter_rules(&rules),
                rule_list(&[rule_bytes(40054, 0, 1, value, 0)]),
                "{value:?}"
            );
        }
    }

    #[test]
    fn int_value_for_architecture_is_32_or_64() {
        for (value, expected) in [("32-bit", 32), ("64-bit", 64), ("x64", 64)] {
            let rules = [rule(
                Column::ARCHITECTURE,
                Relation::Is,
                Action::Include,
                value,
            )];

            assert_eq!(
                encode_filter_rules(&rules),
                rule_list(&[rule_bytes(40088, 0, 1, value, expected)]),
                "{value:?}"
            );
        }
    }

    #[test]
    fn unknown_column_round_trips() {
        let data = rule_list(&[rule_bytes(49999, 0, 1, "x", 0)]);

        let rules = decode_filter_rules(&data).unwrap();

        assert_eq!(rules[0].column, Column(49999));
        assert_eq!(encode_filter_rules(&rules), data);
    }

    #[test]
    fn leading_byte_other_than_1_is_invalid() {
        assert_eq!(
            decode_filter_rules(&[2, 0, 0, 0, 0]),
            Err(PmcError::InvalidValue { offset: 0 })
        );
    }

    #[test]
    fn unknown_relation_is_invalid() {
        let data = rule_list(&[rule_bytes(40053, 8, 0, "x", 0)]);

        assert_eq!(
            decode_filter_rules(&data),
            Err(PmcError::InvalidValue { offset: 5 + 4 })
        );
    }

    #[test]
    fn unknown_action_is_invalid() {
        let data = rule_list(&[rule_bytes(40053, 0, 2, "x", 0)]);

        assert_eq!(
            decode_filter_rules(&data),
            Err(PmcError::InvalidValue { offset: 5 + 8 })
        );
    }

    #[test]
    fn value_without_nul_terminator_is_invalid() {
        let mut rule = rule_bytes(40053, 0, 0, "x", 0);
        // Shrink the declared size by one UTF-16 unit so the NUL falls outside the value.
        rule[9..13].copy_from_slice(&2u32.to_le_bytes());
        rule.drain(15..17);
        let data = rule_list(&[rule]);

        assert_eq!(
            decode_filter_rules(&data),
            Err(PmcError::InvalidString { offset: 5 + 13 })
        );
    }

    #[test]
    fn count_larger_than_the_rules_present_is_truncated() {
        let mut data = rule_list(&[rule_bytes(40053, 0, 0, "x", 0)]);
        data[1] = 2;

        assert_eq!(
            decode_filter_rules(&data),
            Err(PmcError::Truncated { offset: data.len() })
        );
    }

    #[test]
    fn bytes_after_the_last_rule_are_invalid() {
        let mut data = rule_list(&[rule_bytes(40053, 0, 0, "x", 0)]);
        let end = data.len();
        data.push(0);

        assert_eq!(
            decode_filter_rules(&data),
            Err(PmcError::InvalidSize { offset: end })
        );
    }
}
