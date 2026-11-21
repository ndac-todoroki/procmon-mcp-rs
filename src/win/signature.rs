//! Authenticode verification of the Process Monitor executable.

use std::fmt;
use std::mem::zeroed;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};

/// Why a file failed signature verification.
#[derive(Debug, PartialEq, Eq)]
pub enum SignatureError {
    /// WinVerifyTrust rejected the file (unsigned, tampered, untrusted chain...).
    NotTrusted { code: i32 },
    /// Validly signed, but not by Microsoft.
    WrongSigner { signer: String },
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureError::NotTrusted { code } => {
                write!(
                    f,
                    "no trusted Authenticode signature (WinVerifyTrust 0x{code:08X})"
                )
            }
            SignatureError::WrongSigner { signer } => {
                write!(f, "signed by {signer:?}, not Microsoft Corporation")
            }
        }
    }
}

impl std::error::Error for SignatureError {}

/// The leaf certificate's subject name on Sysinternals binaries.
const MICROSOFT: &str = "Microsoft Corporation";

/// Checks that `path` carries a valid embedded Authenticode signature whose signer is
/// Microsoft Corporation.
///
/// Revocation is not checked: the elevated runner may be offline, and the chain to a
/// trusted root plus the signer name already rule out a substituted binary.
pub fn verify_microsoft_signature(path: &Path) -> Result<(), SignatureError> {
    let wide_path = to_wide(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide_path.as_ptr(),
        hFile: null_mut(),
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..unsafe { zeroed() }
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` and `file` are fully initialized and outlive both calls; the
    // verification state is released by the CLOSE call below on every path.
    let status = unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            (&raw mut data).cast(),
        )
    };
    let signer = if status == 0 {
        // SAFETY: after a successful VERIFY, hWVTStateData holds provider data that stays
        // valid until the CLOSE call.
        unsafe { leaf_signer_name(data.hWVTStateData) }
    } else {
        None
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state opened by the VERIFY call with the same structures.
    unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            (&raw mut data).cast(),
        )
    };

    if status != 0 {
        return Err(SignatureError::NotTrusted { code: status });
    }
    match signer {
        Some(name) if name == MICROSOFT => Ok(()),
        other => Err(SignatureError::WrongSigner {
            signer: other.unwrap_or_default(),
        }),
    }
}

/// The simple display name of the first signer's leaf certificate.
///
/// # Safety
/// `state` must come from a successful `WinVerifyTrust` VERIFY that is not yet closed.
unsafe fn leaf_signer_name(state: HANDLE) -> Option<String> {
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() || (*signer).csCertChain == 0 || (*signer).pasCertChain.is_null() {
            return None;
        }
        let cert = (*(*signer).pasCertChain).pCert;
        if cert.is_null() {
            return None;
        }
        let mut buf = [0u16; 256];
        let len = CertGetNameStringW(
            cert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        );
        // The returned length includes the terminating NUL; 1 means an empty name.
        (len > 1).then(|| String::from_utf16_lossy(&buf[..len as usize - 1]))
    }
}

fn to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unsigned_executable() {
        // The test binary itself is built locally and never signed.
        let me = std::env::current_exe().unwrap();
        assert!(matches!(
            verify_microsoft_signature(&me),
            Err(SignatureError::NotTrusted { .. })
        ));
    }

    #[test]
    fn rejects_a_missing_file() {
        assert!(verify_microsoft_signature(Path::new(r"Z:\no\such\file.exe")).is_err());
    }
}
