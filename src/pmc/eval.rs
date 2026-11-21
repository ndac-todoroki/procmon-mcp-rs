//! Applying filter rules to an event, independently of where the event comes from.

use std::borrow::Cow;

use super::filter::{Action, Column, FilterRule, Relation};

/// Something that filter rules can be applied to, such as a PML event.
pub trait FilterSubject {
    /// The column's display text, or `None` when the subject has no such column.
    fn value(&self, column: Column) -> Option<Cow<'_, str>>;
}

/// Whether Procmon would keep an event under `rules`.
pub fn is_included<S: FilterSubject + ?Sized>(rules: &[FilterRule], subject: &S) -> bool {
    !rules
        .iter()
        .any(|rule| rule.action == Action::Exclude && matches(rule, subject))
}

fn matches<S: FilterSubject + ?Sized>(rule: &FilterRule, subject: &S) -> bool {
    let Some(actual) = subject.value(rule.column) else {
        return false;
    };
    // Lowercasing (Unicode, not just ASCII) stands in for Windows' case-insensitive
    // comparison; exact parity with its uppercase table is not worth a dependency.
    let actual = actual.to_lowercase();
    let expected = rule.value.to_lowercase();
    match rule.relation {
        Relation::Is => actual == expected,
        Relation::IsNot => actual != expected,
        Relation::BeginsWith => actual.starts_with(&expected),
        Relation::EndsWith => actual.ends_with(&expected),
        Relation::Contains => actual.contains(&expected),
        Relation::Excludes => !actual.contains(&expected),
        Relation::LessThan | Relation::MoreThan => false,
    }
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

    fn rule(column: Column, relation: Relation, action: Action, value: &str) -> FilterRule {
        FilterRule {
            column,
            relation,
            action,
            value: value.to_string(),
        }
    }

    fn exclude(column: Column, relation: Relation, value: &str) -> FilterRule {
        rule(column, relation, Action::Exclude, value)
    }

    #[test]
    fn without_rules_every_event_is_included() {
        assert!(is_included(&[], &event(&[(Column::PROCESS_NAME, "a.exe")])));
    }

    #[test]
    fn matching_exclude_rule_excludes_the_event() {
        let rules = [exclude(Column::PROCESS_NAME, Relation::Is, "System")];

        assert!(!is_included(
            &rules,
            &event(&[(Column::PROCESS_NAME, "System")])
        ));
    }

    #[test]
    fn exclude_rule_that_does_not_match_leaves_the_event_included() {
        let rules = [exclude(Column::PROCESS_NAME, Relation::Is, "System")];

        assert!(is_included(
            &rules,
            &event(&[(Column::PROCESS_NAME, "Systemd")])
        ));
        assert!(is_included(&rules, &event(&[(Column::PATH, "System")])));
    }

    #[test]
    fn string_comparison_ignores_case_including_non_ascii() {
        let rules = [
            exclude(Column::PROCESS_NAME, Relation::Is, "Procmon.exe"),
            exclude(Column::PATH, Relation::Is, "C:\\Ünïcode"),
        ];

        for (column, value) in [
            (Column::PROCESS_NAME, "PROCMON.EXE"),
            (Column::PATH, "c:\\üNÏCODE"),
        ] {
            assert!(!is_included(&rules, &event(&[(column, value)])), "{value}");
        }
    }

    #[test]
    fn text_relations_match_as_their_names_say() {
        let path = "C:\\Windows\\System32\\Kernel32.dll";
        let cases = [
            (Relation::Is, "c:\\windows\\system32\\kernel32.dll", true),
            (Relation::Is, "C:\\Windows", false),
            (Relation::IsNot, "C:\\Windows", true),
            (Relation::IsNot, path, false),
            (Relation::BeginsWith, "c:\\windows\\", true),
            (Relation::BeginsWith, "kernel32", false),
            (Relation::EndsWith, ".DLL", true),
            (Relation::EndsWith, "C:\\", false),
            (Relation::Contains, "system32", true),
            (Relation::Contains, "SysWOW64", false),
            (Relation::Excludes, "SysWOW64", true),
            (Relation::Excludes, "system32", false),
        ];
        for (relation, value, should_match) in cases {
            let rules = [exclude(Column::PATH, relation, value)];

            let included = is_included(&rules, &event(&[(Column::PATH, path)]));

            assert_eq!(included, !should_match, "{relation:?} {value}");
        }
    }
}
