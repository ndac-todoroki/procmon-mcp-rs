//! Applying filter rules to an event, independently of where the event comes from.

use std::borrow::Cow;

use super::filter::{Column, FilterRule};

/// Something that filter rules can be applied to, such as a PML event.
pub trait FilterSubject {
    /// The column's display text, or `None` when the subject has no such column.
    fn value(&self, column: Column) -> Option<Cow<'_, str>>;
}

/// Whether Procmon would keep an event under `rules`.
pub fn is_included<S: FilterSubject + ?Sized>(_rules: &[FilterRule], _subject: &S) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct Event(HashMap<Column, &'static str>);

    impl FilterSubject for Event {
        fn value(&self, column: Column) -> Option<Cow<'_, str>> {
            self.0.get(&column).map(|&v| Cow::Borrowed(v))
        }
    }

    fn event(values: &[(Column, &'static str)]) -> Event {
        Event(values.iter().copied().collect())
    }

    #[test]
    fn without_rules_every_event_is_included() {
        assert!(is_included(&[], &event(&[(Column::PROCESS_NAME, "a.exe")])));
    }
}
