//! The file panels: their state, the messages they send, `update` and `view`,
//! the column header, the folders they were in, the drag between the panels
//! and the list of search hits a panel can show.

pub mod action;
pub mod column_header;
pub mod ctx;
pub mod hit_list;
pub mod message;
pub mod panel;
pub mod panel_drag;
pub mod panel_history;
pub mod state;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, HermesCellOf, ViewCtx};
pub use message::PanelMessage;
pub use state::Panels;
pub use update::update;
pub use view::view;
