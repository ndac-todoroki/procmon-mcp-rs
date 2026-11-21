//! Safe wrappers over the Windows APIs this tool needs. All `unsafe` code lives here.

mod acl;
mod http;
mod signature;
mod system;

pub use acl::{dacl_sddl, set_dacl};
pub use http::{Endpoint, Fetch, get as http_get};
pub use signature::{SignatureError, verify_microsoft_signature};
pub use system::{current_user_sid, is_elevated, program_data_dir, system_dir};
