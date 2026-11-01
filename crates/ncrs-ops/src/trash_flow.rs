//! Showing the trash in place of a panel: closing it and reading it again.

use std::sync::Arc;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs::{Trash, TrashReach};
use ncrs_i18n::Msg;
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, TrashAction};
use crate::ctx::{Ctx, FlowCtx};
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::TrashMessage;
use crate::restore_flow::{
    restore_last_deletion, restore_listed_deletion, restore_row_done, start_restore,
};
use crate::state::Ops;
use crate::trash::TrashView;
use crate::update::update_trash;

/// A message of the trash view, and what it starts.
pub fn update_trash_flow(
    ops: &mut Ops,
    message: TrashMessage,
    rows: usize,
    flow: &FlowCtx<'_>,
) -> Outcome {
    let ctx = Ctx {
        trash_rows: rows,
        trash_generation: ops.trash_generation,
    };
    let Some(action) = update_trash(ops.trash_view.as_mut(), message, &ctx) else {
        return idle();
    };
    match action {
        TrashAction::Open => actions(vec![Action::OpenTrash(flow.source_side)]),
        TrashAction::Close => {
            ops.close_trash();
            (release_focus(), Vec::new())
        }
        TrashAction::Restore(items) => start_restore(ops, items, flow),
        TrashAction::RestoreRowDone {
            result,
            index,
            generation,
        } => restore_row_done(ops, result, index, generation, flow),
        TrashAction::RestoreLast => restore_last_deletion(ops, flow),
        TrashAction::LastListed { generation, result } => {
            restore_listed_deletion(ops, generation, result, flow)
        }
        TrashAction::OpenInFinder => actions(vec![Action::OpenTrashInFinder]),
    }
}

impl Ops {
    pub fn close_trash(&mut self) {
        self.trash_view = None;
        self.trash_generation = self.trash_generation.wrapping_add(1);
    }

    /// Shows the trash in place of the panel on `side` and reads it.
    pub fn show_trash(
        &mut self,
        side: PanelSide,
        reach: TrashReach,
        source: &Arc<dyn Trash>,
    ) -> Task<TrashMessage> {
        self.trash_side = side;
        self.trash_view = Some(TrashView::new(reach));
        self.list_trash(source)
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
