//! The building blocks every screen of NC-rs is made from: colors, sizes,
//! rows, buttons, inputs, menus and dialog frames. Generic over the message
//! type, so a feature crate maps them into its own messages.

/// The font of all text in the app: the window's default font and the base of
/// every weight variant, so a bold part never switches family.
pub const APP_FONT: iced::Font = iced::Font::MONOSPACE;

pub mod column_header;
pub mod file_row;
pub mod fkeys;
pub mod focus;
pub mod format;
pub mod frame;
pub mod hermes_kit;
pub mod hermes_state;
pub mod key_intercept;
pub mod kit;
pub mod layout;
pub mod list_window;
mod menu_button;
pub mod numbered_hint;
pub mod palette;
pub mod picker;
pub mod tags;
pub mod theme;
