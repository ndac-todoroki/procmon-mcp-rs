//! The scheduled task that starts the runner elevated, and the `schtasks.exe` calls that
//! manage it. The Task Scheduler API is COM-only, so the documented XML form is used.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

pub const TASK_NAME: &str = "ProcmonMcpCapture";

/// What to ask `schtasks.exe` to do with our task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action<'a> {
    /// Create or replace the task from an XML file.
    Register {
        xml_file: &'a Path,
    },
    Run,
    Delete,
    Query,
}

/// Arguments for `schtasks.exe`.
pub fn schtasks_args(action: Action<'_>) -> Vec<OsString> {
    let task = ["/TN", TASK_NAME].map(OsString::from);
    let mut args: Vec<OsString> = match action {
        Action::Register { .. } => vec!["/Create".into()],
        Action::Run => vec!["/Run".into()],
        Action::Delete => vec!["/Delete".into()],
        Action::Query => vec!["/Query".into()],
    };
    args.extend(task);
    match action {
        Action::Register { xml_file } => {
            args.extend(["/XML".into(), xml_file.as_os_str().to_owned(), "/F".into()]);
        }
        // Without /F, schtasks asks for confirmation on the console.
        Action::Delete => args.push("/F".into()),
        Action::Run | Action::Query => {}
    }
    args
}

/// Runs `schtasks.exe` from System32 (never from PATH) and fails unless it exits 0.
pub fn schtasks(action: Action<'_>) -> io::Result<()> {
    let exe = crate::win::system_dir()?.join("schtasks.exe");
    let output = Command::new(exe).args(schtasks_args(action)).output()?;
    if output.status.success() {
        return Ok(());
    }
    // schtasks writes localized text in the console code page; keep it best effort.
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(io::Error::other(format!(
        "schtasks {action:?} failed ({}): {detail}",
        output.status
    )))
}

/// Longest a runner may live: the longest capture plus generous setup and shutdown time.
pub const EXECUTION_TIME_LIMIT: &str = "PT5M";

/// The task definition. It runs `server_exe runner` as `user_sid` with the highest
/// privileges available, and lets only that user (besides admins) read and start it.
pub fn task_xml(server_exe: &Path, user_sid: &str) -> String {
    let command = xml_escape(&server_exe.to_string_lossy());
    let sid = xml_escape(user_sid);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Runs Process Monitor for procmon-mcp. Started on demand only.</Description>
    <SecurityDescriptor>D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;{sid})</SecurityDescriptor>
  </RegistrationInfo>
  <Principals>
    <Principal id="Author">
      <UserId>{sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>{EXECUTION_TIME_LIMIT}</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>runner</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// UTF-16LE with a byte order mark, the encoding the XML declares.
pub fn encode_utf16_with_bom(text: &str) -> Vec<u8> {
    [0xFF, 0xFE]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "S-1-5-21-1-2-3-1001";

    fn xml() -> String {
        task_xml(
            Path::new(r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe"),
            SID,
        )
    }

    fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find(&format!("</{name}>"))? + start;
        Some(&xml[start..end])
    }

    #[test]
    fn runs_the_runner_command_of_the_installed_executable() {
        let xml = xml();
        assert_eq!(
            element(&xml, "Command"),
            Some(r"C:\ProgramData\ProcmonMcp\bin\procmon-mcp.exe")
        );
        assert_eq!(element(&xml, "Arguments"), Some("runner"));
    }

    #[test]
    fn runs_as_the_installing_user_with_highest_privileges_when_logged_on() {
        let xml = xml();
        assert_eq!(element(&xml, "UserId"), Some(SID));
        assert_eq!(element(&xml, "RunLevel"), Some("HighestAvailable"));
        assert_eq!(element(&xml, "LogonType"), Some("InteractiveToken"));
    }

    #[test]
    fn only_admins_system_and_the_user_may_access_the_task() {
        assert_eq!(
            element(&xml(), "SecurityDescriptor"),
            Some("D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;S-1-5-21-1-2-3-1001)")
        );
    }

    #[test]
    fn has_no_triggers_and_never_overlaps_or_runs_long() {
        let xml = xml();
        assert!(!xml.contains("<Triggers>"));
        assert_eq!(element(&xml, "MultipleInstancesPolicy"), Some("IgnoreNew"));
        assert_eq!(
            element(&xml, "ExecutionTimeLimit"),
            Some(EXECUTION_TIME_LIMIT)
        );
        assert_eq!(element(&xml, "DisallowStartIfOnBatteries"), Some("false"));
        assert_eq!(element(&xml, "StopIfGoingOnBatteries"), Some("false"));
    }

    #[test]
    fn escapes_xml_special_characters_in_the_path() {
        let xml = task_xml(Path::new(r"C:\A&B <x>\procmon-mcp.exe"), SID);
        assert_eq!(
            element(&xml, "Command"),
            Some(r"C:\A&amp;B &lt;x&gt;\procmon-mcp.exe")
        );
    }

    fn args(action: Action<'_>) -> Vec<String> {
        schtasks_args(action)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn schtasks_arguments_name_our_task_for_every_action() {
        assert_eq!(
            args(Action::Register {
                xml_file: Path::new(r"C:\ProgramData\ProcmonMcp\bin\task.xml")
            }),
            [
                "/Create",
                "/TN",
                TASK_NAME,
                "/XML",
                r"C:\ProgramData\ProcmonMcp\bin\task.xml",
                "/F"
            ]
        );
        assert_eq!(args(Action::Run), ["/Run", "/TN", TASK_NAME]);
        assert_eq!(args(Action::Delete), ["/Delete", "/TN", TASK_NAME, "/F"]);
        assert_eq!(args(Action::Query), ["/Query", "/TN", TASK_NAME]);
    }

    #[test]
    fn declares_utf16_and_encodes_with_a_bom() {
        assert!(xml().starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
        assert_eq!(
            encode_utf16_with_bom("<a"),
            vec![0xFF, 0xFE, b'<', 0, b'a', 0]
        );
    }
}
