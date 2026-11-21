//! Reading and writing Process Monitor configuration files (`.pmc`).
//!
//! The format is not officially documented. The layout follows the reverse-engineered
//! documentation of procmon-parser (see THIRD_PARTY_NOTICES.md).

mod capture;
mod defaults;
mod eval;
mod filter;
mod record;

pub use capture::{
    CaptureFilter, CaptureFilterError, MAX_OPERATIONS, MAX_TEXT_UNITS, capture_configuration,
    set_dbghelp_path,
};
pub use defaults::default_filter_rules;
pub use eval::{FilterSubject, is_included};
pub use filter::{Action, Column, FilterRule, Relation, decode_filter_rules, encode_filter_rules};
pub use record::{Configuration, PmcError, Record, parse};
