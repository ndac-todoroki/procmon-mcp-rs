//! Safe wrappers over the Windows APIs this tool needs. All `unsafe` code lives here.

mod acl;
mod console;
mod process;
mod registry;
mod signature;
mod system;

pub use acl::{canonical_sddl, security_sddl, set_security};
pub use console::enable_ansi_on_stderr;
pub use process::{find_procmon, running_image_names};
pub use registry::{Hive, KeySnapshot, RegValue, restore_key, snapshot_key};
pub use signature::{SignatureError, verify_microsoft_signature};
pub use system::{
    current_user_sid, delete_at_reboot, is_elevated, program_data_dir, program_files_dir,
    random_bytes, signed_in_user_sid, system_dir, windows_dir,
};
