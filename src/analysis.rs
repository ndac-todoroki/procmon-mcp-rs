//! Aggregates events into the summary returned to MCP clients.

use std::collections::HashMap;
use std::path::Path;

use crate::pml::{Detail, Event, EventClass, PmlError, PmlReader, Process};

/// Reads a PML file and summarizes the events that match `filter`.
pub fn summarize_file(path: &Path, filter: Filter) -> Result<Summary, PmlError> {
    let mut reader = PmlReader::open(path)?;
    let mut summarizer = Summarizer::new(filter);
    reader.for_each_event(|event, process| summarizer.add(event, process))?;
    Ok(summarizer.finish())
}

/// Conditions an event must meet to be counted. String comparisons ignore case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub process_name: Option<String>,
    pub pid: Option<u32>,
    /// Operation names; empty means all.
    pub operations: Vec<String>,
    pub path_contains: Option<String>,
}

impl Filter {
    fn lowercased(self) -> Filter {
        Filter {
            process_name: self.process_name.map(|s| s.to_lowercase()),
            pid: self.pid,
            operations: self.operations.iter().map(|s| s.to_lowercase()).collect(),
            path_contains: self.path_contains.map(|s| s.to_lowercase()),
        }
    }

    /// Expects a filter already passed through `lowercased`.
    fn matches(&self, event: &Event, process: Option<&Process>) -> bool {
        if let Some(name) = &self.process_name
            && process.is_none_or(|p| p.name.to_lowercase() != *name)
        {
            return false;
        }
        if let Some(pid) = self.pid
            && process.is_none_or(|p| p.pid != pid)
        {
            return false;
        }
        if !self.operations.is_empty() {
            let op = event.operation_name().to_lowercase();
            if !self.operations.contains(&op) {
                return false;
            }
        }
        if let Some(needle) = &self.path_contains {
            let path = event.path.as_deref().unwrap_or("").to_lowercase();
            if !path.contains(needle.as_str()) {
                return false;
            }
        }
        true
    }
}

/// Counts sorted by descending count, ties broken by key.
pub type Ranking = Vec<(String, u64)>;

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub total: u64,
    /// FILETIME of the first and last matching event.
    pub first_timestamp: Option<u64>,
    pub last_timestamp: Option<u64>,
    /// Sorted by descending count, then name and PID.
    pub by_process: Vec<ProcessCount>,
    pub by_operation: Ranking,
    /// Sorted by descending count, then operation and path.
    pub by_operation_path: Vec<OperationPath>,
    /// Files opened at least `PERIODIC_MIN_OPENS` times by the same process.
    pub periodic_opens: Vec<PeriodicOpen>,
    /// Operations whose NTSTATUS is an error, by descending count.
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCount {
    pub name: String,
    /// `None` when the event's process is missing from the process table.
    pub pid: Option<u32>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub operation: String,
    pub path: String,
    /// NTSTATUS.
    pub result: u32,
    pub count: u64,
}

const STATUS_END_OF_FILE: u32 = 0xC000_0011;

/// Error severity only: warnings such as NO MORE FILES and BUFFER OVERFLOW are part of
/// normal enumeration, and END OF FILE ends every complete read.
fn is_failure(result: u32) -> bool {
    result >= 0xC000_0000 && result != STATUS_END_OF_FILE
}

impl Summary {
    /// Seconds between the first and last matching event; 0 when there are none.
    pub fn duration_secs(&self) -> f64 {
        match (self.first_timestamp, self.last_timestamp) {
            (Some(first), Some(last)) => last.saturating_sub(first) as f64 / 1e7,
            _ => 0.0,
        }
    }
}

