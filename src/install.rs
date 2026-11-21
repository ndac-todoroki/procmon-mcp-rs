//! `install`, `update` and `uninstall`: set up and remove everything under
//! `C:\ProgramData\ProcmonMcp` plus the scheduled task. All of them run elevated.

use std::io::{BufRead, Write};
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use serde_json::{Value, json};

use crate::download::{self, Endpoint, Fetch};
use crate::layout::{self, Layout};
use crate::task::{self, Action};
use crate::win;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub struct InstallOptions {
    /// The user accepted the Sysinternals license on the command line.
    pub accept_eula: bool,
}

/// Sets up everything: directories with their ACLs, this executable, Procmon, the task.
/// Running it again repairs or refreshes an existing installation.
pub fn install(options: &InstallOptions) -> Result<(), String> {
    require_elevated()?;
    if !options.accept_eula && !confirm_eula(std::io::stdin().lock(), std::io::stderr()) {
        return Err("the Sysinternals license terms were not accepted".to_owned());
    }
    let layout = Layout::system().map_err(|e| format!("cannot locate ProgramData: {e}"))?;
    let user = win::current_user_sid().map_err(|e| format!("cannot identify the user: {e}"))?;

    prepare_root(&layout)?;
    for (dir, sddl) in [
        (layout.bin(), layout::bin_sddl()),
        (layout.jobs(), layout::jobs_sddl(&user)),
        (layout.out(), layout::out_sddl(&user)),
    ] {
        ensure_plain_dir(&dir)?;
        win::set_security(&dir, &sddl)
            .map_err(|e| format!("cannot set permissions on {}: {e}", dir.display()))?;
    }
    copy_self(&layout)?;
    match update_procmon(&layout)? {
        UpdateOutcome::Updated => {
            eprintln!("Downloaded and verified {}", layout.procmon().display())
        }
        UpdateOutcome::UpToDate => eprintln!("{} is up to date", layout.procmon().display()),
    }
    register_task(&layout, &user)?;

    eprintln!(
        "\nInstalled. Add this to mcpServers in Claude Desktop's claude_desktop_config.json:\n\n{}",
        claude_desktop_entry(&layout.server())
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated,
    UpToDate,
}

/// Fetches Procmon again only if Sysinternals has a newer one than the installed copy.
pub fn update() -> Result<UpdateOutcome, String> {
    require_elevated()?;
    let layout = Layout::system().map_err(|e| format!("cannot locate ProgramData: {e}"))?;
    crate::runner::check_security(&layout.bin(), &layout::bin_sddl())?;
    update_procmon(&layout)
}

/// Removes the task and everything under ProgramData\ProcmonMcp.
pub fn uninstall() -> Result<(), String> {
    require_elevated()?;
    let layout = Layout::system().map_err(|e| format!("cannot locate ProgramData: {e}"))?;
    if task::schtasks(Action::Query).is_ok() {
        task::schtasks(Action::Delete).map_err(|e| e.to_string())?;
        eprintln!("Removed the scheduled task {}", task::TASK_NAME);
    }
    match std::fs::symlink_metadata(&layout.root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot inspect {}: {e}", layout.root.display())),
        Ok(_) => {
            check_ours(&layout.root)?;
            // remove_dir_all does not follow junctions or symbolic links.
            std::fs::remove_dir_all(&layout.root).map_err(|e| {
                format!(
                    "cannot remove {} (is Claude Desktop still running the server?): {e}",
                    layout.root.display()
                )
            })?;
            eprintln!("Removed {}", layout.root.display());
        }
    }
    eprintln!("Remove the \"procmon\" entry from Claude Desktop's configuration as well.");
    Ok(())
}

fn require_elevated() -> Result<(), String> {
    match win::is_elevated() {
        Ok(true) => Ok(()),
        Ok(false) => Err("run this from an elevated (administrator) terminal".to_owned()),
        Err(e) => Err(format!("cannot query elevation: {e}")),
    }
}

/// Creates the root, or accepts an existing one only if a previous install made it.
///
/// Anyone may create folders in ProgramData. A pre-created root could hold junctions
/// that redirect the elevated install's writes and ACL changes into system folders.
fn prepare_root(layout: &Layout) -> Result<(), String> {
    match std::fs::symlink_metadata(&layout.root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&layout.root)
            .map_err(|e| format!("cannot create {}: {e}", layout.root.display()))?,
        Err(e) => return Err(format!("cannot inspect {}: {e}", layout.root.display())),
        Ok(_) => check_ours(&layout.root)?,
    }
    win::set_security(&layout.root, &layout::root_sddl())
        .map_err(|e| format!("cannot set permissions on {}: {e}", layout.root.display()))
}

