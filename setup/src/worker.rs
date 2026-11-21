//! The elevated side: does the work and reports it to the window over the pipe.

use std::io::Write;

use procmon_mcp::install::{self, Installer, Programs, Step};

use crate::protocol::{Event, PipeId, Task};

/// The procmon-mcp.exe this setup installs, embedded at build time (see build.rs).
const SERVER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/procmon-mcp.exe"));

/// Runs `task` elevated and reports to the window's pipe. Returns the exit code:
/// 0 done, 1 failed (reported), 2 the window could not be reached (nothing done).
pub fn run(task: Task, id: &PipeId) -> i32 {
    let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(id.path()) else {
        return 2;
    };
    let mut failed = false;
    let result = match task {
        Task::Install => match Installer::prepare() {
            Ok(installer) => report_steps(
                &Step::ALL,
                |step| {
                    let result = installer
                        .run(step, &Programs::Setup { server: SERVER })
                        .map(|_| ());
                    failed |= result.is_err();
                    result
                },
                &mut pipe,
            ),
            Err(message) => {
                failed = true;
                send(&mut pipe, Event::Failed(message))
            }
        },
        Task::Uninstall => match install::remove_installation() {
            Ok(report) => {
                if report.in_use > 0 {
                    let _ = send(&mut pipe, Event::InUse(report.in_use));
                }
                send(&mut pipe, Event::Finished)
            }
            Err(message) => {
                failed = true;
                send(&mut pipe, Event::Failed(message))
            }
        },
    };
    match (result, failed) {
        (Err(_), _) => 2,
        (Ok(()), true) => 1,
        (Ok(()), false) => 0,
    }
}

/// Runs `steps` in order through `run`, reporting each finished step, then success or the
/// first error. Stops at the first error.
pub fn report_steps(
    steps: &[Step],
    mut run: impl FnMut(Step) -> Result<(), String>,
    out: &mut impl Write,
) -> std::io::Result<()> {
    for (index, step) in steps.iter().enumerate() {
        if let Err(message) = run(*step) {
            return send(out, Event::Failed(message));
        }
        send(out, Event::StepDone(index))?;
    }
    send(out, Event::Finished)
}

fn send(out: &mut impl Write, event: Event) -> std::io::Result<()> {
    out.write_all(event.to_line().as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(out: &[u8]) -> Vec<Event> {
        String::from_utf8_lossy(out)
            .lines()
            .map(|l| Event::from_line(l).unwrap())
            .collect()
    }

    #[test]
    fn reports_every_step_then_success() {
        let mut out = Vec::new();
        let mut ran = Vec::new();
        report_steps(
            &Step::ALL,
            |step| {
                ran.push(step);
                Ok(())
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(ran, Step::ALL);
        assert_eq!(
            lines(&out),
            [
                Event::StepDone(0),
                Event::StepDone(1),
                Event::StepDone(2),
                Event::StepDone(3),
                Event::Finished
            ]
        );
    }

    #[test]
    fn stops_at_the_first_error_and_reports_it() {
        let mut out = Vec::new();
        let mut ran = Vec::new();
        report_steps(
            &Step::ALL,
            |step| {
                ran.push(step);
                if step == Step::Procmon {
                    Err("download failed".into())
                } else {
                    Ok(())
                }
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(ran, [Step::Folders, Step::Programs, Step::Procmon]);
        assert_eq!(
            lines(&out),
            [
                Event::StepDone(0),
                Event::StepDone(1),
                Event::Failed("download failed".into())
            ]
        );
    }
}
