//! Renaming several entries by a pattern with a preview, and taking the last
//! batch back.

pub mod flow;
pub mod pattern;
pub mod state;
pub mod view;

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod hermes_tests;
#[cfg(test)]
mod view_ui_tests;
