//! Pure comparison of calendar snapshots so the sync loop only PUTs when
//! something actually changed, regardless of ordering differences between
//! polls.

use std::collections::HashMap;

use crate::models::CalendarEventIn;

/// Returns `true` if `current` differs from `previous` in content (ignoring
/// order). Compares full event content, not just `external_id`, so an edited
/// title/time/location on an unchanged id still counts as a change.
pub fn has_changed(previous: &[CalendarEventIn], current: &[CalendarEventIn]) -> bool {
    if previous.len() != current.len() {
        return true;
    }

    let prev_by_id: HashMap<&str, &CalendarEventIn> =
        previous.iter().map(|e| (e.external_id.as_str(), e)).collect();

    for ev in current {
        match prev_by_id.get(ev.external_id.as_str()) {
            Some(prev_ev) if *prev_ev == ev => {}
            _ => return true,
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, title: &str) -> CalendarEventIn {
        CalendarEventIn {
            external_id: id.to_string(),
            title: title.to_string(),
            start: "2026-07-06T09:00:00Z".to_string(),
            end: "2026-07-06T09:30:00Z".to_string(),
            place: None,
            is_cancelled: false,
        }
    }

    #[test]
    fn identical_sets_report_no_change() {
        let a = vec![ev("1", "Standup"), ev("2", "Review")];
        let b = vec![ev("2", "Review"), ev("1", "Standup")]; // different order
        assert!(!has_changed(&a, &b));
    }

    #[test]
    fn added_event_is_a_change() {
        let a = vec![ev("1", "Standup")];
        let b = vec![ev("1", "Standup"), ev("2", "Review")];
        assert!(has_changed(&a, &b));
    }

    #[test]
    fn removed_event_is_a_change() {
        let a = vec![ev("1", "Standup"), ev("2", "Review")];
        let b = vec![ev("1", "Standup")];
        assert!(has_changed(&a, &b));
    }

    #[test]
    fn edited_title_on_same_id_is_a_change() {
        let a = vec![ev("1", "Standup")];
        let b = vec![ev("1", "Standup (moved)")];
        assert!(has_changed(&a, &b));
    }

    #[test]
    fn empty_sets_are_equal() {
        assert!(!has_changed(&[], &[]));
    }
}
