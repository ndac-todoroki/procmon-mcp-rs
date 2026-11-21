//! How the setup window and its elevated worker talk.
//!
//! The window runs with normal rights and starts `procmon-mcp-setup.exe --worker <task> <id>`
//! elevated. The worker connects to the window's named pipe `procmon-mcp-setup-<id>` and
//! writes one line per event. It takes nothing else from the window: the elevated side
//! accepts only the task name and a 32-digit hex ID.

/// What the executable was started to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// Double-clicked: the install walkthrough.
    Install,
    /// From Settings → Apps: the uninstall confirmation.
    Uninstall,
    /// Elevated by the window to do the work.
    Worker { task: Task, id: PipeId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Install,
    Uninstall,
}

impl Task {
    fn arg(self) -> &'static str {
        match self {
            Task::Install => "install",
            Task::Uninstall => "uninstall",
        }
    }
}

/// The random part of the pipe name: 32 lowercase hex digits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipeId(String);

/// One event from the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Install step `n` (0-based) finished.
    StepDone(usize),
    /// Everything succeeded.
    Finished,
    /// The work stopped with this error.
    Failed(String),
}

impl Mode {
    /// Parses the arguments after the program name.
    pub fn parse(args: &[String]) -> Result<Mode, String> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args.as_slice() {
            [] => Ok(Mode::Install),
            ["--uninstall"] => Ok(Mode::Uninstall),
            ["--worker", task, id] => {
                let task = match *task {
                    "install" => Task::Install,
                    "uninstall" => Task::Uninstall,
                    _ => return Err(format!("unknown task: {task}")),
                };
                let id = PipeId::parse(id).ok_or("invalid pipe id")?;
                Ok(Mode::Worker { task, id })
            }
            _ => Err(format!("unexpected arguments: {}", args.join(" "))),
        }
    }
}

/// The arguments that start the elevated worker.
pub fn worker_args(task: Task, id: &PipeId) -> Vec<String> {
    vec!["--worker".into(), task.arg().into(), id.0.clone()]
}

impl PipeId {
    pub fn new() -> std::io::Result<PipeId> {
        let mut bytes = [0u8; 16];
        procmon_mcp::win::random_bytes(&mut bytes)?;
        Ok(PipeId(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    pub fn parse(text: &str) -> Option<PipeId> {
        let valid = text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        valid.then(|| PipeId(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn path(&self) -> String {
        format!(r"\\.\pipe\procmon-mcp-setup-{}", self.0)
    }
}

impl Event {
    pub fn to_line(&self) -> String {
        match self {
            Event::StepDone(n) => format!("step {n}\n"),
            Event::Finished => "finished\n".to_owned(),
            Event::Failed(message) => {
                let flat: Vec<&str> = message.split_whitespace().collect();
                format!("failed {}\n", flat.join(" "))
            }
        }
    }

    pub fn from_line(line: &str) -> Option<Event> {
        if line == "finished" {
            return Some(Event::Finished);
        }
        if let Some(n) = line.strip_prefix("step ") {
            return n.parse().ok().map(Event::StepDone);
        }
        line.strip_prefix("failed ")
            .map(|message| Event::Failed(message.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_means_the_install_walkthrough() {
        assert_eq!(Mode::parse(&args(&[])), Ok(Mode::Install));
    }

    #[test]
    fn uninstall_flag_from_installed_apps() {
        assert_eq!(Mode::parse(&args(&["--uninstall"])), Ok(Mode::Uninstall));
    }

    #[test]
    fn worker_takes_a_task_and_a_pipe_id() {
        assert_eq!(
            Mode::parse(&args(&["--worker", "install", ID])),
            Ok(Mode::Worker {
                task: Task::Install,
                id: PipeId::parse(ID).unwrap()
            })
        );
        assert_eq!(
            Mode::parse(&args(&["--worker", "uninstall", ID])),
            Ok(Mode::Worker {
                task: Task::Uninstall,
                id: PipeId::parse(ID).unwrap()
            })
        );
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            &["--worker"][..],
            &["--worker", "install"],
            &["--worker", "format", ID],
            &["--worker", "install", ID, "extra"],
            &["--worker", "install", "../../x"],
            &["--uninstall", "now"],
            &["/S"],
        ] {
            assert!(Mode::parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn worker_arguments_round_trip() {
        let id = PipeId::parse(ID).unwrap();
        let line = worker_args(Task::Uninstall, &id);
        assert_eq!(
            Mode::parse(&line),
            Ok(Mode::Worker {
                task: Task::Uninstall,
                id
            })
        );
    }

    #[test]
    fn pipe_ids_are_32_lowercase_hex_digits() {
        assert!(PipeId::parse(ID).is_some());
        for bad in [
            "",
            "0123456789abcdef",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcdeg",
            "0123456789abcdef0123456789abcdef0",
        ] {
            assert!(PipeId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn new_pipe_ids_are_valid_and_differ() {
        let a = PipeId::new().unwrap();
        let b = PipeId::new().unwrap();
        assert!(PipeId::parse(a.as_str()).is_some());
        assert_ne!(a, b);
    }

    #[test]
    fn pipe_path_is_in_the_local_pipe_namespace() {
        let id = PipeId::parse(ID).unwrap();
        assert_eq!(id.path(), format!(r"\\.\pipe\procmon-mcp-setup-{ID}"));
    }

    #[test]
    fn events_round_trip_as_single_lines() {
        for event in [
            Event::StepDone(0),
            Event::StepDone(3),
            Event::Finished,
            Event::Failed("cannot replace procmon-mcp.exe".into()),
        ] {
            let line = event.to_line();
            assert!(
                line.ends_with('\n') && line.matches('\n').count() == 1,
                "{line:?}"
            );
            assert_eq!(Event::from_line(line.trim_end()), Some(event));
        }
    }

    #[test]
    fn multi_line_errors_are_flattened() {
        let line = Event::Failed("first\r\nsecond".into()).to_line();
        assert_eq!(
            Event::from_line(line.trim_end()),
            Some(Event::Failed("first second".into()))
        );
    }

    #[test]
    fn unknown_lines_are_ignored() {
        assert_eq!(Event::from_line("step x"), None);
        assert_eq!(Event::from_line("hello"), None);
    }
}
