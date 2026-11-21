//! Reading and writing Process Monitor configuration files (`.pmc`).
//!
//! The format is not officially documented. The layout follows the reverse-engineered
//! documentation of procmon-parser (see THIRD_PARTY_NOTICES.md).

mod filter;
mod record;

pub use filter::{FilterRule, decode_filter_rules};
pub use record::{Configuration, PmcError, Record, parse};
