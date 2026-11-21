//! Applying filter rules to an event, independently of where the event comes from.

use std::borrow::Cow;
use std::cmp::Ordering;

use super::filter::{Action, Column, FilterRule, Relation};

/// Something that filter rules can be applied to, such as a PML event.
pub trait FilterSubject {
    /// The column's display text, or `None` when the subject has no such column.
    fn value(&self, column: Column) -> Option<Cow<'_, str>>;
}

/// Whether Procmon would keep an event under `rules`.
///
/// Any matching Exclude rule drops the event. Otherwise, for every column that has
/// Include rules, at least one of that column's Include rules must match.
/// Text comparisons ignore case; a column the subject lacks matches no rule.
pub fn is_included<S: FilterSubject + ?Sized>(rules: &[FilterRule], subject: &S) -> bool {
    let excluded = rules
        .iter()
        .any(|rule| rule.action == Action::Exclude && matches(rule, subject));
    if excluded {
        return false;
    }
    // (column, any Include rule of that column matched)
    let mut columns: Vec<(Column, bool)> = Vec::new();
    for rule in rules.iter().filter(|rule| rule.action == Action::Include) {
        match columns
            .iter_mut()
            .find(|(column, _)| *column == rule.column)
        {
            Some((_, matched)) => *matched = *matched || matches(rule, subject),
            None => columns.push((rule.column, matches(rule, subject))),
        }
    }
    columns.iter().all(|&(_, matched)| matched)
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
        Relation::LessThan => compare(&actual, &expected) == Some(Ordering::Less),
        Relation::MoreThan => compare(&actual, &expected) == Some(Ordering::Greater),
    }
}

/// Numeric order when both sides are finite numbers, else text order of the lowercased text.
///
/// Procmon's own rule is undocumented. Parsing per column type (durations, times,
/// hex values) is not attempted; plain decimal numbers cover PID, TID and Duration.
fn compare(actual: &str, expected: &str) -> Option<Ordering> {
    let number = |text: &str| text.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    match (number(actual), number(expected)) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => Some(actual.cmp(expected)),
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

    fn include(column: Column, relation: Relation, value: &str) -> FilterRule {
        rule(column, relation, Action::Include, value)
    }

    #[test]
    fn with_an_include_rule_only_matching_events_are_included() {
        let rules = [include(Column::PROCESS_NAME, Relation::Is, "a.exe")];

        assert!(is_included(
            &rules,
            &event(&[(Column::PROCESS_NAME, "a.exe")])
        ));
        assert!(!is_included(
            &rules,
            &event(&[(Column::PROCESS_NAME, "b.exe")])
        ));
    }

    #[test]
    fn include_rules_on_the_same_column_are_alternatives() {
        let rules = [
            include(Column::OPERATION, Relation::Is, "ReadFile"),
            include(Column::OPERATION, Relation::Is, "WriteFile"),
        ];

        assert!(is_included(
            &rules,
            &event(&[(Column::OPERATION, "ReadFile")])
        ));
        assert!(is_included(
            &rules,
            &event(&[(Column::OPERATION, "WriteFile")])
        ));
        assert!(!is_included(
            &rules,
            &event(&[(Column::OPERATION, "CloseFile")])
        ));
    }

    #[test]
    fn include_rules_on_different_columns_must_all_be_satisfied() {
        let rules = [
            include(Column::PROCESS_NAME, Relation::Is, "a.exe"),
            include(Column::OPERATION, Relation::Is, "ReadFile"),
            include(Column::OPERATION, Relation::Is, "WriteFile"),
        ];
        let a_read = event(&[
            (Column::PROCESS_NAME, "a.exe"),
            (Column::OPERATION, "ReadFile"),
        ]);
        let a_close = event(&[
            (Column::PROCESS_NAME, "a.exe"),
            (Column::OPERATION, "CloseFile"),
        ]);
        let b_read = event(&[
            (Column::PROCESS_NAME, "b.exe"),
            (Column::OPERATION, "ReadFile"),
        ]);

        assert!(is_included(&rules, &a_read));
        assert!(!is_included(&rules, &a_close));
        assert!(!is_included(&rules, &b_read));
    }

    #[test]
    fn exclude_rule_wins_over_a_matching_include_rule() {
        let rules = [
            include(Column::PROCESS_NAME, Relation::Is, "a.exe"),
            exclude(Column::OPERATION, Relation::BeginsWith, "IRP_MJ_"),
        ];
        let event = event(&[
            (Column::PROCESS_NAME, "a.exe"),
            (Column::OPERATION, "IRP_MJ_READ"),
        ]);

        assert!(!is_included(&rules, &event));
    }

    #[test]
    fn column_the_subject_lacks_matches_no_rule() {
        let rules = [include(Column::PATH, Relation::IsNot, "x")];
        let excludes = [exclude(Column::PATH, Relation::IsNot, "x")];
        let without_path = event(&[(Column::PROCESS_NAME, "a.exe")]);

        // Not even negated relations: an unknown value is neither equal nor unequal.
        assert!(!is_included(&rules, &without_path));
        assert!(is_included(&excludes, &without_path));
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

    #[test]
    fn ordering_relations_compare_numerically_when_both_sides_are_numbers() {
        // As text "900" sorts after "1000"; numerically it is smaller.
        let cases = [
            (Column::PID, "900", Relation::LessThan, "1000", true),
            (Column::PID, "900", Relation::MoreThan, "1000", false),
            (Column::PID, "1000", Relation::LessThan, "1000", false),
            (Column::PID, "1000", Relation::MoreThan, "1000", false),
            (Column::DURATION, "0.5", Relation::MoreThan, "0.25", true),
        ];
        for (column, actual, relation, value, should_match) in cases {
            let rules = [exclude(column, relation, value)];

            let included = is_included(&rules, &event(&[(column, actual)]));

            assert_eq!(included, !should_match, "{actual} {relation:?} {value}");
        }
    }

    #[test]
    fn ordering_relations_fall_back_to_case_insensitive_text_order() {
        let cases = [
            ("apple", Relation::LessThan, "Banana", true),
            ("apple", Relation::MoreThan, "Banana", false),
            ("10 ms", Relation::LessThan, "9 ms", true),
        ];
        for (actual, relation, value, should_match) in cases {
            let rules = [exclude(Column::DETAIL, relation, value)];

            let included = is_included(&rules, &event(&[(Column::DETAIL, actual)]));

            assert_eq!(included, !should_match, "{actual} {relation:?} {value}");
        }
    }
}
