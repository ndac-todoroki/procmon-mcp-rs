//! The non-elevated side of a capture: hand the request to the runner and wait for it.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crate::job::{CaptureRequest, JobStatus};
use crate::layout::Layout;
use crate::runner::{RUNNER_ERROR_FILE, STATUS_FILE};

/// How often to look for the runner's status file.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Time allowed beyond the capture itself for the task to start and Procmon to finish.
pub const OVERHEAD: Duration = Duration::from_secs(90);

/// Progress while waiting: seconds elapsed and the expected total.
pub type ProgressFn<'a> = dyn FnMut(f64, f64) + 'a;

/// Fails with a message meant for the user when Process Monitor is already running.
///
/// A second instance cannot capture: it exits without writing events. It is not closed
/// for the user, since that would discard whatever they are capturing in it.
pub fn ensure_procmon_not_running(running: &[String]) -> Result<(), String> {
    match crate::win::find_procmon(running.iter().map(String::as_str)) {
        None => Ok(()),
        Some(name) => Err(format!(
            "Process Monitor ({name}) is already running, and only one instance can capture \
             at a time. Ask the user to close Process Monitor, then try the capture again."
        )),
    }
}

/// Writes `request`, calls `start` (which starts the elevated runner) and waits until the
/// runner reports. Returns the job directory holding the log.
pub fn run_capture(
    layout: &Layout,
    request: &CaptureRequest,
    start: impl FnOnce() -> std::io::Result<()>,
    timeout: Duration,
    progress: &mut ProgressFn<'_>,
) -> Result<PathBuf, String> {
    let request_file = layout.request();
    let tmp = request_file.with_extension("tmp");
    std::fs::write(&tmp, request.to_json().to_string())
        .and_then(|()| std::fs::rename(&tmp, &request_file))
        .map_err(|e| format!("cannot write {}: {e}", request_file.display()))?;

    let started = SystemTime::now();
    start().map_err(|e| format!("cannot start the capture task: {e}"))?;

    let job = layout.job(request.id.as_str());
    let status_file = job.join(STATUS_FILE);
    let runner_error = layout.out().join(RUNNER_ERROR_FILE);
    let begin = Instant::now();
    loop {
        // The runner renames the status into place, so a readable file is complete.
        if let Ok(bytes) = std::fs::read(&status_file) {
            return match JobStatus::parse(&bytes) {
                Ok(JobStatus::Done) => Ok(job),
                Ok(JobStatus::Failed(message)) => Err(format!("the capture failed: {message}")),
                Err(e) => Err(format!("unreadable capture status: {e}")),
            };
        }
        // Errors before the job directory exists; one left from an earlier run is stale.
        let fresh = std::fs::metadata(&runner_error)
            .and_then(|m| m.modified())
            .is_ok_and(|modified| modified >= started);
        if fresh {
            let message = std::fs::read(&runner_error)
                .ok()
                .and_then(|b| JobStatus::parse(&b).ok())
                .map_or_else(
                    || "unknown error".to_owned(),
                    |s| match s {
                        JobStatus::Failed(m) => m,
                        JobStatus::Done => "unknown error".to_owned(),
                    },
                );
            return Err(format!("the capture could not start: {message}"));
        }
        let elapsed = begin.elapsed();
        if elapsed >= timeout {
            return Err(format!(
                "the capture did not finish within {} s",
                timeout.as_secs()
            ));
        }
        progress(elapsed.as_secs_f64(), timeout.as_secs_f64());
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobId;
    use crate::pmc::CaptureFilter;
    use crate::runner::write_status;

    struct TempLayout(Layout);

    impl TempLayout {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("procmon-mcp-capture-{name}-{}", std::process::id()));
            let layout = Layout { root };
            std::fs::create_dir_all(layout.jobs()).unwrap();
            std::fs::create_dir_all(layout.out()).unwrap();
            TempLayout(layout)
        }
    }

    impl Drop for TempLayout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.root);
        }
    }

    fn request() -> CaptureRequest {
        CaptureRequest {
            id: JobId::random().unwrap(),
            duration_s: 1,
            filter: CaptureFilter {
                process_name: Some("a.exe".into()),
                ..CaptureFilter::default()
            },
        }
    }

    /// Plays the runner: after a short delay, creates the job directory and a status.
    fn fake_runner(
        layout: &Layout,
        id: &JobId,
        status: JobStatus,
    ) -> impl FnOnce() -> std::io::Result<()> + use<> {
        let job = layout.job(id.as_str());
        move || {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                std::fs::create_dir_all(&job).unwrap();
                write_status(&job.join(STATUS_FILE), &status).unwrap();
            });
            Ok(())
        }
    }

    #[test]
    fn writes_the_request_starts_the_runner_and_returns_the_job_directory() {
        let t = TempLayout::new("done");
        let r = request();
        let start = fake_runner(&t.0, &r.id, JobStatus::Done);
        let mut ticks = Vec::new();
        let job = run_capture(&t.0, &r, start, Duration::from_secs(10), &mut |e, total| {
            ticks.push((e, total))
        })
        .unwrap();
        assert_eq!(job, t.0.job(r.id.as_str()));
        let written = std::fs::read(t.0.request()).unwrap();
        assert_eq!(CaptureRequest::parse(&written), Ok(r));
        assert!(!ticks.is_empty());
        assert!(ticks.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn reports_a_failed_capture_with_the_runners_message() {
        let t = TempLayout::new("failed");
        let r = request();
        let start = fake_runner(&t.0, &r.id, JobStatus::Failed("Procmon crashed".into()));
        let err =
            run_capture(&t.0, &r, start, Duration::from_secs(10), &mut |_, _| {}).unwrap_err();
        assert!(err.contains("Procmon crashed"), "{err}");
    }

    #[test]
    fn reports_runner_errors_that_happen_before_the_job_starts() {
        let t = TempLayout::new("runner-error");
        let r = request();
        let error_file = t.0.out().join(RUNNER_ERROR_FILE);
        let start = move || {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                write_status(
                    &error_file,
                    &JobStatus::Failed("Procmon64.exe: not signed".into()),
                )
                .unwrap();
            });
            Ok(())
        };
        let err =
            run_capture(&t.0, &r, start, Duration::from_secs(10), &mut |_, _| {}).unwrap_err();
        assert!(err.contains("not signed"), "{err}");
    }

    #[test]
    fn ignores_a_runner_error_left_over_from_an_earlier_run() {
        let t = TempLayout::new("stale");
        let r = request();
        write_status(
            &t.0.out().join(RUNNER_ERROR_FILE),
            &JobStatus::Failed("old problem".into()),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let start = fake_runner(&t.0, &r.id, JobStatus::Done);
        assert!(run_capture(&t.0, &r, start, Duration::from_secs(10), &mut |_, _| {}).is_ok());
    }

    #[test]
    fn gives_up_when_the_runner_never_reports() {
        let t = TempLayout::new("timeout");
        let err = run_capture(
            &t.0,
            &request(),
            || Ok(()),
            Duration::from_secs(1),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.contains("did not finish"), "{err}");
    }

    #[test]
    fn reports_when_the_task_cannot_be_started() {
        let t = TempLayout::new("nostart");
        let start = || Err(std::io::Error::other("task not found"));
        let err = run_capture(
            &t.0,
            &request(),
            start,
            Duration::from_secs(1),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.contains("task not found"), "{err}");
    }
}

#[cfg(test)]
mod already_running_tests {
    use super::*;

    #[test]
    fn asks_the_user_to_close_a_running_process_monitor() {
        let running = ["explorer.exe".to_owned(), "Procmon64.exe".to_owned()];
        let err = ensure_procmon_not_running(&running).unwrap_err();
        assert!(err.contains("Procmon64.exe"), "{err}");
        assert!(err.to_lowercase().contains("close"), "{err}");
    }

    #[test]
    fn passes_when_no_process_monitor_runs() {
        let running = ["explorer.exe".to_owned(), "procmon-mcp.exe".to_owned()];
        assert_eq!(ensure_procmon_not_running(&running), Ok(()));
    }
}
