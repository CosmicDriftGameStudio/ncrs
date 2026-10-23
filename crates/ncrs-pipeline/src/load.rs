use std::num::NonZeroUsize;

/// How many workers the pipeline may use. Gentle is the default because ncrs
/// runs while the user works on the same machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Gentle,
    Balanced,
    Full,
    Workers(NonZeroUsize),
}

const GENTLE_WORKERS: NonZeroUsize = match NonZeroUsize::new(2) {
    Some(count) => count,
    None => NonZeroUsize::MIN,
};
const BALANCED_WORKERS: NonZeroUsize = match NonZeroUsize::new(4) {
    Some(count) => count,
    None => NonZeroUsize::MIN,
};

impl Load {
    /// Never more than the machine has logical cores, and never zero.
    pub fn worker_count(self, logical_cores: NonZeroUsize) -> NonZeroUsize {
        let wanted = match self {
            Self::Gentle => GENTLE_WORKERS,
            Self::Balanced => BALANCED_WORKERS,
            Self::Full => logical_cores,
            Self::Workers(count) => count,
        };
        wanted.min(logical_cores)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn cores(count: usize) -> NonZeroUsize {
        NonZeroUsize::new(count).unwrap()
    }

    fn counts(load: Load) -> Vec<usize> {
        [1, 3, 8, 64]
            .into_iter()
            .map(|available| load.worker_count(cores(available)).get())
            .collect()
    }

    #[test]
    fn named_loads_map_to_worker_counts_capped_by_the_cores() {
        assert_eq!(counts(Load::Gentle), [1, 2, 2, 2]);
        assert_eq!(counts(Load::Balanced), [1, 3, 4, 4]);
        assert_eq!(counts(Load::Full), [1, 3, 8, 64]);
        assert_eq!(Load::default(), Load::Gentle);
    }

    #[test]
    fn an_explicit_worker_count_is_capped_and_never_zero() {
        assert_eq!(counts(Load::Workers(cores(5))), [1, 3, 5, 5]);
        assert_eq!(counts(Load::Workers(cores(1000))), [1, 3, 8, 64]);
    }
}
