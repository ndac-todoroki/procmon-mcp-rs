//! The scheduled task that starts the runner elevated, and the `schtasks.exe` calls that
//! manage it. The Task Scheduler API is COM-only, so the documented XML form is used.

use std::path::Path;

pub const TASK_NAME: &str = "ProcmonMcpCapture";

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

    #[test]
    fn declares_utf16_and_encodes_with_a_bom() {
        assert!(xml().starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
        assert_eq!(
            encode_utf16_with_bom("<a"),
            vec![0xFF, 0xFE, b'<', 0, b'a', 0]
        );
    }
}
