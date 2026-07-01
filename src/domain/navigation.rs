//! Pure split-pane navigation state.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationEffect {
    None,
    OpenRight,
    CloseRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneLayout {
    split: bool,
    focused: Pane,
}

impl PaneLayout {
    pub fn is_split(self) -> bool {
        self.split
    }

    pub fn focused(self) -> Pane {
        self.focused
    }

    pub fn focus(&mut self, pane: Pane) -> NavigationEffect {
        match pane {
            Pane::Left => {
                self.focused = Pane::Left;
                NavigationEffect::None
            }
            Pane::Right if self.split => {
                self.focused = Pane::Right;
                NavigationEffect::None
            }
            Pane::Right => {
                self.split = true;
                self.focused = Pane::Right;
                NavigationEffect::OpenRight
            }
        }
    }

    pub fn toggle_split(&mut self) -> NavigationEffect {
        if self.split {
            self.split = false;
            self.focused = Pane::Left;
            NavigationEffect::CloseRight
        } else {
            self.split = true;
            self.focused = Pane::Right;
            NavigationEffect::OpenRight
        }
    }

    pub fn close_empty_right(&mut self) -> NavigationEffect {
        if self.split {
            self.split = false;
            self.focused = Pane::Left;
            NavigationEffect::CloseRight
        } else {
            NavigationEffect::None
        }
    }
}
