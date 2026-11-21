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
pub const QUIET_WAITING_MS: u64 = 300;
/// The same for work nobody waits for: it runs only while the window rests.
#[cfg(any(test, feature = "llama"))]
pub const QUIET_BACKGROUND_MS: u64 = 5_000;
/// The longest a model waits in one go; then a chunk may compute.
// ponytail: a pointer that never rests would starve the background work for
// good; one chunk per 30 s is slow but moves on. Make it adaptive if it shows.
#[cfg(any(test, feature = "llama"))]
pub const MAX_WAIT_MS: u64 = 30_000;
/// The same cap for a job a person waits for: every decoded token waits, so a
/// long cap would stretch an answer by minutes while the pointer moves.
#[cfg(any(test, feature = "llama"))]
pub const MAX_WAIT_WAITING_MS: u64 = 100;
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

/// Keep waiting: the window is not quiet yet and the wait is under the cap.
#[cfg(any(test, feature = "llama"))]
fn keep_waiting(last_input_ms: u64, now_ms: u64, quiet_for_ms: u64, waited_ms: u64) -> bool {
    let cap_ms = if quiet_for_ms == QUIET_WAITING_MS {
        MAX_WAIT_WAITING_MS
    } else {
        MAX_WAIT_MS
    };
    waited_ms < cap_ms && should_yield(last_input_ms, now_ms, quiet_for_ms)
}

/// Blocks while the last input is less than `quiet_for_ms` ago, at most
/// [`MAX_WAIT_MS`]; returns at once when `stop` is set.
#[cfg(feature = "llama")]
pub fn wait_until_quiet(stop: &AtomicBool, quiet_for_ms: u64) {
    let started = now_ms();
    while !stop.load(Ordering::Relaxed)
        && keep_waiting(
            LAST_INPUT_MS.load(Ordering::Relaxed),
            now_ms(),
            quiet_for_ms,
            now_ms().saturating_sub(started),
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

    #[test]
    fn a_job_a_person_waits_for_never_waits_long_for_one_token() {
        assert!(keep_waiting(
            1_000,
            1_010,
            QUIET_WAITING_MS,
            MAX_WAIT_WAITING_MS - 1
        ));
        assert!(!keep_waiting(
            1_000,
            1_010,
            QUIET_WAITING_MS,
            MAX_WAIT_WAITING_MS
        ));
    }

    #[test]
    fn a_wait_ends_at_the_cap_even_when_the_window_never_rests() {
        assert!(keep_waiting(
            1_000,
            1_100,
            QUIET_BACKGROUND_MS,
            MAX_WAIT_MS - 1
        ));
        assert!(!keep_waiting(
            1_000,
            1_100,
            QUIET_BACKGROUND_MS,
            MAX_WAIT_MS
        ));
    }
}
