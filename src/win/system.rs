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

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

/// Whether this process runs with an elevated (administrator) token.
pub fn is_elevated() -> io::Result<bool> {
    let token = ProcessToken::open()?;
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(elevation.TokenIsElevated != 0)
}

/// The SID of the user this process runs as, e.g. `S-1-5-21-…-1001`.
pub fn current_user_sid() -> io::Result<String> {
    let token = ProcessToken::open()?;
    // TOKEN_USER is followed by the variable-length SID it points to; 256 bytes is
    // far beyond the largest SID (68 bytes).
    let mut buf = [0u64; 32];
    let mut returned = 0u32;
    // SAFETY: the buffer is 8-byte aligned, as TOKEN_USER requires, and its size is passed.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buf.as_mut_ptr().cast(),
            size_of_val(&buf) as u32,
            &mut returned,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = buf.as_ptr().cast::<TOKEN_USER>();
    let mut text = null_mut();
    // SAFETY: the SID pointer refers into `buf`, alive for the call; `text` is
    // LocalFree'd below.
    if unsafe { ConvertSidToStringSidW((*user).User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful conversion returns a NUL-terminated wide string.
    let sid = unsafe { wide_c_str_to_path(text) };
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(text.cast()) };
    Ok(sid.to_string_lossy().into_owned())
}

/// This process's access token, opened for querying and closed on drop.
struct ProcessToken(HANDLE);

impl ProcessToken {
    fn open() -> io::Result<Self> {
        let mut token: HANDLE = null_mut();
        // SAFETY: the pseudo handle from GetCurrentProcess needs no closing.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ProcessToken(token))
    }
}

impl Drop for ProcessToken {
    fn drop(&mut self) {
        // SAFETY: opened by OpenProcessToken and closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Fills `buf` from the system's preferred random number generator.
pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    // SAFETY: the buffer and its length are passed together.
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!("BCryptGenRandom 0x{status:08X}")));
    }
    Ok(())
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
    fn current_user_sid_is_a_user_sid_string() {
        let sid = current_user_sid().unwrap();
        // Local and domain accounts: S-1-5-21-<domain>-<rid>; Microsoft accounts too.
        assert!(sid.starts_with("S-1-5-21-"), "{sid}");
        assert!(
            sid.split('-').skip(1).all(|p| p.parse::<u64>().is_ok()),
            "{sid}"
        );
    }

    #[test]
    fn elevation_query_succeeds() {
        // Whether tests run elevated depends on the shell; the query itself must work.
        // `cargo test` from a normal terminal is expected to report false.
        is_elevated().unwrap();
    }
}
