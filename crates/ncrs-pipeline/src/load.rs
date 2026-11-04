use std::num::{NonZeroU32, NonZeroUsize};

use ncrs_priority::ThreadPriority;

/// How many workers the pipeline may use, how many threads the model gets and
/// how much the work yields to the user. Gentle is the default because ncrs
/// runs while the user works on the same machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Gentle,
    Balanced,
    Full,
    Workers(NonZeroUsize),
}

const fn non_zero(wanted: usize) -> NonZeroUsize {
    match NonZeroUsize::new(wanted) {
        Some(count) => count,
        None => NonZeroUsize::MIN,
    }
}

const GENTLE_WORKERS: NonZeroUsize = non_zero(1);
const BALANCED_WORKERS: NonZeroUsize = non_zero(2);
const GENTLE_AI_THREADS: NonZeroUsize = non_zero(2);
const BALANCED_AI_THREADS: NonZeroUsize = non_zero(4);

impl Load {
    /// Never more than the machine has logical cores, and never zero.
    pub fn worker_count(self, logical_cores: NonZeroUsize) -> NonZeroUsize {
        let wanted = match self {
            Self::Gentle => GENTLE_WORKERS,
            Self::Balanced => BALANCED_WORKERS,
            Self::Full => half_of(logical_cores),
            Self::Workers(count) => count,
        };
        wanted.min(logical_cores)
    }

    /// The threads the model computes with for a document. The same setting
    /// decides this and the workers, so one switch sets the whole load.
    pub fn ai_threads(self, logical_cores: NonZeroUsize) -> NonZeroU32 {
        let wanted = match self {
            Self::Gentle => GENTLE_AI_THREADS,
            Self::Balanced => BALANCED_AI_THREADS,
            Self::Full => half_of(logical_cores),
            Self::Workers(count) => count.min(half_of(logical_cores)),
        };
        let threads = u32::try_from(wanted.min(logical_cores).get()).unwrap_or(u32::MAX);
        NonZeroU32::new(threads).unwrap_or(NonZeroU32::MIN)
    }

    /// Indexing yields to the user most under the gentle load.
    pub fn thread_priority(self) -> ThreadPriority {
        match self {
            Self::Gentle => ThreadPriority::Background,
            Self::Balanced | Self::Full | Self::Workers(_) => ThreadPriority::Utility,
        }
    }
}

fn half_of(logical_cores: NonZeroUsize) -> NonZeroUsize {
    non_zero(logical_cores.get().div_euclid(2))
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
        [1, 4, 12, 64]
            .into_iter()
            .map(|available| load.worker_count(cores(available)).get())
            .collect()
    }

    fn ai_thread_counts(load: Load) -> Vec<u32> {
        [1, 4, 12, 64]
            .into_iter()
            .map(|available| load.ai_threads(cores(available)).get())
            .collect()
    }

    #[test]
    fn named_loads_map_to_worker_counts_capped_by_the_cores() {
        assert_eq!(counts(Load::Gentle), [1, 1, 1, 1]);
        assert_eq!(counts(Load::Balanced), [1, 2, 2, 2]);
        assert_eq!(counts(Load::Full), [1, 2, 6, 32]);
        assert_eq!(Load::default(), Load::Gentle);
    }

    #[test]
    fn named_loads_map_to_ai_threads_capped_by_the_cores() {
        assert_eq!(ai_thread_counts(Load::Gentle), [1, 2, 2, 2]);
        assert_eq!(ai_thread_counts(Load::Balanced), [1, 4, 4, 4]);
        assert_eq!(ai_thread_counts(Load::Full), [1, 2, 6, 32]);
    }

    #[test]
    fn an_explicit_worker_count_gives_at_most_half_the_cores_as_ai_threads() {
        assert_eq!(ai_thread_counts(Load::Workers(cores(5))), [1, 2, 5, 5]);
        assert_eq!(ai_thread_counts(Load::Workers(cores(1000))), [1, 2, 6, 32]);
    }

    #[test]
    fn only_the_gentle_load_runs_in_the_background() {
        assert_eq!(Load::Gentle.thread_priority(), ThreadPriority::Background);
        assert_eq!(Load::Balanced.thread_priority(), ThreadPriority::Utility);
        assert_eq!(Load::Full.thread_priority(), ThreadPriority::Utility);
        assert_eq!(
            Load::Workers(cores(3)).thread_priority(),
            ThreadPriority::Utility
        );
    }

    #[test]
    fn an_explicit_worker_count_is_capped_and_never_zero() {
        assert_eq!(counts(Load::Workers(cores(5))), [1, 4, 5, 5]);
        assert_eq!(counts(Load::Workers(cores(1000))), [1, 4, 12, 64]);
    }
}
