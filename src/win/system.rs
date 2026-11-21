//! Process elevation, the current user, randomness and system folder locations.
//!
//! Folders come from the system, never from environment variables: the elevated runner
//! inherits the user's environment, and a redefined `%ProgramData%` would point it at a
//! user-writable directory.

use std::io;
use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{FOLDERID_ProgramData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{Owned, PWSTR};

/// Whether this process runs with an elevated (administrator) token.
pub fn is_elevated() -> io::Result<bool> {
    let token = process_token()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed.
    unsafe {
        GetTokenInformation(
            *token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }?;
    Ok(elevation.TokenIsElevated != 0)
}

/// The SID of the user this process runs as, e.g. `S-1-5-21-…-1001`.
pub fn current_user_sid() -> io::Result<String> {
    let token = process_token()?;
    // TOKEN_USER is followed by the variable-length SID it points to; 256 bytes is
    // far beyond the largest SID (68 bytes).
    let mut buf = [0u64; 32];
    let mut returned = 0u32;
    // SAFETY: the buffer is 8-byte aligned, as TOKEN_USER requires, and its size is passed.
    unsafe {
        GetTokenInformation(
            *token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            size_of_val(&buf) as u32,
            &mut returned,
        )
    }?;
    let user = buf.as_ptr().cast::<TOKEN_USER>();
    let mut text = PWSTR::null();
    // SAFETY: the SID pointer refers into `buf`, alive for the call.
    unsafe { ConvertSidToStringSidW((*user).User.Sid, &mut text) }?;
    // SAFETY: a successful conversion returns a NUL-terminated string allocated with
    // LocalAlloc, which we take over and free after copying.
    let sid = unsafe {
        let _free = Owned::new(HLOCAL(text.0.cast()));
        text.to_string()
    };
    sid.map_err(io::Error::other)
}

/// Fills `buf` from the system's preferred random number generator.
pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    // SAFETY: the slice carries its own length.
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok()?;
    Ok(())
}

/// `C:\ProgramData` as configured on this system.
pub fn program_data_dir() -> io::Result<PathBuf> {
    // SAFETY: on success the shell allocates the string, which we free after copying.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_ProgramData, KF_FLAG_DEFAULT, None)?;
        let copied = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        Ok(PathBuf::from(copied.map_err(io::Error::other)?))
    }
}

/// `C:\Windows\System32` as configured on this system.
pub fn system_dir() -> io::Result<PathBuf> {
    let mut buf = [0u16; 260];
    // SAFETY: the slice carries its own length.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if len == 0 || len >= buf.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
}

/// This process's access token, opened for querying and closed on drop.
fn process_token() -> io::Result<Owned<HANDLE>> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle from GetCurrentProcess needs no closing; the opened token
    // is owned by the returned value.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        Ok(Owned::new(token))
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
