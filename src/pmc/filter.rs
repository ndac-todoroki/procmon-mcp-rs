//! Typed view of the `FilterRules` record.
//!
//! Layout (procmon-parser `configuration_format.py`, `RawRulesStruct` / `RawRuleStruct`):
//! `u8 1, u32 count`, then per rule `u32 column, u32 relation, u8 action,
//! u32 value_bytes, UTF-16LE value with NUL, u32 int_value, u32 reserved`.

use super::record::{PmcError, decode_utf16z};

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
    reader.u8()?;
    let count = reader.u32()?;
    let mut rules = Vec::new();
    for _ in 0..count {
        let column = Column(reader.u32()?);
        let relation = match reader.u32()? {
            0 => Relation::Is,
            1 => Relation::IsNot,
            2 => Relation::LessThan,
            3 => Relation::MoreThan,
            4 => Relation::BeginsWith,
            5 => Relation::EndsWith,
            6 => Relation::Contains,
            _ => Relation::Excludes,
        };
        let action = match reader.u8()? {
            0 => Action::Exclude,
            _ => Action::Include,
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
    Ok(rules)
}

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
}
