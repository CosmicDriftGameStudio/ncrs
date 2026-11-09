//! The folder comparison: the start dialog, the comparison of two folders, the
//! compare bar and the sync dialog. The marks go to the panels as plain data
//! (`ncrs_core::CompareMarks`); copying and deleting are asked of the app.

pub mod action;
pub mod core;
pub mod ctx;
pub mod message;
pub mod scan;
pub mod state;
pub mod update;
pub mod view;

#[cfg(test)]
mod update_tests;
#[cfg(test)]
mod view_ui_tests;

pub use action::Action;
pub use core::{Choices, Compared, Direction};
pub use ctx::Ctx;
pub use message::Message;
pub use state::Compare;
pub use update::{update, Outcome};