/// Fewer opens than this are not treated as polling.
pub const PERIODIC_MIN_OPENS: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct PeriodicOpen {
    pub process: String,
    pub path: String,
    pub count: u64,
    pub mean_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPath {
    pub operation: String,
    pub path: String,
    pub count: u64,
    /// Total bytes transferred, for ReadFile / WriteFile only.
    pub bytes: Option<u64>,
}

#[derive(Default)]
pub struct Summarizer {
    filter: Filter,
    total: u64,
    first_timestamp: Option<u64>,
    last_timestamp: Option<u64>,
    by_process: HashMap<(String, Option<u32>), u64>,
    by_operation: HashMap<&'static str, u64>,
    by_operation_path: HashMap<(&'static str, String), (u64, Option<u64>)>,
    open_times: HashMap<(String, String), Vec<u64>>,
    failures: HashMap<(&'static str, String, u32), u64>,
}

impl Summarizer {
    pub fn new(filter: Filter) -> Self {
        Summarizer {
            filter: filter.lowercased(),
            ..Summarizer::default()
        }
    }

    pub fn add(&mut self, event: &Event, process: Option<&Process>) {
        // Procmon emits profiling events every second regardless of the filter; they
        // describe Procmon's sampling, not what the process did.
        if event.class == EventClass::Profiling || !self.filter.matches(event, process) {
            return;
        }
        self.total += 1;
        self.first_timestamp.get_or_insert(event.timestamp);
        self.last_timestamp = Some(event.timestamp);
        let process_key = match process {
            Some(p) => (p.name.clone(), Some(p.pid)),
            None => ("<unknown process>".to_owned(), None),
        };
        *self.by_process.entry(process_key).or_default() += 1;
        *self.by_operation.entry(event.operation_name()).or_default() += 1;

        let path = event.path.clone().unwrap_or_default();
        if is_failure(event.result) {
            *self
                .failures
                .entry((event.operation_name(), path.clone(), event.result))
                .or_default() += 1;
        }
        if event.operation_name() == "CreateFile" {
            let name = process.map(|p| p.name.clone()).unwrap_or_default();
            self.open_times
                .entry((name, path.clone()))
                .or_default()
                .push(event.timestamp);
        }
        let entry = self
            .by_operation_path
            .entry((event.operation_name(), path))
            .or_default();
        entry.0 += 1;
        if let Detail::ReadWrite { length, .. } = event.detail {
            *entry.1.get_or_insert(0) += u64::from(length);
        }
    }

    pub fn finish(self) -> Summary {
        Summary {
            total: self.total,
            first_timestamp: self.first_timestamp,
            last_timestamp: self.last_timestamp,
            by_process: rank_processes(self.by_process),
            by_operation: rank(self.by_operation),
            by_operation_path: rank_operation_paths(self.by_operation_path),
            periodic_opens: periodic_opens(self.open_times),
            failures: rank_failures(self.failures),
        }
    }
}

fn rank_processes(counts: HashMap<(String, Option<u32>), u64>) -> Vec<ProcessCount> {
    let mut v: Vec<ProcessCount> = counts
        .into_iter()
        .map(|((name, pid), count)| ProcessCount { name, pid, count })
        .collect();
    v.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.pid.cmp(&b.pid))
    });
    v
}

fn rank<K: ToString>(counts: HashMap<K, u64>) -> Ranking {
    let mut v: Ranking = counts
        .into_iter()
        .map(|(k, n)| (k.to_string(), n))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

fn rank_failures(counts: HashMap<(&'static str, String, u32), u64>) -> Vec<Failure> {
    let mut v: Vec<Failure> = counts
        .into_iter()
        .map(|((operation, path, result), count)| Failure {
            operation: operation.to_owned(),
            path,
            result,
            count,
        })
        .collect();
    v.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.operation.cmp(&b.operation))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.result.cmp(&b.result))
    });
    v
}

fn periodic_opens(open_times: HashMap<(String, String), Vec<u64>>) -> Vec<PeriodicOpen> {
    const TICKS_PER_MS: f64 = 10_000.0;
    let mut v: Vec<PeriodicOpen> = open_times
        .into_iter()
        .filter(|(_, times)| times.len() >= PERIODIC_MIN_OPENS)
        .map(|((process, path), times)| {
            let intervals: Vec<f64> = times
                .windows(2)
                .map(|w| w[1].saturating_sub(w[0]) as f64 / TICKS_PER_MS)
                .collect();
            PeriodicOpen {
                process,
                path,
                count: times.len() as u64,
                mean_ms: intervals.iter().sum::<f64>() / intervals.len() as f64,
                min_ms: intervals.iter().copied().fold(f64::INFINITY, f64::min),
                max_ms: intervals.iter().copied().fold(0.0, f64::max),
            }
        })
        .collect();
    v.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.process.cmp(&b.process))
            .then_with(|| a.path.cmp(&b.path))
    });
    v
}

