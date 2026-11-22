//! The elevated side: started by the scheduled task, it runs one capture and exits.
//!
//! It reads only the request from the user-writable `jobs` directory, writes only under
//! `out`, and refuses to run when the installed layout or Procmon's signature does not
//! look exactly as `install` left it.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::job::{CaptureRequest, JobStatus};
use crate::layout::{self, Layout};
use crate::pmc::{capture_configuration, set_dbghelp_path};
use crate::win::{self, Hive};

pub const CAPTURE_FILE: &str = "capture.pml";
pub const CONFIG_FILE: &str = "filter.pmc";
pub const STATUS_FILE: &str = "status.json";

/// Arguments for Process Monitor: capture quietly to `job_dir` and exit after the run time.
pub fn procmon_args(job_dir: &Path, duration_s: u64) -> Vec<OsString> {
    vec![
        "/AcceptEula".into(),
        "/Quiet".into(),
        "/Minimized".into(),
        "/BackingFile".into(),
        job_dir.join(CAPTURE_FILE).into(),
        "/LoadConfig".into(),
        job_dir.join(CONFIG_FILE).into(),
        "/Runtime".into(),
        duration_s.to_string().into(),
    ]
}

/// Reads the request, refusing anything but a small regular file. A link would let the
/// user point the elevated runner at a file they cannot read themselves.
pub fn read_request(path: &Path) -> Result<CaptureRequest, String> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    // Open the link itself rather than its target, then inspect what was opened.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| format!("cannot open the request: {e}"))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("cannot inspect the request: {e}"))?;
    use std::os::windows::fs::MetadataExt;
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("the request is not a regular file".to_owned());
    }
    if meta.len() > crate::job::MAX_REQUEST_BYTES {
        return Err("the request is too large".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(crate::job::MAX_REQUEST_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read the request: {e}"))?;
    CaptureRequest::parse(&bytes)
}

/// Writes `status` to `path` via a temporary file, so readers never see a partial file.
pub fn write_status(path: &Path, status: &JobStatus) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, status.to_json().to_string())?;
    std::fs::rename(&tmp, path)
}

/// Written to `out` when a run fails before it has a job directory, e.g. on a bad request.
pub const RUNNER_ERROR_FILE: &str = "runner-error.json";

/// How long Procmon may take beyond the requested run time to start, flush and exit.
const PROCMON_GRACE: Duration = Duration::from_secs(60);

/// One runner invocation: checks the installation, then serves the pending request.
pub fn run(layout: &Layout) -> Result<(), String> {
    let request = match prepare(layout) {
        Ok(request) => request,
        Err(message) => {
            let _ = write_status(
                &layout.out().join(RUNNER_ERROR_FILE),
                &JobStatus::Failed(message.clone()),
            );
            return Err(message);
        }
    };
    let job_dir = layout.job(request.id.as_str());
    // create_dir, not create_dir_all: an existing directory means a replayed request.
    std::fs::create_dir(&job_dir)
        .map_err(|e| format!("cannot create {}: {e}", job_dir.display()))?;
    let outcome = capture(layout, &request, &job_dir);
    let status = match &outcome {
        Ok(()) => JobStatus::Done,
        Err(message) => JobStatus::Failed(message.clone()),
    };
    write_status(&job_dir.join(STATUS_FILE), &status)
        .map_err(|e| format!("cannot write the status: {e}"))?;
    outcome
}

/// Everything that must hold before Procmon may be started on the user's behalf.
fn prepare(layout: &Layout) -> Result<CaptureRequest, String> {
    let elevated = win::is_elevated().map_err(|e| format!("cannot query elevation: {e}"))?;
    if !elevated {
        return Err("the runner must be started elevated by the scheduled task".to_owned());
    }
    let user = win::current_user_sid().map_err(|e| format!("cannot identify the user: {e}"))?;
    check_security(&layout.programs, &layout::programs_sddl())?;
    check_security(&layout.data, &layout::data_sddl())?;
    check_security(&layout.jobs(), &layout::jobs_sddl(&user))?;
    check_security(&layout.out(), &layout::out_sddl(&user))?;
    // Checked again here: Process Monitor may have been opened since the server checked.
    let running =
        win::running_image_names().map_err(|e| format!("cannot list running processes: {e}"))?;
    crate::capture::ensure_procmon_not_running(&running)?;
    win::verify_microsoft_signature(&layout.procmon())
        .map_err(|e| format!("{}: {e}", layout.procmon().display()))?;
    read_request(&layout.request())
}

