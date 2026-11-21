//! `install`, `update` and `uninstall`: set up and remove everything under
//! `C:\ProgramData\ProcmonMcp` plus the scheduled task. All of them run elevated.

use std::io::{BufRead, Write};
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use serde_json::{Value, json};

use crate::download::{self, Endpoint, Fetch};
use crate::layout::{self, Layout};
use crate::task;
use crate::term::{Color, Style};
use crate::win;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub struct InstallOptions {
    /// The user accepted the Sysinternals license on the command line.
    pub accept_eula: bool,
}

/// The steps of an install, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The root, `bin`, `jobs` and `out` with their ACLs.
    Folders,
    /// procmon-mcp.exe, plus the setup when the setup installs.
    Programs,
    /// Download Process Monitor and verify its signature.
    Procmon,
    /// The scheduled task, plus the Installed apps entry when the setup is kept.
    Register,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Folders, Step::Programs, Step::Procmon, Step::Register];
}

/// Where the programs placed in `bin` come from.
pub enum Programs<'a> {
    /// `procmon-mcp install`: this executable is the server.
    ThisExecutable,
    /// The graphical setup: the server it carries, and the setup itself, kept to
    /// uninstall from Settings.
    Setup { server: &'a [u8] },
}

/// An install that passed its checks: elevated, by the signed-in user.
pub struct Installer {
    layout: Layout,
    user: String,
}