fn rank_operation_paths(
    counts: HashMap<(&'static str, String), (u64, Option<u64>)>,
) -> Vec<OperationPath> {
    let mut v: Vec<OperationPath> = counts
        .into_iter()
        .map(|((operation, path), (count, bytes))| OperationPath {
            operation: operation.to_owned(),
            path,
            count,
            bytes,
        })
        .collect();
    v.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.operation.cmp(&b.operation))
            .then_with(|| a.path.cmp(&b.path))
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pml::{Detail, EventClass};

    fn process(name: &str, pid: u32) -> Process {
        Process {
            pid,
            name: name.to_owned(),
        }
    }

    fn fs(operation: u16, path: &str, timestamp: u64) -> Event {
        Event {
            process_index: 0,
            tid: 1,
            class: EventClass::FileSystem,
            operation,
            duration: 0,
            timestamp,
            result: 0,
            path: Some(path.to_owned()),
            detail: Detail::None,
        }
    }

    const CREATE_FILE: u16 = 20;
    const READ_FILE: u16 = 23;
    const CLOSE_FILE: u16 = 38;

    fn profiling() -> Event {
        Event {
            class: EventClass::Profiling,
            operation: 1,
            path: None,
            ..fs(0, "", 0)
        }
    }

    fn summarize(filter: Filter, events: &[(Event, Process)]) -> Summary {
        let mut s = Summarizer::new(filter);
        for (e, p) in events {
            s.add(e, Some(p));
        }
        s.finish()
    }

    #[test]
    fn excludes_profiling_and_ranks_operations_by_count() {
        let p = process("a.exe", 1);
        let events = [
            (fs(READ_FILE, "x", 0), p.clone()),
            (fs(READ_FILE, "x", 1), p.clone()),
            (fs(CLOSE_FILE, "x", 2), p.clone()),
            (fs(CREATE_FILE, "x", 3), p.clone()),
            (profiling(), p.clone()),
        ];
        let s = summarize(Filter::default(), &events);
        assert_eq!(s.total, 4);
        assert_eq!(
            s.by_operation,
            [
                ("ReadFile".to_owned(), 2),
                ("CloseFile".to_owned(), 1),
                ("CreateFile".to_owned(), 1)
            ]
        );
    }

    fn mixed_events() -> Vec<(Event, Process)> {
        vec![
            (fs(READ_FILE, r"C:\App\user.cfg", 0), process("App.exe", 10)),
            (
                fs(CREATE_FILE, r"C:\App\user.cfg", 1),
                process("App.exe", 10),
            ),
            (
                fs(READ_FILE, r"C:\Other\log.txt", 2),
                process("App.exe", 11),
            ),
            (
                fs(READ_FILE, r"C:\App\user.cfg", 3),
                process("other.exe", 20),
            ),
        ]
    }

    #[test]
    fn filters_by_process_name_ignoring_case() {
        let filter = Filter {
            process_name: Some("app.EXE".into()),
            ..Filter::default()
        };
        assert_eq!(summarize(filter, &mixed_events()).total, 3);
    }

    #[test]
    fn filters_by_pid() {
        let filter = Filter {
            pid: Some(11),
            ..Filter::default()
        };
        assert_eq!(summarize(filter, &mixed_events()).total, 1);
    }

    #[test]
    fn filters_by_any_of_the_operations_ignoring_case() {
        let filter = Filter {
            operations: vec!["createfile".into(), "WriteFile".into()],
            ..Filter::default()
        };
        assert_eq!(summarize(filter, &mixed_events()).total, 1);
    }

    #[test]
    fn filters_by_path_substring_ignoring_case() {
        let filter = Filter {
            path_contains: Some("USER.CFG".into()),
            ..Filter::default()
        };
        assert_eq!(summarize(filter, &mixed_events()).total, 3);
    }

    #[test]
    fn combines_conditions_with_and() {
        let filter = Filter {
            process_name: Some("App.exe".into()),
            path_contains: Some("user.cfg".into()),
            operations: vec!["ReadFile".into()],
            ..Filter::default()
        };
        assert_eq!(summarize(filter, &mixed_events()).total, 1);
    }

    #[test]
    fn records_time_span_of_matching_events_only() {
        let filter = Filter {
            pid: Some(10),
            ..Filter::default()
        };
        let s = summarize(filter, &mixed_events());
        assert_eq!((s.first_timestamp, s.last_timestamp), (Some(0), Some(1)));
        assert_eq!(Summary::default().first_timestamp, None);
    }

    #[test]
    fn ranks_processes_by_name_and_pid() {
        let s = summarize(Filter::default(), &mixed_events());
        let pc = |name: &str, pid, count| ProcessCount {
            name: name.into(),
            pid: Some(pid),
            count,
        };
        assert_eq!(
            s.by_process,
            [
                pc("App.exe", 10, 2),
                pc("App.exe", 11, 1),
                pc("other.exe", 20, 1)
            ]
        );
    }

    fn read(path: &str, length: u32) -> Event {
        Event {
            detail: Detail::ReadWrite { offset: 0, length },
            ..fs(READ_FILE, path, 0)
        }
    }

    #[test]
    fn ranks_operation_and_path_pairs_and_sums_read_write_bytes() {
        let p = process("a.exe", 1);
        let events = [
            (read("big", 4096), p.clone()),
            (read("big", 612), p.clone()),
            (read("small", 10), p.clone()),
            (fs(CREATE_FILE, "big", 0), p.clone()),
        ];
        let s = summarize(Filter::default(), &events);
        assert_eq!(
            s.by_operation_path,
            [
                OperationPath {
                    operation: "ReadFile".into(),
                    path: "big".into(),
                    count: 2,
                    bytes: Some(4708)
                },
                OperationPath {
                    operation: "CreateFile".into(),
                    path: "big".into(),
                    count: 1,
                    bytes: None
                },
                OperationPath {
                    operation: "ReadFile".into(),
                    path: "small".into(),
                    count: 1,
                    bytes: Some(10)
                },
            ]
        );
    }

    const MS: u64 = 10_000; // FILETIME ticks per millisecond

    #[test]
    fn reports_intervals_of_files_opened_five_times_or_more() {
        let p = process("a.exe", 1);
        let mut events: Vec<_> = [0, 100, 250, 300, 400]
            .into_iter()
            .map(|ms| (fs(CREATE_FILE, "polled", ms * MS), p.clone()))
            .collect();
        // Opened only four times: not considered periodic.
        events.extend((0..4).map(|i| (fs(CREATE_FILE, "rare", i * MS), p.clone())));

        let s = summarize(Filter::default(), &events);
        assert_eq!(
            s.periodic_opens,
            [PeriodicOpen {
                process: "a.exe".into(),
                path: "polled".into(),
                count: 5,
                mean_ms: 100.0,
                min_ms: 50.0,
                max_ms: 150.0,
            }]
        );
    }

    fn with_result(mut e: Event, result: u32) -> Event {
        e.result = result;
        e
    }

    #[test]
    fn ranks_error_results_but_not_end_of_file_or_warnings() {
        let p = process("a.exe", 1);
        let events = [
            (
                with_result(fs(CREATE_FILE, ".cfg", 0), 0xC000_0034),
                p.clone(),
            ),
            (
                with_result(fs(CREATE_FILE, ".cfg", 1), 0xC000_0034),
                p.clone(),
            ),
            (
                with_result(fs(CREATE_FILE, "locked", 2), 0xC000_0043),
                p.clone(),
            ),
            // END OF FILE ends every full read; it is not a failure.
            (with_result(fs(READ_FILE, "f", 3), 0xC000_0011), p.clone()),
            // NO MORE FILES is a warning (0x8...), the normal end of a directory listing.
            (with_result(fs(READ_FILE, "d", 4), 0x8000_0006), p.clone()),
            (fs(READ_FILE, "ok", 5), p.clone()),
        ];
        let s = summarize(Filter::default(), &events);
        assert_eq!(
            s.failures,
            [
                Failure {
                    operation: "CreateFile".into(),
                    path: ".cfg".into(),
                    result: 0xC000_0034,
                    count: 2
                },
                Failure {
                    operation: "CreateFile".into(),
                    path: "locked".into(),
                    result: 0xC000_0043,
                    count: 1
                },
            ]
        );
    }
}
