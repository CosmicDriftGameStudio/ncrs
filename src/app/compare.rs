//! The folder comparison in the app: its dialogs and bar are the feature's
//! views; what it asks of the app (copying, deleting, the panel rows) is done here.

use crate::app::*;

impl App {
    pub(in crate::app) fn update_compare(&mut self, message: CompareMessage) -> Task<Message> {
        let blocking_overlay_open = self.blocking_overlay_is_open();
        let mut compare = std::mem::take(&mut self.compare);
        let (task, actions) = ncrs_compare::update(
            &mut compare,
            message,
            &ncrs_compare::Ctx {
                lang: self.lang,
                pool: &self.remote,
                left: &self.panel(PanelSide::Left).path,
                right: &self.panel(PanelSide::Right).path,
                blocking_overlay_open,
            },
        );
        self.compare = compare;
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_compare_action(action));
        }
        Task::batch(tasks)
    }

    fn run_compare_action(&mut self, action: ncrs_compare::Action) -> Task<Message> {
        use ncrs_compare::Action as Ask;
        match action {
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Ask::MarksChanged => {
                self.apply_compare_filter();
                Task::none()
            }
            Ask::Copy { sources, target } => self.with_ops(|ops, ctx| {
                ncrs_ops::start_validated_transfer(
                    ops,
                    TransferKind::Copy,
                    sources,
                    target,
                    ncrs_ops::Policy::Ask,
                    ctx,
                )
            }),
            Ask::Delete { side, sources } => {
                let request = ncrs_ops::DeleteRequest {
                    permanent: false,
                    sources,
                    side,
                    in_archive: false,
                };
                self.with_ops(|ops, ctx| ncrs_ops::open_delete(ops, request, ctx))
            }
        }
    }

    /// Nothing of the file operations is running or asking.
    fn ops_are_idle(&self) -> bool {
        !self.jobs.is_busy()
            && self.ops.transfer.is_none()
            && self.ops.transfer_dialog.is_none()
            && self.ops.deleting.is_none()
            && self.ops.delete_dialog.is_none()
            && !self.ops.conflict_is_open()
    }

    /// After every update: a comparison ends when a panel leaves its folder,
    /// a sync goes on when the copy queue is done, and a new listing of a
    /// panel gets the filter again.
    pub(in crate::app) fn follow_compare(&mut self) -> Task<Message> {
        if self.compare.run.is_none() && !self.compare.waits_for_ops() {
            return Task::none();
        }
        let mut tasks = vec![self.update_compare(CompareMessage::PanelsChanged)];
        if self.compare.waits_for_ops() && self.ops_are_idle() {
            tasks.push(self.update_compare(CompareMessage::OpsIdle));
        }
        self.apply_compare_filter();
        Task::batch(tasks)
    }

    /// Takes the rows that are the same out of both panels while "Only
    /// differences" is on, and puts them back when it is off.
    pub(in crate::app) fn apply_compare_filter(&mut self) {
        let rows = self.visible_rows;
        let filtering = self.compare.filters_rows();
        for side in [PanelSide::Left, PanelSide::Right] {
            let compare = &self.compare;
            let panel = self.panels.panel_mut(side);
            if filtering {
                if panel
                    .entries
                    .iter()
                    .any(|entry| !compare.shows_row(side, entry))
                {
                    panel.hide_rows_except(|entry| compare.shows_row(side, entry), rows);
                }
            } else if !panel.compare_hidden.is_empty() {
                panel.restore_hidden_rows(rows);
            }
        }
    }
}
