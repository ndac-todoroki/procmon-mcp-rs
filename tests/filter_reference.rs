//! The default filter template must keep exactly the events Procmon itself kept.
//!
//! Both reference logs come from one capture with Procmon's default filter: one saved
//! with all events, one saved with only the events the filter displayed. Applying our
//! rules to the first must reproduce the second, event for event and in order, apart
//! from profiling events: Procmon saves every one of those regardless of the filter.
//! Reads about 600 MB of logs, so it takes a while (around 20 s in a debug build).

use std::path::PathBuf;

use procmon_mcp::event_filter::EventSubject;
use procmon_mcp::pmc::{default_filter_rules, is_included};
use procmon_mcp::pml::{Event, EventClass, PmlReader, Process};

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

/// Identifies an event across the two files; process table indexes may differ.
fn key(e: &Event, p: Option<&Process>) -> String {
    format!(
        "{} tid={} {} {} 0x{:08X} {:?} [{}]",
        e.timestamp,
        e.tid,
        e.class.display_name(),
        e.operation_name(),
        e.result,
        e.path,
        p.map_or("?", |p| p.name.as_str()),
    )
}

fn reference_pair() -> Option<(PathBuf, PathBuf)> {
    Some((
        reference_file("Logfile_default_nofilter.PML")?,
        reference_file("Logfile_default_filter.PML")?,
    ))
}

/// Keys of the events in `path` that satisfy `keep`, in file order.
fn keys(path: &PathBuf, mut keep: impl FnMut(&Event, Option<&Process>) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    PmlReader::open(path)
        .unwrap()
        .for_each_event(|e, p| {
            if keep(e, p) {
                out.push(key(e, p));
            }
        })
        .unwrap();
    out
}

fn is_profiling(e: &Event) -> bool {
    e.class == EventClass::Profiling
}

#[test]
fn procmon_saves_every_profiling_event_despite_the_exclude_rule() {
    let Some((all, filtered)) = reference_pair() else {
        return;
    };
    let in_all = keys(&all, |e, _| is_profiling(e));
    assert!(!in_all.is_empty());
    assert_eq!(keys(&filtered, |e, _| is_profiling(e)), in_all);
}

#[test]
fn default_template_reproduces_procmons_filtered_log_apart_from_profiling() {
    let Some((all, filtered)) = reference_pair() else {
        return;
    };
    let expected = keys(&filtered, |e, _| !is_profiling(e));
    let rules = default_filter_rules();
    let kept = keys(&all, |event, process| {
        !is_profiling(event) && is_included(&rules, &EventSubject { event, process })
    });

    let first_difference = expected
        .iter()
        .zip(&kept)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(kept.len()));
    assert!(
        expected.len() == kept.len() && first_difference == expected.len(),
        "Procmon kept {}, we kept {}; first difference at #{}:\n  procmon: {:?}\n  ours:    {:?}",
        expected.len(),
        kept.len(),
        first_difference,
        expected.get(first_difference),
        kept.get(first_difference),
    );
}
