//! Signature checks against a genuine Process Monitor executable from Sysinternals.
//! Skipped when the git-ignored reference copy is missing.

use std::path::PathBuf;

use procmon_mcp::win::verify_microsoft_signature;

#[test]
fn accepts_genuine_procmon64() {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let exe = dir.join("Procmon64.exe");
    if !exe.is_file() {
        eprintln!("skipped: {} not found", exe.display());
        return;
    }
    assert_eq!(verify_microsoft_signature(&exe), Ok(()));
}

#[test]
fn rejects_a_tampered_copy_of_procmon64() {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let exe = dir.join("Procmon64.exe");
    if !exe.is_file() {
        eprintln!("skipped: {} not found", exe.display());
        return;
    }
    let mut bytes = std::fs::read(&exe).unwrap();
    // Flip one byte in the middle of the code: the signed hash no longer matches.
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    let tampered =
        std::env::temp_dir().join(format!("procmon-mcp-tampered-{}.exe", std::process::id()));
    std::fs::write(&tampered, bytes).unwrap();
    let result = verify_microsoft_signature(&tampered);
    std::fs::remove_file(&tampered).unwrap();
    assert!(result.is_err(), "tampered copy accepted");
}

#[test]
fn rejects_a_validly_signed_non_microsoft_executable() {
    // Git for Windows is signed by its maintainer, not Microsoft.
    let git = PathBuf::from(std::env::var_os("ProgramFiles").unwrap()).join(r"Git\cmd\git.exe");
    if !git.is_file() {
        eprintln!("skipped: {} not found", git.display());
        return;
    }
    match verify_microsoft_signature(&git) {
        Err(procmon_mcp::win::SignatureError::WrongSigner { signer }) => {
            assert!(signer.contains("Schindelin"), "{signer}");
        }
        other => panic!("expected WrongSigner, got {other:?}"),
    }
}
