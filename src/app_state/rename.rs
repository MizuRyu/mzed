use std::path::{Path, PathBuf};

use crate::tabs::Tabs;

pub(crate) fn apply_renamed_path(
    left_tabs: &mut Tabs,
    right_tabs: &mut Tabs,
    favorites: &mut Vec<PathBuf>,
    old: &Path,
    new: PathBuf,
) {
    left_tabs.replace_path(old, new.clone());
    right_tabs.replace_path(old, new.clone());
    for favorite in favorites {
        if favorite == old {
            *favorite = new.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_renamed_path_updates_both_panes_and_favorites() {
        let old = PathBuf::from("/project/old.md");
        let new = PathBuf::from("/project/new.md");
        let mut left = Tabs::default();
        let mut right = Tabs::default();
        let mut favorites = vec![old.clone(), PathBuf::from("/project")];

        left.open(old.clone());
        right.open(old.clone());

        apply_renamed_path(&mut left, &mut right, &mut favorites, &old, new.clone());

        assert_eq!(left.active(), Some(&new));
        assert_eq!(right.active(), Some(&new));
        assert_eq!(favorites, vec![new, PathBuf::from("/project")]);
    }
}
