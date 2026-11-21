//! The summary of the HuionTablet reference capture must match the figures recorded in
//! docs/PLAN.md (produced by the Python prototype with procmon-parser).
//! Skipped when the git-ignored reference file is missing.

use std::path::PathBuf;

use procmon_mcp::analysis::{Filter, Summary, summarize_file};

const USER_CFG: &str = r"C:\Program Files\HuionTablet\res\user.cfg";
const DOT_CFG: &str = r"C:\Program Files\HuionTablet\.cfg";

fn huion_summary() -> Option<Summary> {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let path = dir.join("Logfile.PML");
    if !path.is_file() {
        eprintln!("skipped: {} not found", path.display());
        return None;
    }
    Some(summarize_file(&path, Filter::default()).unwrap())
}

#[test]
fn counts_6426_events_over_about_13_seconds() {
    let Some(s) = huion_summary() else { return };
    assert_eq!(s.total, 6_426);
    let secs = s.duration_secs();
    assert!((12.8..=13.0).contains(&secs), "duration {secs}");
}

#[test]
fn user_cfg_is_read_6300_times_for_195454056_bytes() {
    let Some(s) = huion_summary() else { return };
    let top = &s.by_operation_path[0];
    assert_eq!(
        (
            top.operation.as_str(),
            top.path.as_str(),
            top.count,
            top.bytes
        ),
        ("ReadFile", USER_CFG, 6_300, Some(195_454_056))
    );
}

#[test]
fn user_cfg_is_reopened_about_every_312_ms() {
    let Some(s) = huion_summary() else { return };
    let user_cfg = s
        .periodic_opens
        .iter()
        .find(|p| p.path == USER_CFG)
        .expect("user.cfg reopened periodically");
    assert_eq!(user_cfg.count, 42);
    assert!((user_cfg.mean_ms - 312.0).abs() < 1.0, "{user_cfg:?}");
    assert!((user_cfg.min_ms - 284.0).abs() < 1.0, "{user_cfg:?}");
    assert!((user_cfg.max_ms - 329.0).abs() < 1.0, "{user_cfg:?}");
}

#[test]
fn only_failure_is_name_not_found_on_dot_cfg() {
    let Some(s) = huion_summary() else { return };
    assert_eq!(s.failures.len(), 1, "{:?}", s.failures);
    let f = &s.failures[0];
    assert_eq!(
        (f.operation.as_str(), f.path.as_str(), f.result, f.count),
        ("CreateFile", DOT_CFG, 0xC000_0034, 42)
    );
}
