//! Takes documents through the processing steps on a pool of worker threads.
//! The size of the pool follows a load setting that can change while it runs.
//! Nothing here reads document content except the intake's hashing, and
//! nothing leaves the machine unless an AI provider is given to
//! `intake_steps`; the provider then asks for consent before it sends.

mod ai_step;
mod clock;
mod load;
mod runner;
mod step;
mod steps;

pub use clock::{Clock, SystemClock};
pub use load::Load;
pub use runner::{
    Busy, ChangeOutcome, ChangedPath, ErrorEntry, Runner, RunnerState, Settings, Snapshot,
    StepSnapshot, Task, WorkerSnapshot, MAX_CHANGED_PATHS, MAX_ERRORS, RATE_MINUTES,
};
pub use step::{with_index_value, IndexAccess, Item, Step, StepOutcome};
pub use steps::{intake_steps, OwnDetails};
