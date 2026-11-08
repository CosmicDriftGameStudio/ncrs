//! The small building blocks of the dialogs, in the manner of React
//! components: every function takes what it shows and returns an `Element`,
//! and every color comes from the palette. A dialog is put together from these
//! and the [`Frame`](super::frame::Frame); it does not style widgets itself.

mod buttons;
mod fields;
mod icons;
mod lines;
mod summary;
mod tables;

pub use buttons::*;
pub use fields::*;
pub use icons::*;
pub use lines::*;
pub use summary::*;
pub use tables::*;
