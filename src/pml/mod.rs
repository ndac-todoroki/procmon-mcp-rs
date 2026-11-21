//! Streaming reader for Process Monitor log files (`.pml`).
//!
//! The format is not officially documented. The layout follows the reverse-engineered
//! documentation of procmon-parser (see THIRD_PARTY_NOTICES.md).

mod bytes;
mod error;
mod header;
mod tables;

pub use error::PmlError;
pub use header::Header;
