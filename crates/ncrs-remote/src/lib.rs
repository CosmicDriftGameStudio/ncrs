//! The SFTP connect feature: the connect dialog, the question about an unknown
//! host key, the passwords of the keychain and the texts of the connect
//! failures. The connections themselves live in `ncrs-fs`.

pub mod action;
pub mod ctx;
pub mod dialog;
pub mod message;
pub mod state;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::Ctx;
pub use message::{ConnectField, Message};
pub use state::Remote;
pub use update::{open_connect_dialog_for, update, Outcome};
