#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Generation(u64);

impl Generation {
    pub(crate) fn advance(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }

    pub(crate) fn is_current(self, candidate: u64) -> bool {
        self.0 == candidate
    }
}
