//! Applies Procmon filter rules (from `pmc`) to events read from a PML file.

use std::borrow::Cow;

use crate::pmc::{Column, FilterSubject};
use crate::pml::{Event, Process, result_text};

/// An event as Procmon's filter sees it: the text of each column.
///
/// Only the columns that capture filters use are provided; rules on other columns
/// match nothing.
pub struct EventSubject<'a> {
    pub event: &'a Event,
    pub process: Option<&'a Process>,
}

impl FilterSubject for EventSubject<'_> {
    fn value(&self, column: Column) -> Option<Cow<'_, str>> {
        let e = self.event;
        Some(match column {
            Column::PROCESS_NAME => Cow::Borrowed(self.process?.name.as_str()),
            Column::PID => Cow::Owned(self.process?.pid.to_string()),
            Column::OPERATION => Cow::Borrowed(e.operation_name()),
            // Procmon shows an empty Path for events without one.
            Column::PATH => Cow::Borrowed(e.path.as_deref().unwrap_or("")),
            Column::RESULT => Cow::Owned(result_text(e.result)),
            Column::EVENT_CLASS => Cow::Borrowed(e.class.display_name()),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmc::{Action, FilterRule, Relation, default_filter_rules, is_included};
    use crate::pml::{Detail, EventClass};

    fn event(class: EventClass, operation: u16, path: &str, result: u32) -> Event {
        Event {
            process_index: 0,
            tid: 1,
            class,
            operation,
            duration: 0,
            timestamp: 0,
            result,
            path: Some(path.to_owned()),
            detail: Detail::None,
        }
    }

    const READ_FILE: u16 = 23;
    const IRP_MJ_CLOSE: u16 = 22;

    fn kept(rules: &[FilterRule], event: &Event, name: &str, pid: u32) -> bool {
        let process = Process {
            pid,
            name: name.to_owned(),
        };
        is_included(
            rules,
            &EventSubject {
                event,
                process: Some(&process),
            },
        )
    }

    #[test]
    fn default_filter_keeps_ordinary_file_reads() {
        let read = event(EventClass::FileSystem, READ_FILE, r"C:\a.cfg", 0);
        assert!(kept(&default_filter_rules(), &read, "app.exe", 7));
    }

    #[test]
    fn default_filter_drops_procmon_itself() {
        let read = event(EventClass::FileSystem, READ_FILE, r"C:\a.cfg", 0);
        assert!(!kept(&default_filter_rules(), &read, "Procmon64.exe", 7));
    }

    #[test]
    fn default_filter_drops_irp_names_metafiles_profiling_and_ipc() {
        let rules = default_filter_rules();
        for e in [
            event(EventClass::FileSystem, IRP_MJ_CLOSE, r"C:\a.cfg", 0),
            event(EventClass::FileSystem, READ_FILE, r"C:\$Mft", 0),
            event(EventClass::FileSystem, READ_FILE, r"C:\a.cfg", 0xC01C_0004), // FAST IO DISALLOWED
            event(EventClass::Profiling, 1, "", 0),
            event(EventClass::Ipc, READ_FILE, r"\Device\NamedPipe\x", 0),
        ] {
            assert!(!kept(&rules, &e, "app.exe", 7), "kept {e:?}");
        }
    }

    #[test]
    fn include_rules_match_process_name_and_pid_text() {
        let read = event(EventClass::FileSystem, READ_FILE, r"C:\a.cfg", 0);
        let include = |column, value: &str| FilterRule {
            column,
            relation: Relation::Is,
            action: Action::Include,
            value: value.to_owned(),
        };
        let by_name = [include(Column::PROCESS_NAME, "APP.exe")];
        assert!(kept(&by_name, &read, "app.exe", 7));
        assert!(!kept(&by_name, &read, "other.exe", 7));
        let by_pid = [include(Column::PID, "7")];
        assert!(kept(&by_pid, &read, "app.exe", 7));
        assert!(!kept(&by_pid, &read, "app.exe", 8));
    }
}
