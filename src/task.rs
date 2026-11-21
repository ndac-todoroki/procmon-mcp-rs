//! The scheduled task that starts the runner elevated, managed through the Task
//! Scheduler COM API. The definition is passed as XML text, so nothing is written to disk.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::TaskScheduler::{
    ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT};

pub const TASK_NAME: &str = "ProcmonMcpCapture";

/// Creates or replaces our task from `xml` (see [`task_xml`]). Needs elevation.
pub fn register(xml: &str) -> io::Result<()> {
    let none = VARIANT::default();
    // SAFETY: COM calls on interfaces obtained from the connected service.
    unsafe {
        root_folder()?.RegisterTask(
            &BSTR::from(TASK_NAME),
            &BSTR::from(xml),
            TASK_CREATE_OR_UPDATE.0,
            &none,
            &none,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &none,
        )?;
    }
    Ok(())
}

/// Starts our task now. Allowed for the installing user by the task's security descriptor.
pub fn run() -> io::Result<()> {
    // SAFETY: COM calls on interfaces obtained from the connected service.
    unsafe {
        root_folder()?
            .GetTask(&BSTR::from(TASK_NAME))?
            .Run(&VARIANT::default())?;
    }
    Ok(())
}

/// Removes our task. Needs elevation.
pub fn delete() -> io::Result<()> {
    // SAFETY: COM call on an interface obtained from the connected service.
    unsafe { root_folder()?.DeleteTask(&BSTR::from(TASK_NAME), 0)? };
    Ok(())
}

/// Whether our task is registered.
pub fn exists() -> io::Result<bool> {
    task_exists(TASK_NAME)
}

fn task_exists(name: &str) -> io::Result<bool> {
    // SAFETY: COM call on an interface obtained from the connected service.
    match unsafe { root_folder()?.GetTask(&BSTR::from(name)) } {
        Ok(_) => Ok(true),
        Err(e) if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The root folder (`\`) of the local Task Scheduler.
fn root_folder() -> io::Result<ITaskFolder> {
    let none = VARIANT::default();
    // SAFETY: COM is initialized on this thread before the service is created. A thread
    // already in the multithreaded apartment gets S_FALSE, which is fine; the matching
    // uninitialize is left to process exit.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        service.Connect(&none, &none, &none, &none)?;
        Ok(service.GetFolder(&BSTR::from("\\"))?)
    }
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
    fn declares_utf16_as_passed_to_the_task_scheduler() {
        // The XML travels as a BSTR (UTF-16), so the declaration must say so.
        assert!(xml().starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
    }

    #[test]
    fn a_task_that_was_never_registered_does_not_exist() {
        // Read-only query through the Task Scheduler COM API; needs no elevation.
        let name = format!("ProcmonMcpTest-never-registered-{}", std::process::id());
        assert!(matches!(task_exists(&name), Ok(false)));
    }
}
