//! The file panels: their state, the messages they send, `update` and `view`,
//! the folders they were in, the drag between the panels
//! and the list of search hits a panel can show.

pub mod action;
#[cfg(test)]
mod compare_column_ui_tests;
pub mod context_menu;
pub mod context_menu_flow;
pub mod context_menu_view;
pub mod ctx;
pub mod favourites;
pub mod folder_size;
pub mod folder_watch;
pub mod header_drag;
pub mod header_menu;
pub mod hit_list;
pub mod hit_list_flow;
pub mod keys;
pub mod load_flow;
pub mod message;
pub mod open_plan;
pub mod panel;
pub mod panel_drag;
pub mod panel_history;
pub mod panel_totals;
pub mod path_field;
pub mod reload;
pub mod rename;
pub mod special_folders;
pub mod state;
pub mod update;
pub mod view;
pub mod volume_flow;
pub mod volumes;
pub mod volumes_view;

pub use action::Action;
pub use ctx::{ColumnSettings, Ctx, HermesCellOf, ViewCtx};
pub use header_menu::{header_menu_view, HeaderMenu};
pub use message::PanelMessage;
pub use reload::PendingReload;
pub use state::Panels;
pub use update::update;
pub use view::view;
pub use volume_flow::{server_rows, VolumeFacts};
