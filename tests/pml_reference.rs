//! Checks against real logs in the git-ignored reference directory.
//! Each test is skipped (passes with a note) when its file is missing.

use std::path::PathBuf;

use procmon_mcp::pml::{Header, PmlReader};

fn reference_file(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let path = dir.join(name);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("skipped: {} not found", path.display());
        None
    }
}

#[test]
fn huion_log_header_is_64bit_with_14266_events() {
    let Some(path) = reference_file("Logfile.PML") else { return };
    let bytes = std::fs::read(path).unwrap();
    let header = Header::parse(&bytes[..Header::SIZE]).unwrap();
    assert!(header.is_64bit);
    assert_eq!(header.number_of_events, 14_266);
}

#[test]
fn huion_log_process_table_contains_huion_tablet() {
    let Some(path) = reference_file("Logfile.PML") else { return };
    let reader = PmlReader::open(&path).unwrap();
    let huion: Vec<_> = reader
        .processes()
        .filter(|p| p.name == "HuionTablet.exe")
        .collect();
    assert_eq!(huion.len(), 1);
    assert!(huion[0].pid > 0);
}
