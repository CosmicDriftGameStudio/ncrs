//! The questions that open a dialog or a prompt for the selection of a panel:
//! the copy and move dialog, the pack prompt, the pasted file name.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_core::{FileEntry, PanelSide, TransferKind};
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::Msg;
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, TransferAction};
use crate::archive_flow::start_pack;
use crate::clipboard_paste::PastedContent;
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, with_front, Outcome};
use crate::message::{OpsMessage, TransferDialogMessage};
use crate::prompt::{Prompt, PromptKind};
use crate::state::Ops;
use crate::transfer::view::refusal_text;
use crate::transfer::{self, Policy};
use crate::transfer_flow::start_validated_transfer;
use crate::transfer_open::{conflicts_task, unreachable_server_text, ValidTransfer};

/// What a panel shows, as far as the questions need it.
#[derive(Debug, Clone)]
pub struct PanelFacts<'a> {
    pub side: PanelSide,
    pub path: &'a Path,
    /// In an archive or on a server.
    pub is_virtual: bool,
    pub hit_list: Option<HitListFacts<'a>>,
    pub entries: &'a [FileEntry],
}

/// The list of search hits a panel shows in place of a folder.
#[derive(Debug, Clone)]
pub struct HitListFacts<'a> {
    /// The folder the search ran in.
    pub scope: &'a Path,
    /// What the query suggests as the name of an archive of the hits.
    pub archive_name: Option<String>,
}

/// The panel the files come from, the other one, and the files.
#[derive(Debug)]
pub struct SelectionFacts<'a> {
    pub source: PanelFacts<'a>,
    pub other: PanelFacts<'a>,
    pub sources: Vec<PathBuf>,
}

/// The name a new archive starts with: the entry's own name for one, the name
/// of the folder they are in for several.
pub fn suggested_archive_name(sources: &[PathBuf]) -> String {
    let named = match sources {
        [only] => only.file_name(),
        many => many
            .first()
            .and_then(|first| first.parent())
            .and_then(Path::file_name),
    };
    format!(
        "{}.zip",
        named.map_or_else(|| "archive".into(), |name| name.to_string_lossy())
    )
}

