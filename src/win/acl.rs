//! Setting and reading the owner and DACL of files and directories as SDDL strings.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::HLOCAL;
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID,
};
use windows::core::{BOOL, HSTRING, Owned, PWSTR};

/// What we read and write: the owner (who can always rewrite the DACL) and the DACL.
const OWNER_AND_DACL: OBJECT_SECURITY_INFORMATION =
    OBJECT_SECURITY_INFORMATION(OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0);

/// Sets the owner and DACL of `path` from `sddl` (e.g. `O:BAD:PAI(A;OICI;FA;;;BA)`),
/// with the DACL protected from inheriting the parent's entries. Both parts are required.
pub fn set_security(path: &Path, sddl: &str) -> io::Result<()> {
    let mut raw = PSECURITY_DESCRIPTOR::default();
    // SAFETY: on success the descriptor is allocated with LocalAlloc; `_free` releases it.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &mut raw,
            None,
        )
    }?;
    // SAFETY: allocated by the conversion above and freed exactly once.
    let _free = unsafe { Owned::new(HLOCAL(raw.0)) };

    let mut owner = PSID::default();
    let mut present = BOOL::default();
    let mut defaulted = BOOL::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `raw` is a valid descriptor; `owner` and `dacl` point into it and are used
    // only while `_free` keeps it alive.
    unsafe {
        GetSecurityDescriptorOwner(raw, &mut owner, &mut defaulted)?;
        GetSecurityDescriptorDacl(raw, &mut present, &mut dacl, &mut defaulted)?;
    }
    if owner.is_invalid() || !present.as_bool() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SDDL must give both an owner (O:) and a DACL (D:)",
        ));
    }
    // SAFETY: `owner` and `dacl` are valid for the call.
    unsafe {
        SetNamedSecurityInfoW(
            &HSTRING::from(path.as_os_str()),
            SE_FILE_OBJECT,
            OBJECT_SECURITY_INFORMATION(OWNER_AND_DACL.0 | PROTECTED_DACL_SECURITY_INFORMATION.0),
            Some(owner),
            None,
            Some(dacl),
            None,
        )
    }
    .ok()?;
    Ok(())
}

/// The owner and DACL of `path` as an SDDL string, in the form `set_security` takes.
pub fn security_sddl(path: &Path) -> io::Result<String> {
    let mut raw = PSECURITY_DESCRIPTOR::default();
    // SAFETY: on success the system allocates the descriptor with LocalAlloc.
    unsafe {
        GetNamedSecurityInfoW(
            &HSTRING::from(path.as_os_str()),
            SE_FILE_OBJECT,
            OWNER_AND_DACL,
            None,
            None,
            None,
            None,
            &mut raw,
        )
    }
    .ok()?;
    // SAFETY: allocated by the call above and freed exactly once.
    let _free = unsafe { Owned::new(HLOCAL(raw.0)) };
    descriptor_to_sddl(raw)
}

/// `sddl` as Windows renders it, e.g. with well-known SIDs as aliases (`S-1-5-18` as `SY`,
/// the built-in Administrator account as `LA`). Compare SDDL only in this form.
pub fn canonical_sddl(sddl: &str) -> io::Result<String> {
    let mut raw = PSECURITY_DESCRIPTOR::default();
    // SAFETY: on success the descriptor is allocated with LocalAlloc; `_free` releases it.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &mut raw,
            None,
        )
    }?;
    // SAFETY: allocated by the conversion above and freed exactly once.
    let _free = unsafe { Owned::new(HLOCAL(raw.0)) };
    descriptor_to_sddl(raw)
}

/// The owner and DACL of a valid descriptor as an SDDL string.
fn descriptor_to_sddl(raw: PSECURITY_DESCRIPTOR) -> io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: `raw` is a valid descriptor; the returned string is LocalAlloc'd and freed
    // by `_free_text` after it is copied.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            raw,
            SDDL_REVISION_1,
            OWNER_AND_DACL,
            &mut text,
            None,
        )?;
        let _free_text = Owned::new(HLOCAL(text.0.cast()));
        text.to_string().map_err(io::Error::other)
    }
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

    fn sddl_owned_by_me() -> String {
        // Owner: the current user, which needs no elevation to set. Everyone keeps full
        // control so the test can still clean up.
        let me = crate::win::current_user_sid().unwrap();
        format!("O:{me}D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;WD)")
    }

    #[test]
    fn set_security_replaces_owner_and_entries_and_blocks_inheritance() {
        let dir = TempDir::new("set");
        let sddl = sddl_owned_by_me();
        set_security(&dir.0, &sddl).unwrap();
        assert_eq!(
            security_sddl(&dir.0).unwrap(),
            canonical_sddl(&sddl).unwrap()
        );
    }

    #[test]
    fn canonical_sddl_renders_well_known_sids_as_aliases() {
        assert_eq!(
            canonical_sddl("O:S-1-5-32-544D:PAI(A;OICI;FA;;;S-1-5-18)").unwrap(),
            "O:BAD:PAI(A;OICI;FA;;;SY)"
        );
    }

    #[test]
    fn canonical_sddl_rejects_malformed_sddl() {
        assert!(canonical_sddl("D:(not sddl").is_err());
    }

    #[test]
    fn rejects_sddl_without_owner_or_dacl() {
        let dir = TempDir::new("partial");
        assert!(set_security(&dir.0, "D:PAI(A;OICI;FA;;;WD)").is_err());
        let me = crate::win::current_user_sid().unwrap();
        assert!(set_security(&dir.0, &format!("O:{me}")).is_err());
    }

    #[test]
    fn rejects_malformed_sddl() {
        let dir = TempDir::new("bad");
        assert!(set_security(&dir.0, "D:(not sddl").is_err());
    }

    #[test]
    fn reading_a_missing_path_fails() {
        assert!(security_sddl(Path::new(r"Z:\no\such\dir")).is_err());
    }
}
