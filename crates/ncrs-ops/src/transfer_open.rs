//! Opening the copy and move dialog, the counting it starts and the check
//! before Enter starts the transfer.

use std::path::PathBuf;

use iced::widget::operation;
use iced::Task;
use ncrs_core::TransferKind;
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::message::TransferDialogMessage;
use crate::state::Ops;
use crate::transfer::view::TARGET_FIELD_ID;
use crate::transfer::{self, Policy, TransferDialog};

/// Counts what the selection of `dialog` holds, off the UI thread. The dialog's
/// stop flag ends the walk when the dialog goes away.
pub fn scan_task(dialog: &TransferDialog) -> Task<TransferDialogMessage> {
    // Nothing on a server is counted: that would be a listing of the whole
    // tree before the copy.
    if dialog.counts_unknown() {
        return Task::none();
    }
    let (id, sources, stop) = (dialog.id(), dialog.sources().to_vec(), dialog.stop_flag());
    Task::perform(
        tokio::task::spawn_blocking(move || transfer::scan(&sources, &stop)),
        move |done| TransferDialogMessage::Scanned {
            dialog: id,
            scan: done.ok(),
        },
    )
}

/// Counts which top-level names of the selection the typed target already
/// has, off the UI thread. The result carries the edit it was counted for.
pub fn conflicts_task(dialog: &TransferDialog) -> Task<TransferDialogMessage> {
    if dialog.involves_server() {
        return Task::none();
    }
    let (id, edit, stop) = (dialog.id(), dialog.edit(), dialog.stop_flag());
    let (sources, target) = (dialog.sources().to_vec(), dialog.target_path());
    Task::perform(
        tokio::task::spawn_blocking(move || transfer::count_conflicts(&sources, &target, &stop)),
        move |counted| TransferDialogMessage::ConflictsCounted {
            dialog: id,
            edit,
            count: counted.unwrap_or_default(),
        },
    )
}

/// What the open copy and move dialog is asked to start.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidTransfer {
    pub kind: TransferKind,
    pub sources: Vec<PathBuf>,
    pub target: PathBuf,
    pub policy: Policy,
}

impl Ops {
    /// A job runs or a dialog is open, so the copy and move dialog may not
    /// open.
    pub fn transfer_dialog_is_blocked(&self) -> bool {
        self.prompt.is_some()
            || self.conflict_is_open()
            || self.delete_dialog.is_some()
            || self.transfer_dialog.is_some()
            || self.archive_write.is_some()
    }

    /// Shows the dialog for `sources` with the target field on `base`, and
    /// starts the counting.
    pub fn show_transfer_dialog(
        &mut self,
        kind: TransferKind,
        sources: Vec<PathBuf>,
        base: PathBuf,
        (remote_home, hit_scope): (Option<String>, Option<PathBuf>),
    ) -> Task<TransferDialogMessage> {
        self.transfer_dialogs_opened = self.transfer_dialogs_opened.wrapping_add(1);
        let dialog = TransferDialog::new(self.transfer_dialogs_opened, kind, sources, base)
            .with_remote_home(remote_home)
            .with_hit_scope(hit_scope);
        let scan = scan_task(&dialog);
        let conflicts = conflicts_task(&dialog);
        self.transfer_dialog = Some(dialog);
        Task::batch([
            scan,
            conflicts,
            operation::focus(TARGET_FIELD_ID).chain(operation::move_cursor_to_end(TARGET_FIELD_ID)),
        ])
    }

    /// Enter in the confirmation: refuses a target that cannot take the
    /// selection, with the reason in the dialog, or gives what to start.
    pub fn validate_transfer_dialog(&mut self) -> Option<ValidTransfer> {
        let open = self.transfer_dialog.as_mut()?;
        let target = match transfer::validate(open.kind(), open.sources(), &open.target_path()) {
            Ok(target) => target,
            Err(refusal) => {
                open.refuse(refusal);
                return None;
            }
        };
        Some(ValidTransfer {
            kind: open.kind(),
            sources: open.sources().to_vec(),
            target,
            policy: open.policy(),
        })
    }
}

/// What to tell the user when `target` is on a server that is not connected;
/// `None` when it is reachable.
pub fn unreachable_server_text(
    remote: &Pool,
    target: &std::path::Path,
    lang: Language,
) -> Option<String> {
    let place = RemotePath::parse(target)?;
    if remote.is_connected(place.server()) {
        return None;
    }
    Some(fill_template(
        lang.text(Msg::ErrorRemoteDisconnected),
        &[("{server}", &place.server().to_string())],
    ))
}
