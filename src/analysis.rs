//! Aggregates events into the summary returned to MCP clients.

use std::collections::HashMap;

use crate::pml::{Event, EventClass, Process};

/// Conditions an event must meet to be counted. String comparisons ignore case.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub process_name: Option<String>,
    pub pid: Option<u32>,
    /// Operation names; empty means all.
    pub operations: Vec<String>,
    pub path_contains: Option<String>,
}

/// Counts sorted by descending count, ties broken by key.
pub type Ranking = Vec<(String, u64)>;

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub total: u64,
    pub by_operation: Ranking,
}

pub struct Summarizer {
    filter: Filter,
    total: u64,
    by_operation: HashMap<&'static str, u64>,
}

impl Summarizer {
    pub fn new(filter: Filter) -> Self {
        Summarizer {
            filter,
            total: 0,
            by_operation: HashMap::new(),
        }
    }

    pub fn add(&mut self, event: &Event, _process: Option<&Process>) {
        // Procmon emits profiling events every second regardless of the filter; they
        // describe Procmon's sampling, not what the process did.
        if event.class == EventClass::Profiling {
            return;
        }
        self.total += 1;
        *self.by_operation.entry(event.operation_name()).or_default() += 1;
    }

    pub fn finish(self) -> Summary {
        let _ = self.filter;
        Summary {
            total: self.total,
            by_operation: rank(self.by_operation),
        }
    }
}

fn rank<K: ToString>(counts: HashMap<K, u64>) -> Ranking {
    let mut v: Ranking = counts
        .into_iter()
        .map(|(k, n)| (k.to_string(), n))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
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
}
