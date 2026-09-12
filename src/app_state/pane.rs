use std::path::PathBuf;

pub(crate) fn focused_path(
    left: Option<PathBuf>,
    right: Option<PathBuf>,
    split: bool,
    active_pane: u8,
) -> Option<PathBuf> {
    if split && active_pane == 1 {
        right
    } else {
        left
    }
}

/// The document a WebView selection in `pane` belongs to. why: `pane` is
/// WebView input — only 0 and 1 exist, and 1 only while the split is showing.
pub(crate) fn selected_pane_path(
    pane: u8,
    left: Option<PathBuf>,
    right: Option<PathBuf>,
    split: bool,
) -> Option<PathBuf> {
    match pane {
        0 => left,
        1 if split => right,
        _ => None,
    }
}

pub(crate) fn focused_pane_index(split: bool, active_pane: u8) -> u8 {
    if split && active_pane == 1 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_path_uses_left_when_split_is_closed() {
        let left = Some(PathBuf::from("/left.md"));
        let right = Some(PathBuf::from("/right.md"));

        assert_eq!(focused_path(left.clone(), right, false, 1), left);
    }

    #[test]
    fn focused_path_uses_right_when_split_is_open_and_right_is_focused() {
        let left = Some(PathBuf::from("/left.md"));
        let right = Some(PathBuf::from("/right.md"));

        assert_eq!(
            focused_path(left, right.clone(), true, 1),
            Some(PathBuf::from("/right.md"))
        );
    }

    #[test]
    fn focused_pane_index_falls_back_to_left_when_split_is_closed() {
        assert_eq!(focused_pane_index(false, 1), 0);
    }

    #[test]
    fn selected_pane_path_maps_each_pane_to_its_document() {
        let left = Some(PathBuf::from("/left.md"));
        let right = Some(PathBuf::from("/right.md"));

        assert_eq!(
            selected_pane_path(0, left.clone(), right.clone(), true),
            left
        );
        assert_eq!(
            selected_pane_path(1, left.clone(), right.clone(), true),
            right
        );
    }

    #[test]
    fn selected_pane_path_rejects_the_right_pane_without_a_split() {
        let left = Some(PathBuf::from("/left.md"));
        let right = Some(PathBuf::from("/right.md"));

        assert_eq!(selected_pane_path(1, left, right, false), None);
    }

    #[test]
    fn selected_pane_path_rejects_a_pane_that_does_not_exist() {
        let left = Some(PathBuf::from("/left.md"));
        let right = Some(PathBuf::from("/right.md"));

        assert_eq!(
            selected_pane_path(2, left.clone(), right.clone(), true),
            None
        );
        assert_eq!(selected_pane_path(u8::MAX, left, right, true), None);
    }
}
