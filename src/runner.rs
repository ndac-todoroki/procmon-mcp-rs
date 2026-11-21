//! The elevated side: started by the scheduled task, it runs one capture and exits.
//!
//! It reads only the request from the user-writable `jobs` directory, writes only under
//! `out`, and refuses to run when the installed layout or Procmon's signature does not
//! look exactly as `install` left it.

use std::ffi::OsString;
use std::path::Path;

use crate::job::CaptureRequest;

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
