//! Renders a [`Summary`] as text for people (CLI) and as JSON for MCP clients.

use std::fmt::Write;

use serde_json::{Value, json};

use crate::analysis::Summary;
use crate::pml::status_name;

const MIB: f64 = 1024.0 * 1024.0;

/// The structured form of a summary, returned as MCP `structuredContent`.
///
/// Each section keeps its number of distinct entries next to the top `top_n`, so a reader
/// can tell when entries were cut off. Derived values are rounded; raw counts are exact.
pub fn to_json(summary: &Summary, top_n: usize) -> Value {
    let secs = summary.duration_secs().max(0.001);
    let section = |items: Vec<Value>| json!({"distinct": items.len(), "top": items.into_iter().take(top_n).collect::<Vec<_>>()});
    json!({
        "total_events": summary.total,
        "duration_secs": round(summary.duration_secs(), 3),
        "events_per_sec": round(summary.total as f64 / secs, 1),
        "top_n": top_n,
        "by_process": section(summary.by_process.iter().map(|p| {
            json!({"name": p.name, "pid": p.pid, "count": p.count})
        }).collect()),
        "by_operation": section(summary.by_operation.iter().map(|(op, count)| {
            json!({"operation": op, "count": count})
        }).collect()),
        "by_operation_path": section(summary.by_operation_path.iter().map(|op| {
            let mut v = json!({"operation": op.operation, "path": op.path, "count": op.count});
            if let Some(bytes) = op.bytes {
                let mib = bytes as f64 / MIB;
                v["bytes"] = json!(bytes);
                v["mib"] = json!(round(mib, 2));
                v["mib_per_sec"] = json!(round(mib / secs, 2));
            }
            v
        }).collect()),
        "periodic_opens": section(summary.periodic_opens.iter().map(|p| {
            json!({
                "process": p.process,
                "path": p.path,
                "count": p.count,
                "mean_interval_ms": round(p.mean_ms, 1),
                "min_interval_ms": round(p.min_ms, 1),
                "max_interval_ms": round(p.max_ms, 1),
            })
        }).collect()),
        "failures": section(summary.failures.iter().map(|f| {
            json!({
                "operation": f.operation,
                "path": f.path,
                "count": f.count,
                "status": status_text(f.result),
                "code": format!("0x{:08X}", f.result),
            })
        }).collect()),
    })
}

fn round(x: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (x * scale).round() / scale
}

/// The Result column text: the NTSTATUS name, or its hex code when we do not know it.
fn status_text(code: u32) -> String {
    status_name(code)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("0x{code:08X}"))
}

