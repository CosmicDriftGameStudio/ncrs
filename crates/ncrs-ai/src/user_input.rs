//! The window comes before the local model: while someone is using it, the
//! model leaves the GPU alone between two decode calls.

#[cfg(feature = "llama")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
#[cfg(feature = "llama")]
use std::time::Duration;
use std::time::Instant;

/// How long after the last input the model keeps waiting for a job a person
/// waits for.
#[cfg(any(test, feature = "llama"))]
pub(crate) const QUIET_WAITING_MS: u64 = 300;
/// The same for work nobody waits for: it runs only while the window rests.
#[cfg(any(test, feature = "llama"))]
pub(crate) const QUIET_BACKGROUND_MS: u64 = 5_000;
#[cfg(feature = "llama")]
const POLL: Duration = Duration::from_millis(20);

// ponytail: one global time stamp is enough for one window.
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();

/// Milliseconds since the first call; starts at 1 so 0 means "no input yet".
fn now_ms() -> u64 {
    let start = START.get_or_init(Instant::now);
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX) + 1
}

/// Called by the app on every key, click, wheel turn and pointer move.
pub fn note_user_input() {
    LAST_INPUT_MS.store(now_ms(), Ordering::Relaxed);
}

#[cfg(any(test, feature = "llama"))]
fn should_yield(last_input_ms: u64, now_ms: u64, quiet_for_ms: u64) -> bool {
    last_input_ms != 0 && now_ms.saturating_sub(last_input_ms) < quiet_for_ms
}

/// Blocks while the last input is less than `quiet_for_ms` ago; returns at
/// once when `stop` is set.
#[cfg(feature = "llama")]
pub(crate) fn wait_until_quiet(stop: &AtomicBool, quiet_for_ms: u64) {
    while !stop.load(Ordering::Relaxed)
        && should_yield(
            LAST_INPUT_MS.load(Ordering::Relaxed),
            now_ms(),
            quiet_for_ms,
        )
    {
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_job_yields_for_a_short_window_and_a_background_job_for_a_long_one() {
        for window in [QUIET_WAITING_MS, QUIET_BACKGROUND_MS] {
            assert!(!should_yield(0, 100_000, window), "no input yet");
            assert!(should_yield(1_000, 1_100, window));
            assert!(should_yield(1_000, 1_000 + window - 1, window));
            assert!(!should_yield(1_000, 1_000 + window, window));
        }
        assert!(!should_yield(1_000, 2_000, QUIET_WAITING_MS));
        assert!(should_yield(1_000, 2_000, QUIET_BACKGROUND_MS));
    }
}
