pub(crate) mod generation;
pub(crate) mod pane;
pub(crate) mod rename;

#[cfg(test)]
mod generation_contract_tests {
    use super::generation::Generation;

    #[test]
    fn only_the_latest_background_generation_is_current() {
        let mut generation = Generation::default();
        let first = generation.advance();
        let second = generation.advance();

        assert!(!generation.is_current(first));
        assert!(generation.is_current(second));
    }
}