/// An existing root must be a plain directory owned by Administrators.
fn check_ours(root: &Path) -> Result<(), String> {
    ensure_plain_dir(root)?;
    let sddl = win::security_sddl(root)
        .map_err(|e| format!("cannot read the permissions of {}: {e}", root.display()))?;
    if sddl.starts_with("O:BA") {
        Ok(())
    } else {
        Err(format!(
            "{} exists but was not created by procmon-mcp install; inspect and remove it first",
            root.display()
        ))
    }
}

/// Creates `dir` if missing; an existing one must be a real directory, not a link.
fn ensure_plain_dir(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
        }
        Err(e) => Err(format!("cannot inspect {}: {e}", dir.display())),
        Ok(meta) if meta.is_dir() && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 => {
            Ok(())
        }
        Ok(_) => Err(format!(
            "{} is not a plain directory (a file or a link); remove it first",
            dir.display()
        )),
    }
}

fn copy_self(layout: &Layout) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| format!("cannot locate this executable: {e}"))?;
    let target = layout.server();
    if same_file(&me, &target) {
        return Ok(());
    }
    let tmp = target.with_extension("exe.new");
    std::fs::copy(&me, &tmp).map_err(|e| format!("cannot copy to {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!(
            "cannot replace {} (quit Claude Desktop if it is running the server): {e}",
            target.display()
        )
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn update_procmon(layout: &Layout) -> Result<UpdateOutcome, String> {
    let state = State::parse(&std::fs::read(layout.state()).unwrap_or_default());
    // Only trust the stored ETag while the installed copy still verifies.
    let installed_ok = win::verify_microsoft_signature(&layout.procmon()).is_ok();
    let etag = state.procmon_etag.as_deref().filter(|_| installed_ok);
    let path = procmon_download_path(std::env::consts::ARCH);
    let fetched = download::get(
        &Endpoint::https(SYSINTERNALS_LIVE),
        path,
        etag,
        MAX_PROCMON_BYTES,
    )
    .map_err(|e| format!("cannot download https://{SYSINTERNALS_LIVE}{path}: {e}"))?;
    let (body, etag) = match fetched {
        Fetch::NotModified => return Ok(UpdateOutcome::UpToDate),
        Fetch::Downloaded { body, etag } => (body, etag),
    };
    let tmp = layout.procmon().with_extension("exe.download");
    std::fs::write(&tmp, &body).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    if let Err(e) = win::verify_microsoft_signature(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "the downloaded Process Monitor failed verification: {e}"
        ));
    }
    std::fs::rename(&tmp, layout.procmon()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!(
            "cannot replace {} (is a capture running?): {e}",
            layout.procmon().display()
        )
    })?;
    let state = State { procmon_etag: etag };
    std::fs::write(layout.state(), state.to_json().to_string())
        .map_err(|e| format!("cannot write {}: {e}", layout.state().display()))?;
    Ok(UpdateOutcome::Updated)
}

fn register_task(layout: &Layout, user: &str) -> Result<(), String> {
    // Written into bin, where only administrators can write, so it cannot be swapped
    // between writing and schtasks reading it.
    let xml_file = layout.bin().join("task.xml");
    let xml = task::task_xml(&layout.server(), user);
    std::fs::write(&xml_file, task::encode_utf16_with_bom(&xml))
        .map_err(|e| format!("cannot write {}: {e}", xml_file.display()))?;
    let result = task::schtasks(Action::Register {
        xml_file: &xml_file,
    });
    let _ = std::fs::remove_file(&xml_file);
    result.map_err(|e| e.to_string())
}

