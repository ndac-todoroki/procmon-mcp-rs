//! Setting and reading file system ACLs as SDDL strings.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};

/// Replaces the DACL of `path` with the one in `sddl` (e.g. `D:PAI(A;OICI;FA;;;BA)`),
/// protected from inheriting the parent's entries.
pub fn set_dacl(path: &Path, sddl: &str) -> io::Result<()> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl: *mut ACL = null_mut();
    // SAFETY: `descriptor` is a valid self-relative descriptor; `dacl` points into it and
    // is used only while it is alive.
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    if present == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SDDL has no DACL",
        ));
    }
    let wide = to_wide(path);
    // SAFETY: `wide` is NUL-terminated and `dacl` is valid for the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    win32_result(status)
}

/// The DACL of `path` as an SDDL string.
pub fn dacl_sddl(path: &Path) -> io::Result<String> {
    let wide = to_wide(path);
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: on success the system allocates `descriptor`, freed by `Descriptor`'s drop.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    win32_result(status)?;
    Descriptor(descriptor).to_dacl_sddl()
}

/// A security descriptor allocated with LocalAlloc by the system.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `wide` is NUL-terminated; the result is owned by the returned value.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor(descriptor))
    }

    fn to_dacl_sddl(&self) -> io::Result<String> {
        let mut text = null_mut();
        let mut len = 0u32;
        // SAFETY: `self.0` is a valid descriptor; `text` is LocalFree'd below.
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                self.0,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                &mut len,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `len` counts the characters written, including the terminating NUL.
        let sddl = unsafe { std::slice::from_raw_parts(text, len as usize) };
        let sddl = String::from_utf16_lossy(sddl)
            .trim_end_matches('\0')
            .to_owned();
        // SAFETY: allocated by the conversion above.
        unsafe { LocalFree(text.cast()) };
        Ok(sddl)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the system with LocalAlloc; freed exactly once.
        unsafe { LocalFree(self.0) };
    }
}

fn win32_result(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

fn to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("procmon-mcp-acl-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // Everyone keeps full control so the test can still clean up without elevation.
    const SDDL: &str = "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;WD)";

    #[test]
    fn set_dacl_replaces_entries_and_blocks_inheritance() {
        let dir = TempDir::new("set");
        set_dacl(&dir.0, SDDL).unwrap();
        assert_eq!(dacl_sddl(&dir.0).unwrap(), SDDL);
    }

    #[test]
    fn rejects_malformed_sddl() {
        let dir = TempDir::new("bad");
        assert!(set_dacl(&dir.0, "D:(not sddl").is_err());
    }

    #[test]
    fn reading_a_missing_path_fails() {
        assert!(dacl_sddl(Path::new(r"Z:\no\such\dir")).is_err());
    }
}
