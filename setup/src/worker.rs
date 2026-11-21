//! The elevated side: does the work and reports it to the window over the pipe.

use std::io::Write;

use procmon_mcp::install::Step;

use crate::protocol::Event;

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
