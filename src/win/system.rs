//! Process elevation and system folder locations.
//!
//! Folders come from the system, never from environment variables: the elevated runner
//! inherits the user's environment, and a redefined `%ProgramData%` would point it at a
//! user-writable directory.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

/// Whether this process runs with an elevated (administrator) token.
pub fn is_elevated() -> io::Result<bool> {
    let mut token: HANDLE = null_mut();
    // SAFETY: the pseudo handle from GetCurrentProcess needs no closing; `token` is
    // closed below once opened.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    let result = if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(elevation.TokenIsElevated != 0)
    };
    // SAFETY: `token` was opened above and is closed exactly once.
    unsafe { CloseHandle(token) };
    result
}

/// `C:\ProgramData` as configured on this system.
pub fn program_data_dir() -> io::Result<PathBuf> {
    let mut path = null_mut();
    // SAFETY: on success the shell allocates `path`, which we free with CoTaskMemFree.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, null_mut(), &mut path) };
    let result = if hr < 0 {
        Err(io::Error::from_raw_os_error(hr))
    } else {
        // SAFETY: a successful call returns a NUL-terminated wide string.
        Ok(unsafe { wide_c_str_to_path(path) })
    };
    // SAFETY: freeing NULL is allowed, and the shell expects this even on failure.
    unsafe { CoTaskMemFree(path.cast()) };
    result
}

/// `C:\Windows\System32` as configured on this system.
pub fn system_dir() -> io::Result<PathBuf> {
    let mut buf = [0u16; 260];
    // SAFETY: the buffer length is passed alongside the buffer.
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buf[..len])))
}

/// # Safety
/// `p` must point to a NUL-terminated UTF-16 string.
unsafe fn wide_c_str_to_path(p: *const u16) -> PathBuf {
    let mut len = 0;
    // SAFETY: guaranteed by the caller.
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(p, len)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same_dir(a: &std::path::Path, b: &str) -> bool {
        a.to_string_lossy().eq_ignore_ascii_case(b)
    }

    #[test]
    fn program_data_matches_the_default_environment() {
        // In an unmodified environment both sources agree.
        let expected = std::env::var("ProgramData").unwrap();
        let actual = program_data_dir().unwrap();
        assert!(same_dir(&actual, &expected), "{actual:?} vs {expected}");
    }

    #[test]
    fn system_dir_is_system32_under_the_windows_directory() {
        let expected = format!(r"{}\System32", std::env::var("SystemRoot").unwrap());
        let actual = system_dir().unwrap();
        assert!(same_dir(&actual, &expected), "{actual:?} vs {expected}");
    }

    #[test]
    fn elevation_query_succeeds() {
        // Whether tests run elevated depends on the shell; the query itself must work.
        // `cargo test` from a normal terminal is expected to report false.
        is_elevated().unwrap();
    }
}
