//! The update check: how this copy was installed, the look-up of the newest
//! release once a day, the Updates page and "skip this version".

pub mod channel;
pub mod fetch;
pub mod release;
pub mod state;
pub mod store;
pub mod view;
#[cfg(test)]
mod view_tests;

pub use channel::{Channel, PackageManager};
pub use fetch::{latest_release, FetchFn};
pub use release::LatestRelease;
pub use state::{update, Action, Ctx, Message, State, CHECK_AVAILABLE};
pub use store::file_beside;
pub use view::{keys, view, ViewCtx, COMMAND_FIELD_ID};
