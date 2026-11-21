//! Streaming reader for Process Monitor log files (`.pml`).
//!
//! The format is not officially documented. The layout follows the reverse-engineered
//! documentation of procmon-parser (see THIRD_PARTY_NOTICES.md).

mod bytes;
mod details;
mod error;
mod event;
mod header;
mod reader;
mod tables;

pub use error::PmlError;
pub use event::{Detail, Event, EventClass};
pub use header::Header;
pub use reader::PmlReader;
pub use tables::Process;
