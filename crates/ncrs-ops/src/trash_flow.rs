//! Showing the trash in place of a panel: closing it and reading it again.

use std::sync::Arc;

use iced::Task;
use ncrs_fs::{Trash, TrashReach};
use ncrs_i18n::Msg;

use crate::message::TrashMessage;
use crate::state::Ops;

impl Ops {
    pub fn close_trash(&mut self) {
        self.trash_view = None;
        self.trash_generation = self.trash_generation.wrapping_add(1);
    }

    /// Reads the trash on the blocking pool; the answer carries the generation,
    /// so a late one finds the view closed or read again.
    pub fn list_trash(&mut self, source: &Arc<dyn Trash>) -> Task<TrashMessage> {
        let Some(view) = self.trash_view.as_mut() else {
            return Task::none();
        };
        view.begin_reading();
        self.trash_generation = self.trash_generation.wrapping_add(1);
        let generation = self.trash_generation;
        let trash = Arc::clone(source);
        Task::perform(
            tokio::task::spawn_blocking(move || trash.list()),
            move |outcome| TrashMessage::Listed {
                generation,
                result: outcome.unwrap_or_else(|join| Err(join.to_string())),
            },
        )
    }
}

/// What the last row of the drive menu is called, if this build has one.
pub fn menu_row(reach: TrashReach) -> Option<Msg> {
    match reach {
        TrashReach::Everything => Some(Msg::TrashTitleEverything),
        TrashReach::OwnDeletions => Some(Msg::TrashTitleOwn),
        TrashReach::FinderOnly => cfg!(target_os = "macos").then_some(Msg::ActionOpenTrashInFinder),
    }
}
