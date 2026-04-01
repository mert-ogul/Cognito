//! notify-debouncer-mini collapses raw `EventKind` into `DebouncedEventKind::{Any, AnyContinuous}`.
//! We map **disk state** (path still exists) to ingest vs removal, not the debounced kind.

#[must_use]
pub fn action_for_disk_state(path_exists: bool) -> &'static str {
    if path_exists {
        "reindex"
    } else {
        "remove"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_maps_to_remove() {
        assert_eq!(action_for_disk_state(false), "remove");
    }

    #[test]
    fn present_file_maps_to_reindex() {
        assert_eq!(action_for_disk_state(true), "reindex");
    }
}