/// F5 / F6: resolve what to transfer and ask before anything starts.
///
/// The rows are resolved here rather than at the confirmation because a
/// conflict dialog pauses the operation, and by then the selection may
/// have moved. With a transfer already running the key brings its progress
/// back instead: one transfer at a time owns the conflict state. A `preset`
/// fills the target field instead of the other panel's folder.
pub fn open_transfer_dialog(
    ops: &mut Ops,
    kind: TransferKind,
    preset: Option<PathBuf>,
    facts: SelectionFacts<'_>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if ops.transfer.is_some() {
        ops.transfer_mask_visible = true;
        return idle();
    }
    if ops.extracting.is_some() {
        let text = ctx.lang.text(Msg::StatusArchiveViewBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    if ops.transfer_dialog_is_blocked() {
        return idle();
    }
    if facts.sources.is_empty() {
        return idle();
    }
    if preset.is_none() && facts.other.hit_list.is_some() {
        let text = ctx.lang.text(Msg::StatusHitListNotAFolder).to_owned();
        return actions(vec![Action::ShowError(text)]);
    }
    let base = preset.unwrap_or_else(|| facts.other.path.to_path_buf());
    let remote_home = RemotePath::parse(&base).and_then(|place| {
        ctx.remote
            .session(place.server())
            .map(|session| session.home().to_owned())
    });
    let hit_scope = facts
        .source
        .hit_list
        .as_ref()
        .map(|list| list.scope.to_path_buf());
    let opened = ops
        .show_transfer_dialog(kind, facts.sources, base, (remote_home, hit_scope))
        .map(OpsMessage::TransferDialog);
    (opened, vec![Action::ClearStatus])
}

/// A message of the copy and move dialog.
pub fn update_transfer_dialog_flow(
    ops: &mut Ops,
    message: TransferDialogMessage,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let Some(action) = crate::update::update_transfer_dialog(&mut ops.transfer_dialog, message)
    else {
        return idle();
    };
    match action {
        TransferAction::CountConflicts => {
            let counting = ops
                .transfer_dialog
                .as_ref()
                .map_or_else(Task::none, |dialog| {
                    conflicts_task(dialog).map(OpsMessage::TransferDialog)
                });
            (counting, Vec::new())
        }
        TransferAction::Closed => (release_focus(), Vec::new()),
        TransferAction::Confirm => confirm_transfer(ops, ctx),
        TransferAction::HideProgress => {
            ops.transfer_mask_visible = false;
            idle()
        }
        TransferAction::ShowProgress => {
            ops.transfer_mask_visible = ops.transfer.is_some();
            idle()
        }
    }
}

/// Enter in the confirmation: refuse a target that cannot take the
/// selection, with the reason in the dialog, or start the transfer.
pub fn confirm_transfer(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    let Some(ValidTransfer {
        kind,
        sources,
        target,
        policy,
    }) = ops.validate_transfer_dialog()
    else {
        return idle();
    };
    if let Some(refusal) = unreachable_server_text(ctx.remote, &target, ctx.lang) {
        return actions(vec![Action::ShowError(refusal)]);
    }
    ops.transfer_dialog = None;
    let (started, asked) = start_validated_transfer(ops, kind, sources, target, policy, ctx);
    (Task::batch([release_focus(), started]), asked)
}

/// Copies files that come from outside the panels (dropped from another
/// app, pasted from the clipboard) into the active panel. They carry no
/// cursor position or modifier, so the target is always the active panel
/// and the transfer always a copy.
pub fn copy_external_files(
    ops: &mut Ops,
    paths: Vec<PathBuf>,
    active: PanelFacts<'_>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if active.hit_list.is_some() {
        let text = ctx.lang.text(Msg::StatusHitListNotAFolder).to_owned();
        return actions(vec![Action::ShowError(text)]);
    }
    if ops.transfer_into_panel_is_blocked(ops.transfer_dialog.is_some()) {
        let text = ctx.lang.text(Msg::StatusDropBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    let resolved = match transfer::validate(TransferKind::Copy, &paths, active.path) {
        Ok(resolved) => resolved,
        Err(refusal) => {
            let text = refusal_text(refusal, ctx.lang).to_owned();
            return actions(vec![Action::ShowError(text)]);
        }
    };
    if let Some(unreachable) = unreachable_server_text(ctx.remote, &resolved, ctx.lang) {
        return actions(vec![Action::ShowError(unreachable)]);
    }
    with_front(
        Action::ClearStatus,
        start_validated_transfer(ops, TransferKind::Copy, paths, resolved, Policy::Ask, ctx),
    )
}

/// What the clipboard held: files are copied like a drop, a picture or
/// text first asks for the name of the file it becomes.
pub fn paste_content(
    ops: &mut Ops,
    content: Option<PastedContent>,
    active: PanelFacts<'_>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    match content {
        None => {
            let text = ctx.lang.text(Msg::StatusClipboardEmpty).to_owned();
            actions(vec![Action::ShowNotice(text)])
        }
        Some(PastedContent::Files(files)) => copy_external_files(ops, files, active, ctx),
        Some(pasted) => ask_for_pasted_file_name(ops, pasted, active, ctx),
    }
}

/// The picture or text of a paste needs a name before it becomes a file.
fn ask_for_pasted_file_name(
    ops: &mut Ops,
    pasted: PastedContent,
    active: PanelFacts<'_>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if active.hit_list.is_some() {
        let text = ctx.lang.text(Msg::StatusHitListNotAFolder).to_owned();
        return actions(vec![Action::ShowError(text)]);
    }
    let Some(kind) = pasted.kind() else {
        return idle();
    };
    if ops.transfer_into_panel_is_blocked(ops.transfer_dialog.is_some()) {
        let text = ctx.lang.text(Msg::StatusPasteBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    let asked = ops.ask_for_pasted_file_name(
        pasted,
        kind,
        ctx.lang,
        (active.path.to_path_buf(), active.side),
        |candidate| active.entries.iter().any(|entry| entry.name == candidate),
    );
    (asked, Vec::new())
}

/// Enter in a prompt that starts a job: the pack prompt writes the archive,
/// the paste prompt copies the staged file.
pub fn start_prompt_job(ops: &mut Ops, active: PanelFacts<'_>, ctx: &FlowCtx<'_>) -> Outcome {
    match ops.prompt.as_ref().map(Prompt::kind) {
        Some(PromptKind::Pack) => {
            if ctx.jobs_busy {
                let text = ctx.lang.text(Msg::StatusArchiveWriteBusy).to_owned();
                return actions(vec![Action::ShowNotice(text)]);
            }
            let Some(request) = ops.take_pack_request(active.side) else {
                return idle();
            };
            start_pack(ops, request, ctx)
        }
        Some(PromptKind::Paste) => match ops.stage_paste() {
            Some(file) => copy_external_files(ops, vec![file], active, ctx),
            None => idle(),
        },
        Some(PromptKind::CreateDir | PromptKind::CreateFile) | None => idle(),
    }
}

/// Alt+F5: the name prompt for a new archive in the other panel's folder,
/// with the name of the single entry, or of the folder they are in, as a
/// start.
pub fn open_pack_prompt(ops: &mut Ops, facts: SelectionFacts<'_>, ctx: &FlowCtx<'_>) -> Outcome {
    if ctx.jobs_busy
        || ops.prompt.is_some()
        || ops.conflict_is_open()
        || ops.delete_dialog.is_some()
        || ops.transfer_dialog.is_some()
        || ops.transfer.is_some()
    {
        return idle();
    }
    let SelectionFacts {
        source,
        other,
        sources,
    } = facts;
    // An archive or a server in the other panel cannot take the new
    // archive, so it goes beside the files; the field shows where.
    let target = if other.is_virtual || other.hit_list.is_some() {
        &source
    } else {
        &other
    };
    if source.is_virtual {
        let text = ctx.lang.text(Msg::StatusPackUnavailable).to_owned();
        return actions(vec![Action::ShowError(text)]);
    }
    if sources.is_empty() {
        return idle();
    }
    let parent = target.path.to_path_buf();
    let prompt = match &source.hit_list {
        Some(list) => {
            let name = list
                .archive_name
                .clone()
                .unwrap_or_else(|| suggested_archive_name(&sources));
            Prompt::pack_hits(parent, sources, list.scope.to_path_buf(), &name)
        }
        None => {
            let name = suggested_archive_name(&sources);
            Prompt::pack(parent, sources, &name)
        }
    };
    (
        ops.open_prompt(prompt, target.side),
        vec![Action::ClearStatus],
    )
}
