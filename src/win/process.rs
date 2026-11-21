//! Finding running processes by image name.

use std::io;

use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::core::Owned;

/// Image names of the Process Monitor builds. Only one instance can capture at a time.
const PROCMON_IMAGES: &[&str] = &["Procmon.exe", "Procmon64.exe", "Procmon64a.exe"];

/// Image names of all running processes. Names of elevated processes are visible too.
pub fn running_image_names() -> io::Result<Vec<String>> {
    // SAFETY: the snapshot handle is owned and closed on drop; the entry is sized as the
    // API requires before the first call.
    unsafe {
        let snapshot = Owned::new(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut names = Vec::new();
        Process32FirstW(*snapshot, &mut entry)?;
        loop {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            if Process32NextW(*snapshot, &mut entry).is_err() {
                return Ok(names);
            }
        }
    }
}

/// The first Process Monitor image among `names`, compared without regard to case.
pub fn find_procmon<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .find(|name| PROCMON_IMAGES.iter().any(|p| p.eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_this_test_process_among_running_images() {
        let me = std::env::current_exe().unwrap();
        let me = me.file_name().unwrap().to_string_lossy().to_lowercase();
        let names = running_image_names().unwrap();
        assert!(
            names.iter().any(|n| n.to_lowercase() == me),
            "{me} not in {names:?}"
        );
    }

    #[test]
    fn finds_any_procmon_build_ignoring_case() {
        for name in ["Procmon.exe", "procmon64.EXE", "Procmon64a.exe"] {
            assert_eq!(find_procmon(["svchost.exe", name]), Some(name));
        }
    }

    #[test]
    fn ignores_other_processes_with_similar_names() {
        assert_eq!(
            find_procmon(["procmon-mcp.exe", "Procmon64.exe.bak", "Procexp64.exe"]),
            None
        );
    }
}