impl Installer {
    /// Checks that installing may go ahead, before anything changes.
    pub fn prepare() -> Result<Installer, String> {
        require_elevated()?;
        let layout = Layout::system().map_err(|e| format!("cannot locate ProgramData: {e}"))?;
        let user = win::current_user_sid().map_err(|e| format!("cannot identify the user: {e}"))?;
        let signed_in = win::signed_in_user_sid()
            .map_err(|e| format!("cannot identify the signed-in user: {e}"))?;
        ensure_signed_in_user(&user, &signed_in)?;
        Ok(Installer { layout, user })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Runs one step. Running all of them again repairs or refreshes an installation.
    pub fn run(&self, step: Step, programs: &Programs) -> Result<StepOutcome, String> {
        let layout = &self.layout;
        match step {
            Step::Folders => {
                prepare_root(layout)?;
                for (dir, sddl) in [
                    (layout.bin(), layout::bin_sddl()),
                    (layout.jobs(), layout::jobs_sddl(&self.user)),
                    (layout.out(), layout::out_sddl(&self.user)),
                ] {
                    ensure_plain_dir(&dir)?;
                    win::set_security(&dir, &sddl)
                        .map_err(|e| format!("cannot set permissions on {}: {e}", dir.display()))?;
                }
            }
            Step::Programs => match programs {
                Programs::ThisExecutable => copy_self(layout)?,
                Programs::Setup { server } => {
                    write_program(&layout.server(), server)?;
                    copy_running_to(&layout.setup())?;
                }
            },
            Step::Procmon => {
                return update_procmon(layout).map(|outcome| match outcome {
                    UpdateOutcome::Updated => StepOutcome::ProcmonUpdated,
                    UpdateOutcome::UpToDate => StepOutcome::ProcmonUpToDate,
                });
            }
            Step::Register => {
                register_task(layout, &self.user)?;
                // A CLI repair of a setup install keeps the entry up to date.
                if layout.setup().is_file() {
                    let entry = win::KeySnapshot {
                        values: Some(uninstall_entry(layout, installed_size_kib(&layout.bin()))),
                    };
                    win::restore_key(win::Hive::LocalMachine, UNINSTALL_KEY, &entry)
                        .map_err(|e| format!("cannot add procmon-mcp to Installed apps: {e}"))?;
                }
            }
        }
        Ok(StepOutcome::Done)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Done,
    ProcmonUpdated,
    ProcmonUpToDate,
}

/// `procmon-mcp install`: shows what will change, asks to accept the license, then
/// runs every step, reporting each on stderr.
pub fn install(options: &InstallOptions) -> Result<(), String> {
    let installer = Installer::prepare()?;
    let layout = installer.layout();
    let style = Style::for_stderr();
    // Shown even with --accept-eula, so every run records what it is about to change.
    eprint!("{}", install_notice(layout, std::env::consts::ARCH, style));
    if !options.accept_eula && !confirm_eula(std::io::stdin().lock(), std::io::stderr()) {
        return Err("the Sysinternals license terms were not accepted".to_owned());
    }
    eprintln!();

    for step in Step::ALL {
        let label = match step {
            Step::Folders => format!(
                "Creating {}",
                style.paint(&layout.root.display().to_string(), Color::Code)
            ),
            Step::Programs => format!("Copying {}", style.paint("procmon-mcp.exe", Color::Code)),
            Step::Procmon => "Downloading Process Monitor".to_owned(),
            Step::Register => format!(
                "Registering the scheduled task {}",
                style.paint(task::TASK_NAME, Color::Code)
            ),
        };
        report_step(style, &label, || {
            installer
                .run(step, &Programs::ThisExecutable)
                .map(|outcome| match outcome {
                    StepOutcome::Done => "Done!",
                    StepOutcome::ProcmonUpdated => "Done! (Microsoft signature verified)",
                    StepOutcome::ProcmonUpToDate => "Already up to date.",
                })
        })?;
    }

    eprintln!(
        "\n{} is installed. Add this to {} in Claude Desktop's {}:\n\n{}\n",
        style.paint("procmon-mcp", Color::Accent),
        style.paint("mcpServers", Color::Code),
        style.paint("claude_desktop_config.json", Color::Code),
        style.highlight_json(&claude_desktop_entry(&layout.server()))
    );
    Ok(())
}

/// Runs one install step, printing `label ... ` and then its result on the same line.
fn report_step(
    style: Style,
    label: &str,
    run: impl FnOnce() -> Result<&'static str, String>,
) -> Result<(), String> {
    eprint!("{label} ... ");
    let _ = std::io::stderr().flush();
    match run() {
        Ok(result) => {
            eprintln!("{}", style.paint(result, Color::Success));
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", style.paint("Failed.", Color::Warning));
            Err(e)
        }
    }
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
    if task::exists().map_err(|e| format!("cannot query the scheduled task: {e}"))? {
        task::delete().map_err(|e| format!("cannot remove the scheduled task: {e}"))?;
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

/// The Installed apps entry, under HKLM.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\ProcmonMcp";

/// The values of the Installed apps entry. `size_kib` is the installed size.
pub fn uninstall_entry(layout: &Layout, size_kib: u32) -> Vec<win::RegValue> {
    let setup = layout.setup().display().to_string();
    vec![
        win::RegValue::string("DisplayName", "procmon-mcp"),
        win::RegValue::string("DisplayVersion", env!("CARGO_PKG_VERSION")),
        win::RegValue::string("DisplayIcon", &setup),
        win::RegValue::string("InstallLocation", &layout.root.display().to_string()),
        win::RegValue::string("UninstallString", &format!("\"{setup}\" --uninstall")),
        win::RegValue::dword("EstimatedSize", size_kib),
        win::RegValue::dword("NoModify", 1),
        win::RegValue::dword("NoRepair", 1),
    ]
}

/// The elevated account must be the signed-in user.
///
/// The capture task runs as that account with its highest privileges, and the MCP
/// server, running as the signed-in user, must be able to start it and write requests.
/// Elevating with another administrator's credentials satisfies neither: a standard
/// user's task is never elevated, and another account's task only starts while that
/// account is signed in.
pub fn ensure_signed_in_user(elevated_sid: &str, signed_in_sid: &str) -> Result<(), String> {
    if elevated_sid == signed_in_sid {
        Ok(())
    } else {
        Err(
            "procmon-mcp can only be installed by the signed-in user, who must be an \
             administrator; this was elevated with another account's credentials. \
             Sign in with an administrator account and run it again"
                .to_owned(),
        )
    }
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
    copy_running_to(&layout.server())
}

/// Copies this executable to `target`, unless it already runs from there (a repair).
fn copy_running_to(target: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| format!("cannot locate this executable: {e}"))?;
    if same_file(&me, target) {
        return Ok(());
    }
    let tmp = target.with_extension("exe.new");
    std::fs::copy(&me, &tmp).map_err(|e| format!("cannot copy to {}: {e}", tmp.display()))?;
    replace_with(&tmp, target)
}

/// Deletes `root` and everything in it. A file that cannot be deleted because it is
/// running (the uninstaller itself, or a server Claude Desktop still runs) is moved into
/// `graveyard` instead; the moved paths are returned so they can be deleted at reboot.
///
/// Links are removed, never followed, like `remove_dir_all` does.
fn remove_tree(root: &Path, graveyard: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut retired = Vec::new();
    remove_tree_into(root, graveyard, &mut retired)?;
    Ok(retired)
}

fn remove_tree_into(
    dir: &Path,
    graveyard: &Path,
    retired: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot list {}: {e}", dir.display()))?
            .path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
        let is_link = meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        if meta.is_dir() && !is_link {
            remove_tree_into(&path, graveyard, retired)?;
        } else if meta.is_dir() {
            std::fs::remove_dir(&path)
                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        } else if std::fs::remove_file(&path).is_err() {
            retired.push(retire(&path, graveyard)?);
        }
    }
    std::fs::remove_dir(dir).map_err(|e| format!("cannot remove {}: {e}", dir.display()))
}

/// Moves a file that is in use out of the way. Running images can be renamed, not deleted.
fn retire(path: &Path, graveyard: &Path) -> Result<std::path::PathBuf, String> {
    let mut id = [0u8; 8];
    win::random_bytes(&mut id).map_err(|e| format!("cannot pick a name: {e}"))?;
    let name = format!(
        "procmon-mcp-{}-{}",
        id.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let target = graveyard.join(name);
    std::fs::rename(path, &target).map_err(|e| {
        format!(
            "cannot remove {}, which is in use (quit Claude Desktop and try again): {e}",
            path.display()
        )
    })?;
    Ok(target)
}

/// Writes a program the setup carries, replacing `target` only once it is complete.
fn write_program(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err(format!(
            "this setup was built without {}; build procmon-mcp first",
            target.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    let tmp = target.with_extension("exe.new");
    std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    replace_with(&tmp, target)
}

/// Moves a fully written `tmp` over `target`.
fn replace_with(tmp: &Path, target: &Path) -> Result<(), String> {
    std::fs::rename(tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(tmp);
        format!(
            "cannot replace {} (quit Claude Desktop if it is running the server): {e}",
            target.display()
        )
    })
}

/// Total size of the files directly in `dir`, for the Installed apps entry.
fn installed_size_kib(dir: &Path) -> u32 {
    let bytes: u64 = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum();
    u32::try_from(bytes.div_ceil(1024)).unwrap_or(u32::MAX)
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
    task::register(&task::task_xml(&layout.server(), user))
        .map_err(|e| format!("cannot register the scheduled task: {e}"))
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

/// What install is about to do, shown before asking for consent. It names every download
/// with its source and destination, so a user can decide before anything is fetched.
pub fn install_notice(layout: &Layout, arch: &str, style: Style) -> String {
    let name = style.paint("procmon-mcp", Color::Accent);
    // Whatever one would put in backticks: paths, URLs, switches, names.
    let code = |text: &str| style.paint(text, Color::Code);
    let url = format!("https://{SYSINTERNALS_LIVE}{}", procmon_download_path(arch));
    format!(
        "Welcome to {name} installation!\n\
         \n\
         This will:\n\
         \x20 - create {root} ({bin}, {jobs}, {out}),\n\
         \x20   writable only by administrators (you may add requests to {jobs}\n\
         \x20   and read or delete results in {out})\n\
         \x20 - copy this program to {server}\n\
         \x20 - download Process Monitor from Microsoft Sysinternals\n\
         \x20     from {url}\n\
         \x20     to   {procmon}\n\
         \x20   and use it only if it carries a valid Microsoft signature\n\
         \x20 - register the scheduled task {task}, which runs a capture with\n\
         \x20   administrator rights when Claude asks for one\n\
         \n\
         This MCP server starts Process Monitor with {accept}, which accepts the\n\
         Sysinternals Software License Terms on your behalf:\n\
         \x20 {eula}\n",
        root = code(&layout.root.display().to_string()),
        bin = code("bin"),
        jobs = code("jobs"),
        out = code("out"),
        server = code(&layout.server().display().to_string()),
        url = code(&url),
        procmon = code(&layout.procmon().display().to_string()),
        task = code(crate::task::TASK_NAME),
        accept = code("/AcceptEula"),
        eula = code(EULA_URL),
    )
}

/// Asks whether the user accepts the Sysinternals license. Only an explicit yes counts.
pub fn confirm_eula(mut input: impl BufRead, mut output: impl Write) -> bool {
    let _ = write!(output, "\nDo you accept them and continue? [y/N] ");
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

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("procmon-mcp-install-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn carried_program_replaces_the_target_and_leaves_no_temporary_file() {
        let dir = TempDir::new("write");
        let target = dir.0.join("procmon-mcp.exe");
        std::fs::write(&target, b"old").unwrap();
        write_program(&target, b"MZ new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"MZ new");
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn empty_carried_program_is_refused_without_touching_the_target() {
        let dir = TempDir::new("empty");
        let target = dir.0.join("procmon-mcp.exe");
        std::fs::write(&target, b"old").unwrap();
        let err = write_program(&target, b"").unwrap_err();
        assert!(err.contains("built without"), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn remove_tree_deletes_nested_files_and_folders() {
        let dir = TempDir::new("tree");
        let root = dir.0.join("ProcmonMcp");
        std::fs::create_dir_all(root.join(r"out\job")).unwrap();
        std::fs::write(root.join(r"out\job\capture.pml"), b"x").unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join(r"bin\state.json"), b"{}").unwrap();
        let graveyard = dir.0.join("graveyard");
        std::fs::create_dir(&graveyard).unwrap();

        let retired = remove_tree(&root, &graveyard).unwrap();
        assert!(retired.is_empty());
        assert!(!root.exists());
    }

    #[test]
    fn remove_tree_moves_a_running_program_aside_instead_of_failing() {
        let dir = TempDir::new("running");
        let root = dir.0.join("ProcmonMcp");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let exe = root.join(r"bin\procmon-mcp.exe");
        // A running image cannot be deleted, only renamed: like Claude Desktop running
        // the server, or the uninstaller running from bin.
        std::fs::copy(win::system_dir().unwrap().join("ping.exe"), &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let graveyard = dir.0.join("graveyard");
        std::fs::create_dir(&graveyard).unwrap();

        let result = remove_tree(&root, &graveyard);
        let _ = child.kill();
        let _ = child.wait();
        let retired = result.unwrap();
        assert!(!root.exists());
        assert_eq!(retired.len(), 1);
        assert!(retired[0].starts_with(&graveyard), "{retired:?}");
        assert!(retired[0].is_file());
    }

    #[test]
    fn installed_size_sums_the_files_in_kib_rounded_up() {
        let dir = TempDir::new("size");
        std::fs::write(dir.0.join("a"), vec![0u8; 1024]).unwrap();
        std::fs::write(dir.0.join("b"), vec![0u8; 1]).unwrap();
        assert_eq!(installed_size_kib(&dir.0), 2);
    }

    fn value(values: &[win::RegValue], name: &str) -> win::RegValue {
        let wide: Vec<u16> = name.encode_utf16().collect();
        values
            .iter()
            .find(|v| v.name == wide)
            .unwrap_or_else(|| panic!("no {name}"))
            .clone()
    }

    #[test]
    fn installed_apps_entry_runs_the_installed_setup_to_uninstall() {
        let layout = Layout::under(Path::new(r"C:\ProgramData"));
        let values = uninstall_entry(&layout, 4321);
        let s = |name| value(&values, name);
        let string = win::RegValue::string;
        assert_eq!(s("DisplayName"), string("DisplayName", "procmon-mcp"));
        assert_eq!(
            s("UninstallString"),
            string(
                "UninstallString",
                r#""C:\ProgramData\ProcmonMcp\bin\procmon-mcp-setup.exe" --uninstall"#
            )
        );
        assert_eq!(
            s("DisplayIcon"),
            string(
                "DisplayIcon",
                r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp-setup.exe"
            )
        );
        assert_eq!(
            s("InstallLocation"),
            string("InstallLocation", r"C:\ProgramData\ProcmonMcp")
        );
        assert_eq!(
            s("DisplayVersion"),
            string("DisplayVersion", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            s("EstimatedSize"),
            win::RegValue::dword("EstimatedSize", 4321)
        );
        // Settings offers only Uninstall: running the setup again is the repair.
        assert_eq!(s("NoModify"), win::RegValue::dword("NoModify", 1));
        assert_eq!(s("NoRepair"), win::RegValue::dword("NoRepair", 1));
    }

    #[test]
    fn installed_apps_entry_lives_under_the_machine_uninstall_key() {
        assert_eq!(
            UNINSTALL_KEY,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\ProcmonMcp"
        );
    }

    #[test]
    fn accepts_an_elevation_by_the_signed_in_user() {
        assert_eq!(
            ensure_signed_in_user("S-1-5-21-1-1001", "S-1-5-21-1-1001"),
            Ok(())
        );
    }

    #[test]
    fn refuses_an_elevation_with_another_account_and_says_why() {
        let err = ensure_signed_in_user("S-1-5-21-1-500", "S-1-5-21-1-1001").unwrap_err();
        assert!(err.contains("signed-in user"), "{err}");
        assert!(err.contains("administrator"), "{err}");
    }

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

    fn notice() -> String {
        let layout = Layout::under(Path::new(r"C:\ProgramData"));
        install_notice(&layout, "x86_64", Style::plain())
    }

    #[test]
    fn notice_welcomes_and_lists_what_will_be_created_downloaded_and_registered() {
        let notice = notice();
        assert!(
            notice.starts_with("Welcome to procmon-mcp installation!\n"),
            "{notice}"
        );
        for expected in [
            r"C:\ProgramData\ProcmonMcp",
            r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe",
            "https://live.sysinternals.com/Procmon64.exe",
            r"C:\ProgramData\ProcmonMcp\bin\Procmon64.exe",
            crate::task::TASK_NAME,
        ] {
            assert!(
                notice.contains(expected),
                "missing {expected:?} in:\n{notice}"
            );
        }
    }

    #[test]
    fn notice_explains_that_starting_procmon_accepts_the_license() {
        let notice = notice();
        assert!(
            notice.contains("This MCP server starts Process Monitor with /AcceptEula"),
            "{notice}"
        );
        assert!(notice.contains(EULA_URL), "{notice}");
    }

    #[test]
    fn notice_names_the_arm64_build_on_arm64() {
        let layout = Layout::under(Path::new(r"C:\ProgramData"));
        let notice = install_notice(&layout, "aarch64", Style::plain());
        assert!(
            notice.contains("https://live.sysinternals.com/Procmon64a.exe"),
            "{notice}"
        );
    }

    #[test]
    fn notice_colors_the_product_name_on_a_terminal() {
        let layout = Layout::under(Path::new(r"C:\ProgramData"));
        let on = Style { enabled: true };
        let notice = install_notice(&layout, "x86_64", on);
        assert!(notice.starts_with(&format!(
            "Welcome to {} installation!",
            on.paint("procmon-mcp", Color::Accent)
        )));
    }

    #[test]
    fn notice_colors_paths_urls_and_switches_as_code() {
        let layout = Layout::under(Path::new(r"C:\ProgramData"));
        let on = Style { enabled: true };
        let notice = install_notice(&layout, "x86_64", on);
        for code in [
            r"C:\ProgramData\ProcmonMcp",
            r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe",
            "https://live.sysinternals.com/Procmon64.exe",
            r"C:\ProgramData\ProcmonMcp\bin\Procmon64.exe",
            crate::task::TASK_NAME,
            "/AcceptEula",
            EULA_URL,
            "jobs",
            "out",
        ] {
            assert!(
                notice.contains(&on.paint(code, Color::Code)),
                "{code:?} not colored in:\n{notice}"
            );
        }
    }

    #[test]
    fn eula_prompt_is_set_apart_by_a_blank_line_and_accepts_only_yes() {
        let (ok, shown) = answer("y\n");
        assert!(ok);
        assert!(shown.starts_with('\n'), "{shown:?}");
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
