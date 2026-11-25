use std::time::{Duration, Instant};

/// Since when every look was quiet; one busy look starts it again.
#[derive(Default)]
pub struct QuietWindow {
    since: Option<Instant>,
}

impl QuietWindow {
    /// True once the looks have been quiet for `needed` without a break.
    pub fn look(&mut self, now: Instant, quiet: bool, needed: Duration) -> bool {
        self.since = if quiet {
            self.since.or(Some(now))
        } else {
            None
        };
        self.since
            .is_some_and(|since| now.duration_since(since) >= needed)
    }

    pub fn since(&self) -> Option<Instant> {
        self.since
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEEDED: Duration = Duration::from_secs(5);

    fn secs(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    #[test]
    fn quiet_looks_a_tick_apart_end_the_wait_after_the_window() {
        let base = Instant::now();
        let mut window = QuietWindow::default();

        let ended: Vec<bool> = (0..=5)
            .map(|second| window.look(base + secs(second), true, NEEDED))
            .collect();

        assert_eq!(ended, [false, false, false, false, false, true]);
        assert_eq!(window.since(), Some(base));
    }

    #[test]
    fn a_busy_look_starts_the_window_again() {
        let base = Instant::now();
        let mut window = QuietWindow::default();
        for second in 0..=2 {
            assert!(!window.look(base + secs(second), true, NEEDED));
        }
        assert!(!window.look(base + secs(3), false, NEEDED));
        assert_eq!(window.since(), None);

        let ended: Vec<bool> = (4..=9)
            .map(|second| window.look(base + secs(second), true, NEEDED))
            .collect();

        assert_eq!(ended, [false, false, false, false, false, true]);
        assert_eq!(window.since(), Some(base + secs(4)));
    }

    #[test]
    fn long_work_keeps_the_wait_open() {
        let base = Instant::now();
        let mut window = QuietWindow::default();

        for second in 0..30 {
            assert!(!window.look(base + secs(second), false, NEEDED));
        }

        assert_eq!(window.since(), None);
    }

    /// The counts are read every fifth tick; for a moment after the tick the
    /// read is in flight.
    fn counts_read_in_flight(at: Duration) -> bool {
        at.as_millis() % 5000 < 150
    }

    #[test]
    fn a_counts_read_every_fifth_tick_ends_the_wait_only_when_looked_at_between_ticks() {
        let base = Instant::now();

        let mut often = QuietWindow::default();
        let ended_often = (0..600).any(|step| {
            let at = Duration::from_millis(step * 100 + 50);
            often.look(base + at, !counts_read_in_flight(at), NEEDED)
        });
        assert!(!ended_often, "looks every 100 ms see a read in flight");

        let mut before_tick = QuietWindow::default();
        let ended_at = (1..=60).find(|second| {
            let at = secs(*second) - Duration::from_millis(1);
            before_tick.look(base + at, !counts_read_in_flight(at), NEEDED)
        });
        assert!(ended_at.is_some_and(|second| second <= 6));
    }
}
