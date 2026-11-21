//! Safe wrappers over the Windows APIs this tool needs. All `unsafe` code lives here.

mod signature;
mod system;

pub use signature::{SignatureError, verify_microsoft_signature};
pub use system::{is_elevated, program_data_dir, system_dir};
