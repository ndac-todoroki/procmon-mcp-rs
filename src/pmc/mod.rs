//! Reading and writing Process Monitor configuration files (`.pmc`).
//!
//! The format is not officially documented. The layout follows the reverse-engineered
//! documentation of procmon-parser (see THIRD_PARTY_NOTICES.md).

mod record;

pub use record::{Configuration, PmcError, Record, parse};