/// Renders at most `top_n` entries per section.
pub fn render(summary: &Summary, top_n: usize) -> String {
    if summary.total == 0 {
        return "No events matched the conditions.\n".to_owned();
    }
    // A single event has no span; avoid dividing by zero in the rates.
    let secs = summary.duration_secs().max(0.001);
    let mut out = String::new();
    // Writing to a String cannot fail.
    let _ = writeln!(
        out,
        "Events: {} / Duration: {:.1} s / {:.0} events/s",
        summary.total,
        secs,
        summary.total as f64 / secs
    );

    out.push_str("\n## By process\n");
    for p in summary.by_process.iter().take(top_n) {
        match p.pid {
            Some(pid) => writeln!(out, "- {} (PID {pid}): {}", p.name, p.count),
            None => writeln!(out, "- {}: {}", p.name, p.count),
        }
        .expect("writing to a String cannot fail");
    }

    out.push_str("\n## By operation\n");
    for (name, count) in summary.by_operation.iter().take(top_n) {
        let _ = writeln!(out, "- {name}: {count}");
    }

    out.push_str("\n## Operation and path (by count)\n");
    for op in summary.by_operation_path.iter().take(top_n) {
        let _ = write!(out, "- {} x {} {}", op.count, op.operation, op.path);
        if let Some(bytes) = op.bytes {
            let mib = bytes as f64 / MIB;
            let _ = write!(out, " ({mib:.1} MiB total, {:.2} MiB/s)", mib / secs);
        }
        out.push('\n');
    }

    if !summary.periodic_opens.is_empty() {
        out.push_str("\n## Files opened repeatedly (CreateFile 5+ times)\n");
        for p in summary.periodic_opens.iter().take(top_n) {
            let _ = writeln!(
                out,
                "- {} opens, every {:.0} ms on average (min {:.0}, max {:.0}): {} {}",
                p.count, p.mean_ms, p.min_ms, p.max_ms, p.process, p.path
            );
        }
    }

    if !summary.failures.is_empty() {
        out.push_str("\n## Failed operations\n");
        for f in summary.failures.iter().take(top_n) {
            let _ = writeln!(
                out,
                "- {} x {} {} {}",
                f.count,
                f.operation,
                status_text(f.result),
                f.path
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Failure, OperationPath, PeriodicOpen, ProcessCount};

    fn sample() -> Summary {
        Summary {
            total: 30,
            first_timestamp: Some(0),
            last_timestamp: Some(20_000_000), // 2 s
            by_process: vec![ProcessCount {
                name: "app.exe".into(),
                pid: Some(7),
                count: 30,
            }],
            by_operation: vec![("ReadFile".into(), 20), ("CreateFile".into(), 10)],
            by_operation_path: vec![
                OperationPath {
                    operation: "ReadFile".into(),
                    path: r"C:\a.cfg".into(),
                    count: 20,
                    bytes: Some(4 * 1024 * 1024),
                },
                OperationPath {
                    operation: "CreateFile".into(),
                    path: r"C:\a.cfg".into(),
                    count: 10,
                    bytes: None,
                },
            ],
            periodic_opens: vec![PeriodicOpen {
                process: "app.exe".into(),
                path: r"C:\a.cfg".into(),
                count: 10,
                mean_ms: 312.4,
                min_ms: 284.0,
                max_ms: 329.0,
            }],
            failures: vec![Failure {
                operation: "CreateFile".into(),
                path: r"C:\.cfg".into(),
                result: 0xC000_0034,
                count: 10,
            }],
        }
    }

    #[test]
    fn renders_every_section_with_rates_and_status_names() {
        let expected = "\
Events: 30 / Duration: 2.0 s / 15 events/s

## By process
- app.exe (PID 7): 30

## By operation
- ReadFile: 20
- CreateFile: 10

## Operation and path (by count)
- 20 x ReadFile C:\\a.cfg (4.0 MiB total, 2.00 MiB/s)
- 10 x CreateFile C:\\a.cfg

## Files opened repeatedly (CreateFile 5+ times)
- 10 opens, every 312 ms on average (min 284, max 329): app.exe C:\\a.cfg

## Failed operations
- 10 x CreateFile NAME NOT FOUND C:\\.cfg
";
        assert_eq!(render(&sample(), 15), expected);
    }

    #[test]
    fn limits_each_section_to_top_n() {
        let text = render(&sample(), 1);
        assert!(text.contains("- ReadFile: 20"));
        assert!(!text.contains("- CreateFile: 10"));
    }

    #[test]
    fn omits_empty_optional_sections() {
        let mut s = sample();
        s.periodic_opens.clear();
        s.failures.clear();
        let text = render(&s, 15);
        assert!(!text.contains("## Files opened repeatedly"));
        assert!(!text.contains("## Failed operations"));
    }

    #[test]
    fn says_so_when_nothing_matched() {
        assert_eq!(
            render(&Summary::default(), 15),
            "No events matched the conditions.\n"
        );
    }

    #[test]
    fn shows_unnamed_status_codes_in_hex() {
        let mut s = sample();
        s.failures[0].result = 0xC0DE_0001;
        assert!(render(&s, 15).contains("- 10 x CreateFile 0xC0DE0001 C:\\.cfg"));
    }

    #[test]
    fn json_carries_raw_and_derived_values_per_section() {
        assert_eq!(
            to_json(&sample(), 15),
            json!({
                "total_events": 30,
                "duration_secs": 2.0,
                "events_per_sec": 15.0,
                "top_n": 15,
                "by_process": {"distinct": 1, "top": [
                    {"name": "app.exe", "pid": 7, "count": 30}
                ]},
                "by_operation": {"distinct": 2, "top": [
                    {"operation": "ReadFile", "count": 20},
                    {"operation": "CreateFile", "count": 10}
                ]},
                "by_operation_path": {"distinct": 2, "top": [
                    {"operation": "ReadFile", "path": "C:\\a.cfg", "count": 20,
                     "bytes": 4_194_304, "mib": 4.0, "mib_per_sec": 2.0},
                    {"operation": "CreateFile", "path": "C:\\a.cfg", "count": 10}
                ]},
                "periodic_opens": {"distinct": 1, "top": [
                    {"process": "app.exe", "path": "C:\\a.cfg", "count": 10,
                     "mean_interval_ms": 312.4, "min_interval_ms": 284.0, "max_interval_ms": 329.0}
                ]},
                "failures": {"distinct": 1, "top": [
                    {"operation": "CreateFile", "path": "C:\\.cfg", "count": 10,
                     "status": "NAME NOT FOUND", "code": "0xC0000034"}
                ]}
            })
        );
    }

    #[test]
    fn json_sections_keep_distinct_count_beyond_top_n() {
        let json = to_json(&sample(), 1);
        assert_eq!(json["by_operation"]["distinct"], 2);
        assert_eq!(json["by_operation"]["top"].as_array().unwrap().len(), 1);
    }
}
