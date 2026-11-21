//! Procmon's default filter, used as the template for generated configurations.

use super::filter::{Action, Column, FilterRule, Relation};

/// The 24 Exclude rules of Procmon 4.13's default filter, in its order.
///
/// Taken from a GUI export (`reference/ProcmonConfiguration.pmc`). procmon-parser's README
/// lists 23 of them; the export also has `Event Class is IPC`.
pub fn default_filter_rules() -> Vec<FilterRule> {
    const RULES: [(Column, Relation, &str); 24] = [
        (Column::PROCESS_NAME, Relation::Is, "Procmon.exe"),
        (Column::PROCESS_NAME, Relation::Is, "Procexp.exe"),
        (Column::PROCESS_NAME, Relation::Is, "Autoruns.exe"),
        (Column::PROCESS_NAME, Relation::Is, "Procmon64.exe"),
        (Column::PROCESS_NAME, Relation::Is, "Procexp64.exe"),
        (Column::PROCESS_NAME, Relation::Is, "System"),
        (Column::OPERATION, Relation::BeginsWith, "IRP_MJ_"),
        (Column::OPERATION, Relation::BeginsWith, "FASTIO_"),
        (Column::RESULT, Relation::BeginsWith, "FAST IO"),
        (Column::PATH, Relation::EndsWith, "pagefile.sys"),
        (Column::PATH, Relation::EndsWith, "$Mft"),
        (Column::PATH, Relation::EndsWith, "$MftMirr"),
        (Column::PATH, Relation::EndsWith, "$LogFile"),
        (Column::PATH, Relation::EndsWith, "$Volume"),
        (Column::PATH, Relation::EndsWith, "$AttrDef"),
        (Column::PATH, Relation::EndsWith, "$Root"),
        (Column::PATH, Relation::EndsWith, "$Bitmap"),
        (Column::PATH, Relation::EndsWith, "$Boot"),
        (Column::PATH, Relation::EndsWith, "$BadClus"),
        (Column::PATH, Relation::EndsWith, "$Secure"),
        (Column::PATH, Relation::EndsWith, "$UpCase"),
        (Column::PATH, Relation::Contains, "$Extend"),
        (Column::EVENT_CLASS, Relation::Is, "Profiling"),
        (Column::EVENT_CLASS, Relation::Is, "IPC"),
    ];
    RULES
        .iter()
        .map(|&(column, relation, value)| FilterRule {
            column,
            relation,
            action: Action::Exclude,
            value: value.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::pmc::{FilterSubject, is_included};

    struct Event {
        process: &'static str,
        operation: &'static str,
        path: &'static str,
        event_class: &'static str,
    }

    impl FilterSubject for Event {
        fn value(&self, column: Column) -> Option<Cow<'_, str>> {
            let value = match column {
                Column::PROCESS_NAME => self.process,
                Column::OPERATION => self.operation,
                Column::PATH => self.path,
                Column::EVENT_CLASS => self.event_class,
                Column::RESULT => "SUCCESS",
                _ => return None,
            };
            Some(Cow::Borrowed(value))
        }
    }

    const ORDINARY: Event = Event {
        process: "notepad.exe",
        operation: "ReadFile",
        path: "C:\\Users\\me\\notes.txt",
        event_class: "File System",
    };

    #[test]
    fn default_rules_keep_ordinary_file_activity() {
        assert!(is_included(&default_filter_rules(), &ORDINARY));
    }

    #[test]
    fn default_rules_drop_procmon_itself_ntfs_metadata_profiling_and_ipc() {
        let dropped = [
            Event {
                process: "Procmon64.exe",
                ..ORDINARY
            },
            Event {
                operation: "IRP_MJ_READ",
                ..ORDINARY
            },
            Event {
                path: "C:\\$Extend\\$UsnJrnl",
                ..ORDINARY
            },
            Event {
                path: "C:\\pagefile.sys",
                ..ORDINARY
            },
            Event {
                event_class: "Profiling",
                ..ORDINARY
            },
            Event {
                event_class: "IPC",
                ..ORDINARY
            },
        ];
        for event in dropped {
            assert!(
                !is_included(&default_filter_rules(), &event),
                "{} {} {} {}",
                event.process,
                event.operation,
                event.path,
                event.event_class
            );
        }
    }
}