/// The `mcpServers` entry that starts the installed server.
pub fn claude_desktop_entry(server: &Path) -> String {
    let entry = json!({"procmon": {"command": server, "args": ["mcp"]}});
    serde_json::to_string_pretty(&entry).expect("JSON values always serialize")
}

/// Process Monitor is fetched from here; nothing else is downloaded.
pub const SYSINTERNALS_LIVE: &str = "live.sysinternals.com";
pub const EULA_URL: &str = "https://learn.microsoft.com/sysinternals/license-terms";
/// Procmon64.exe is about 2 MB; refuse anything absurdly larger.
pub const MAX_PROCMON_BYTES: usize = 64 * 1024 * 1024;

/// The Sysinternals Live path of the Procmon build for an architecture
/// (`std::env::consts::ARCH`).
pub fn procmon_download_path(arch: &str) -> &'static str {
    match arch {
        "aarch64" => "/Procmon64a.exe",
        _ => "/Procmon64.exe",
    }
}

/// Asks whether the user accepts the Sysinternals license. Only an explicit yes counts.
pub fn confirm_eula(mut input: impl BufRead, mut output: impl Write) -> bool {
    let _ = write!(
        output,
        "procmon-mcp starts Process Monitor with /AcceptEula, which accepts the \
         Sysinternals Software License Terms on your behalf:\n  {EULA_URL}\n\
         Do you accept them? [y/N] "
    );
    let _ = output.flush();
    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// What install remembers between runs, stored in `bin\state.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// ETag of the Procmon64.exe currently installed, for conditional updates.
    pub procmon_etag: Option<String>,
}

impl State {
    pub fn to_json(&self) -> Value {
        json!({"procmon_etag": self.procmon_etag})
    }

    /// A missing or unreadable state is treated as empty: the next update downloads again.
    pub fn parse(bytes: &[u8]) -> State {
        let v: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
        State {
            procmon_etag: v["procmon_etag"].as_str().map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_the_arm64_build_on_arm64_and_the_x64_build_otherwise() {
        assert_eq!(procmon_download_path("x86_64"), "/Procmon64.exe");
        assert_eq!(procmon_download_path("aarch64"), "/Procmon64a.exe");
    }

    fn answer(input: &str) -> (bool, String) {
        let mut out = Vec::new();
        let ok = confirm_eula(input.as_bytes(), &mut out);
        (ok, String::from_utf8(out).unwrap())
    }

    #[test]
    fn eula_prompt_shows_the_license_url_and_accepts_only_yes() {
        let (ok, shown) = answer("y\n");
        assert!(ok);
        assert!(shown.contains(EULA_URL), "{shown}");
        for yes in ["Y\n", "yes\n", " YES \r\n"] {
            assert!(answer(yes).0, "{yes:?}");
        }
        for no in ["\n", "n\n", "no\n", "sure\n", ""] {
            assert!(!answer(no).0, "{no:?}");
        }
    }

    #[test]
    fn claude_desktop_entry_runs_the_installed_server_in_mcp_mode() {
        let entry =
            claude_desktop_entry(Path::new(r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe"));
        let v: Value = serde_json::from_str(&entry).unwrap();
        assert_eq!(
            v,
            json!({"procmon": {
                "command": r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe",
                "args": ["mcp"]
            }})
        );
    }

    #[test]
    fn state_round_trips_and_tolerates_garbage() {
        let state = State {
            procmon_etag: Some("\"5b2da45f646ddd1:0\"".into()),
        };
        assert_eq!(State::parse(state.to_json().to_string().as_bytes()), state);
        assert_eq!(State::parse(b"not json"), State::default());
        assert_eq!(State::parse(b"{}"), State::default());
    }
}
