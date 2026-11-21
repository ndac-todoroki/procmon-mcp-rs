//! Tests against `reference/ProcmonConfiguration.pmc`, exported by the Procmon 4.13 GUI
//! with only the default filter. The file is not committed; each test is skipped when it
//! is missing. Set `PROCMON_MCP_REFERENCE` to the folder that holds it (useful in worktrees).

use std::path::PathBuf;

use procmon_mcp::pmc;

fn reference_pmc() -> Option<Vec<u8>> {
    let dir = std::env::var_os("PROCMON_MCP_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference"));
    let path = dir.join("ProcmonConfiguration.pmc");
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            eprintln!("skipped: cannot read {} ({err})", path.display());
            None
        }
    }
}

#[test]
fn reference_file_round_trips_byte_for_byte() {
    let Some(bytes) = reference_pmc() else { return };

    let config = pmc::parse(&bytes).unwrap();

    assert_eq!(config.to_bytes(), bytes);
}

#[test]
fn reference_filter_rules_round_trip_byte_for_byte() {
    let Some(bytes) = reference_pmc() else { return };
    let config = pmc::parse(&bytes).unwrap();
    let data = config.data("FilterRules").unwrap();

    let rules = pmc::decode_filter_rules(data).unwrap();

    assert_eq!(rules.len(), 24);
    assert_eq!(pmc::encode_filter_rules(&rules), data);
}

#[test]
fn reference_file_has_the_24_records_procmon_exports() {
    let Some(bytes) = reference_pmc() else { return };

    let config = pmc::parse(&bytes).unwrap();

    let names: Vec<&str> = config.records.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Columns",
            "ColumnCount",
            "ColumnMap",
            "DbgHelpPath",
            "Logfile",
            "HighlightFG",
            "HighlightBG",
            "Theme",
            "LogFont",
            "BoookmarkFont",
            "AdvancedMode",
            "Autoscroll",
            "HistoryDepth",
            "Profiling",
            "DestructiveFilter",
            "AlwaysOnTop",
            "ResolveAddresses",
            "SourcePath",
            "SymbolPath",
            "FilterRules",
            "HighlightRules",
            "FlightRecorder",
            "RingBufferSize",
            "RingBufferMin",
        ]
    );
}
