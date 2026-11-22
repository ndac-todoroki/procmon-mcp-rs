//! Where installed files live, and who may touch them.
//!
//! ```text
//! C:\Program Files\procmon-mcp\   procmon-mcp.exe, Procmon64.exe, the setup, state
//!                                 admins write, users read+execute
//! C:\ProgramData\ProcmonMcp\      owner Administrators; users read
//!   jobs\request.json             written by the (non-elevated) MCP server; installing user may modify
//!   out\<id>\                     written by the elevated runner; installing user may read and delete
//! ```
//!
//! Programs go under Program Files, where application control such as AppLocker's
//! default rules allows them to run; requests and results are data, so ProgramData.
//!
//! The elevated runner never writes where the user can create files: a junction planted
//! in a user-writable directory would redirect its writes anywhere on the system.

use std::path::{Path, PathBuf};

pub const PROGRAMS_DIR_NAME: &str = "procmon-mcp";
pub const DATA_DIR_NAME: &str = "ProcmonMcp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The programs, under Program Files.
    pub programs: PathBuf,
    /// Requests and results, under ProgramData.
    pub data: PathBuf,
}

impl Layout {
    /// The layout under the given Program Files and ProgramData directories.
    pub fn new(program_files: &Path, program_data: &Path) -> Layout {
        Layout {
            programs: program_files.join(PROGRAMS_DIR_NAME),
            data: program_data.join(DATA_DIR_NAME),
        }
    }

    /// The layout on this machine, from the system's folder locations.
    pub fn system() -> std::io::Result<Layout> {
        Ok(Layout::new(
            &crate::win::program_files_dir()?,
            &crate::win::program_data_dir()?,
        ))
    }

    pub fn jobs(&self) -> PathBuf {
        self.data.join("jobs")
    }

    pub fn out(&self) -> PathBuf {
        self.data.join("out")
    }

    pub fn procmon(&self) -> PathBuf {
        self.programs.join("Procmon64.exe")
    }

    pub fn server(&self) -> PathBuf {
        self.programs.join("procmon-mcp.exe")
    }

    /// The graphical setup, kept to uninstall from Settings → Apps.
    pub fn setup(&self) -> PathBuf {
        self.programs.join("procmon-mcp-setup.exe")
    }

    /// Install state, such as the ETag of the downloaded Procmon.
    pub fn state(&self) -> PathBuf {
        self.programs.join("state.json")
    }

    pub fn request(&self) -> PathBuf {
        self.jobs().join("request.json")
    }

    /// Output directory of one capture. `id` must already be validated.
    pub fn job(&self, id: &str) -> PathBuf {
        self.out().join(id)
    }
}

// Access masks: FA = full; 0x1200a9 = read and execute; 0x1301bf = modify (read, write,
// execute, delete); 0x1300a9 = read and execute plus delete. OICI makes entries apply to
// everything created inside. P blocks inheritance from the parent.
const SYSTEM_AND_ADMINS: &str = "(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

pub fn programs_sddl() -> String {
    format!("O:BAD:PAI{SYSTEM_AND_ADMINS}(A;OICI;0x1200a9;;;BU)")
}

pub fn data_sddl() -> String {
    programs_sddl()
}

pub fn jobs_sddl(user_sid: &str) -> String {
    format!("O:BAD:PAI{SYSTEM_AND_ADMINS}(A;OICI;0x1301bf;;;{user_sid})")
}

/// The user may delete finished captures but never create entries, so nothing can be
/// planted where the elevated runner writes.
pub fn out_sddl(user_sid: &str) -> String {
    format!("O:BAD:PAI{SYSTEM_AND_ADMINS}(A;OICI;0x1300a9;;;{user_sid})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(Path::new(r"C:\Program Files"), Path::new(r"C:\ProgramData"))
    }

    #[test]
    fn programs_live_under_program_files() {
        let l = layout();
        assert_eq!(l.programs, Path::new(r"C:\Program Files\procmon-mcp"));
        assert_eq!(
            l.procmon(),
            Path::new(r"C:\Program Files\procmon-mcp\Procmon64.exe")
        );
        assert_eq!(
            l.server(),
            Path::new(r"C:\Program Files\procmon-mcp\procmon-mcp.exe")
        );
        assert_eq!(
            l.setup(),
            Path::new(r"C:\Program Files\procmon-mcp\procmon-mcp-setup.exe")
        );
        assert_eq!(
            l.state(),
            Path::new(r"C:\Program Files\procmon-mcp\state.json")
        );
    }

    #[test]
    fn requests_and_results_live_under_programdata() {
        let l = layout();
        assert_eq!(l.data, Path::new(r"C:\ProgramData\ProcmonMcp"));
        assert_eq!(
            l.request(),
            Path::new(r"C:\ProgramData\ProcmonMcp\jobs\request.json")
        );
        assert_eq!(
            l.job("0123456789abcdef0123456789abcdef"),
            Path::new(r"C:\ProgramData\ProcmonMcp\out\0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn admin_only_directories_give_users_read_and_execute() {
        for sddl in [data_sddl(), programs_sddl()] {
            assert_eq!(
                sddl,
                "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)"
            );
        }
    }

    #[test]
    fn jobs_lets_only_the_installing_user_modify() {
        assert_eq!(
            jobs_sddl("S-1-5-21-1-2-3-1001"),
            "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;S-1-5-21-1-2-3-1001)"
        );
    }

    #[test]
    fn out_lets_only_the_installing_user_read_and_delete() {
        assert_eq!(
            out_sddl("S-1-5-21-1-2-3-1001"),
            "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1300a9;;;S-1-5-21-1-2-3-1001)"
        );
    }

    /// The runner compares the SDDL it reads back with these strings, so they must already
    /// be in the form Windows renders.
    #[test]
    fn sddl_strings_survive_a_round_trip_through_windows() {
        use crate::win::{current_user_sid, security_sddl, set_security};
        let me = current_user_sid().unwrap();
        let dir = std::env::temp_dir().join(format!("procmon-mcp-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for sddl in [data_sddl(), programs_sddl(), jobs_sddl(&me), out_sddl(&me)] {
            // Setting the owner to Administrators needs elevation, so own it ourselves, and
            // keep full control so the next round and the cleanup may change it again.
            let mine =
                sddl.replacen("O:BA", &format!("O:{me}"), 1) + &format!("(A;OICI;FA;;;{me})");
            set_security(&dir, &mine).unwrap();
            assert_eq!(security_sddl(&dir).unwrap(), mine);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
