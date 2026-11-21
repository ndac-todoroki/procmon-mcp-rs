//! Safe wrappers over the Windows APIs this tool needs. All `unsafe` code lives here.

mod signature;

pub use signature::{SignatureError, verify_microsoft_signature};
