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
}
