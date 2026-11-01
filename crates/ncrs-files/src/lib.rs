//! The file panels: their state, the messages they send, `update` and `view`,
//! the folders they were in, the drag between the panels
//! and the list of search hits a panel can show.

pub mod action;
pub mod context_menu;
pub mod context_menu_flow;
pub mod context_menu_view;
pub mod ctx;
pub mod folder_size;
pub mod hit_list;
pub mod hit_list_flow;
pub mod keys;
pub mod load_flow;
pub mod message;
pub mod open_plan;
pub mod panel;
pub mod panel_drag;
pub mod panel_history;
pub mod path_field;
pub mod reload;
pub mod state;
pub mod update;
pub mod view;
pub mod volume_flow;
pub mod volumes;
pub mod volumes_view;

pub use action::Action;
pub use ctx::{Ctx, HermesCellOf, ViewCtx};
pub use message::PanelMessage;
pub use reload::PendingReload;
pub use state::Panels;
pub use update::update;
pub use view::view;
pub use volume_flow::{server_rows, VolumeFacts};
