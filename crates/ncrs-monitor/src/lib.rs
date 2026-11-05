//! The System page: CPU, memory, drives, network, sensors and the tasks of this
//! computer, and the short text of the badge in the status bar. Only `sampler`
//! knows where the numbers come from.

mod action;
mod ctx;
mod drives;
mod gate;
mod keys;
mod kill;
mod layout;
mod message;
mod sampler;
mod snapshot;
mod state;
mod status;
mod tasks;
mod units;
mod update;
mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use drives::DriveProbe;
pub use keys::{monitor_key, KeyFacts, MonitorKey};
pub use layout::visible_rows;
pub use message::MonitorMessage;
pub use snapshot::{
    CpuInfo, DriveRow, HostInfo, LoadInfo, NetworkRow, SensorRow, Snapshot, TaskCounts, TaskRow,
    TaskStatus, Usage,
};
pub use state::{State, KILL_OFFERED};
pub use status::status_text;
pub use tasks::TaskColumn;
pub use update::{tick_every, update};
pub use view::{dialog_view, menu_view, view, FILTER_ID};

#[cfg(test)]
mod tests;
