use std::fmt;
use std::time::SystemTime;

/// Wall-clock time as the pipeline sees it: the stability check of the intake,
/// the start of a job, error times and the document rate. Waiting and
/// scheduling use `Instant`, not this.
pub trait Clock: Send + Sync + fmt::Debug {
    fn now(&self) -> SystemTime;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
