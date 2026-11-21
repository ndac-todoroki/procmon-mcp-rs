//! Safe wrappers over the Windows APIs this tool needs. All `unsafe` code lives here.

mod acl;
mod process;
mod signature;
mod system;

pub use acl::{security_sddl, set_security};
pub use process::{find_procmon, running_image_names};
pub use signature::{SignatureError, verify_microsoft_signature};
pub use system::{current_user_sid, is_elevated, program_data_dir, random_bytes, system_dir};