fn capture(layout: &Layout, request: &CaptureRequest, job_dir: &Path) -> Result<(), String> {
    let mut config = capture_configuration(&request.filter).map_err(|e| e.to_string())?;
    let system = win::system_dir().map_err(|e| format!("cannot find System32: {e}"))?;
    set_dbghelp_path(&mut config, &system.join("dbghelp.dll"));
    std::fs::write(job_dir.join(CONFIG_FILE), config.to_bytes())
        .map_err(|e| format!("cannot write the Procmon configuration: {e}"))?;

    // Procmon saves the configuration it ran with into the user's GUI settings on exit;
    // put the user's own settings back whatever happens to the capture.
    let settings = win::snapshot_key(Hive::CurrentUser, PROCMON_SETTINGS_KEY)
        .map_err(|e| format!("cannot save the Process Monitor settings: {e}"))?;
    let outcome = run_procmon(layout, request, job_dir);
    let restored = win::restore_key(Hive::CurrentUser, PROCMON_SETTINGS_KEY, &settings)
        .map_err(|e| format!("cannot restore the Process Monitor settings: {e}"));
    outcome.and(restored)
}

/// Where Process Monitor keeps the user's GUI settings.
pub const PROCMON_SETTINGS_KEY: &str = r"Software\Sysinternals\Process Monitor";

fn run_procmon(layout: &Layout, request: &CaptureRequest, job_dir: &Path) -> Result<(), String> {
    let mut procmon = Command::new(layout.procmon())
        .args(procmon_args(job_dir, request.duration_s))
        .spawn()
        .map_err(|e| format!("cannot start Process Monitor: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(request.duration_s) + PROCMON_GRACE;
    loop {
        match procmon.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
            Ok(None) => {
                // Ask Procmon to stop cleanly so the log is closed, then force it.
                let _ = Command::new(layout.procmon()).arg("/Terminate").status();
                std::thread::sleep(Duration::from_secs(10));
                let _ = procmon.kill();
                return Err("Process Monitor did not finish in time".to_owned());
            }
            Err(e) => return Err(format!("cannot wait for Process Monitor: {e}")),
        }
    }
    let log = job_dir.join(CAPTURE_FILE);
    match std::fs::metadata(&log) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        _ => Err("Process Monitor exited without writing a log".to_owned()),
    }
}

/// Fails unless `path` has exactly the owner and DACL in `expected`.
pub fn check_security(path: &Path, expected: &str) -> Result<(), String> {
    let actual = crate::win::security_sddl(path)
        .map_err(|e| format!("cannot read the permissions of {}: {e}", path.display()))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} does not have the permissions install set (expected {expected}, found {actual})",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobId, MAX_REQUEST_BYTES};
    use crate::pmc::CaptureFilter;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("procmon-mcp-runner-{name}-{}", std::process::id()));
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
    fn procmon_captures_quietly_into_the_job_directory_for_the_run_time() {
        let args: Vec<String> = procmon_args(Path::new(r"C:\out\job"), 15)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "/AcceptEula",
                "/Quiet",
                "/Minimized",
                "/BackingFile",
                r"C:\out\job\capture.pml",
                "/LoadConfig",
                r"C:\out\job\filter.pmc",
                "/Runtime",
                "15",
            ]
        );
    }

    fn sample_request() -> CaptureRequest {
        CaptureRequest {
            id: JobId::parse("0123456789abcdef0123456789abcdef").unwrap(),
            duration_s: 5,
            filter: CaptureFilter::default(),
        }
    }

    #[test]
    fn reads_a_valid_request_file() {
        let dir = TempDir::new("read");
        let path = dir.0.join("request.json");
        std::fs::write(&path, sample_request().to_json().to_string()).unwrap();
        assert_eq!(read_request(&path), Ok(sample_request()));
    }

    #[test]
    fn refuses_directories_missing_files_and_oversized_requests() {
        let dir = TempDir::new("refuse");
        assert!(read_request(&dir.0).is_err());
        assert!(read_request(&dir.0.join("missing.json")).is_err());
        let big = dir.0.join("big.json");
        std::fs::write(&big, vec![b' '; MAX_REQUEST_BYTES as usize + 1]).unwrap();
        assert!(read_request(&big).is_err());
    }

    #[test]
    fn status_is_written_whole_under_its_final_name() {
        let dir = TempDir::new("status");
        let failed = JobStatus::Failed("Procmon did not start".into());
        write_status(&dir.0.join(STATUS_FILE), &failed).unwrap();
        let bytes = std::fs::read(dir.0.join(STATUS_FILE)).unwrap();
        assert_eq!(JobStatus::parse(&bytes), Ok(failed));
        // Only the final file remains; the temporary one was renamed into place.
        let names: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [STATUS_FILE]);
    }

    #[test]
    fn security_check_passes_only_on_an_exact_match() {
        use crate::win::{current_user_sid, set_security};
        let dir = TempDir::new("security");
        let me = current_user_sid().unwrap();
        let sddl = format!("O:{me}D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;{me})");
        set_security(&dir.0, &sddl).unwrap();
        assert_eq!(check_security(&dir.0, &sddl), Ok(()));
        let looser = format!("O:{me}D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;{me})(A;OICI;FA;;;WD)");
        assert!(check_security(&dir.0, &looser).is_err());
    }
}
