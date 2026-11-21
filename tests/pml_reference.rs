//! Checks against real logs in the git-ignored reference directory.
//! Each test is skipped (passes with a note) when its file is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use procmon_mcp::pml::{Detail, Event, Header, PmlReader};

const USER_CFG: &str = r"C:\Program Files\HuionTablet\res\user.cfg";

fn huion_events() -> Option<Vec<Event>> {
    let path = reference_file("Logfile.PML")?;
    let mut reader = PmlReader::open(&path).unwrap();
    Some(reader.events().map(Result::unwrap).collect())
}

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
    let Some(path) = reference_file("Logfile.PML") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let header = Header::parse(&bytes[..Header::SIZE]).unwrap();
    assert!(header.is_64bit);
    assert_eq!(header.number_of_events, 14_266);
}

#[test]
fn huion_log_process_table_contains_huion_tablet() {
    let Some(path) = reference_file("Logfile.PML") else {
        return;
    };
    let reader = PmlReader::open(&path).unwrap();
    let huion: Vec<_> = reader
        .processes()
        .filter(|p| p.name == "HuionTablet.exe")
        .collect();
    assert_eq!(huion.len(), 1);
    assert!(huion[0].pid > 0);
}

#[test]
fn huion_log_has_7840_process_profiling_events_among_14266() {
    let Some(path) = reference_file("Logfile.PML") else {
        return;
    };
    let mut reader = PmlReader::open(&path).unwrap();
    let mut total = 0;
    let mut profiling = 0;
    for event in reader.events() {
        let event = event.unwrap();
        total += 1;
        if event.operation_name() == "Process Profiling" {
            profiling += 1;
        }
    }
    assert_eq!(total, 14_266);
    assert_eq!(profiling, 7_840);
}

#[test]
fn huion_log_operations_other_than_profiling_are_read_create_close() {
    let Some(events) = huion_events() else { return };
    let mut counts = BTreeMap::new();
    for e in events
        .iter()
        .filter(|e| e.operation_name() != "Process Profiling")
    {
        *counts.entry(e.operation_name()).or_insert(0) += 1;
    }
    assert_eq!(
        counts,
        BTreeMap::from([("CloseFile", 42), ("CreateFile", 84), ("ReadFile", 6_300)])
    );
}

#[test]
fn huion_log_read_file_events_carry_user_cfg_path_and_lengths() {
    let Some(events) = huion_events() else { return };
    let reads: Vec<_> = events
        .iter()
        .filter(|e| e.operation_name() == "ReadFile")
        .collect();
    assert!(reads.iter().all(|e| e.path.as_deref() == Some(USER_CFG)));

    let mut sizes = BTreeMap::new();
    let mut total: u64 = 0;
    for e in &reads {
        let Detail::ReadWrite { length, .. } = e.detail else {
            panic!("ReadFile without read/write detail: {e:?}");
        };
        *sizes.entry(length).or_insert(0) += 1;
        total += u64::from(length);
    }
    assert_eq!(total, 195_454_056);
    assert_eq!(
        sizes,
        BTreeMap::from([(612, 42), (4_096, 3_108), (57_344, 2_646), (61_440, 504)])
    );
}

#[test]
fn huion_log_create_file_on_dot_cfg_fails_with_name_not_found() {
    let Some(events) = huion_events() else { return };
    let dot_cfg: Vec<_> = events
        .iter()
        .filter(|e| e.operation_name() == "CreateFile")
        .filter(|e| e.path.as_deref() == Some(r"C:\Program Files\HuionTablet\.cfg"))
        .collect();
    assert_eq!(dot_cfg.len(), 42);
    assert!(dot_cfg.iter().all(|e| e.result == 0xC000_0034));
}
