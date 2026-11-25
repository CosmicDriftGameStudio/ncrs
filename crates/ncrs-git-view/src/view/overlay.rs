//! What lies over the Git tab: the context menus, the dialog for a new branch
//! and the notice about a push or login that failed.

use iced::widget::{column, stack};
use iced::{Element, Point};
use ncrs_git::PullStrategy;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::{banner, BannerTone};
use ncrs_ui_kit::frame::{DialogSize, Frame};
use ncrs_ui_kit::kit::{
    button, dim, enter_button, error_line, labelled_checkbox, paragraph, scrim, text_field, Check,
    Tone,
};
use ncrs_ui_kit::popup_menu::{
    popup_layer, popup_menu_rows, popup_rows_height, MenuRow, PopupEntry, RightPress,
};
use ncrs_ui_kit::theme::{size, spacing};
use ncrs_ui_kit::tree_list::{LIST_INSET, ROW_OVERHEAD, TREE_INDENT};

use crate::actions::{
    ConflictAction, Dialog, MenuAction, MenuTarget, NewBranch, Notice, OpenMenu, Question,
    RenameBranch, Side,
};
use crate::ctx::ViewCtx;
use crate::git_keys::GitKeys;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::update::menu_target_of;

const MENU_WIDTH: f32 = 300.0;
pub const NAME_ID: &str = "git-new-branch-name";
pub const RENAME_ID: &str = "git-rename-branch-name";

fn row(
    keys: &GitKeys,
    label: String,
    shortcut: Shortcut,
    message: Message,
    enabled: bool,
) -> MenuRow<Message> {
    MenuRow::Entry(PopupEntry {
        label,
        key: Some(shortcut.label(keys)).filter(|key| !key.is_empty()),
        is_selected: false,
        on_press: enabled.then_some(message),
        on_hover: None,
    })
}

fn entry(keys: &GitKeys, label: String, action: MenuAction, enabled: bool) -> MenuRow<Message> {
    row(
        keys,
        label,
        Shortcut::of_menu(action),
        Message::MenuPressed(action),
        enabled,
    )
}

fn checked_out(state: &State) -> String {
    state
        .overview
        .as_ref()
        .and_then(|overview| overview.local.iter().find(|branch| branch.is_current))
        .map(|branch| branch.name.clone())
        .unwrap_or_default()
}

fn rows_of(state: &State, menu: OpenMenu, ctx: ViewCtx<'_>) -> Vec<MenuRow<Message>> {
    let lang = ctx.lang;
    let text = |msg| lang.text(msg).to_owned();
    let entry = |label, action, enabled| entry(ctx.keys, label, action, enabled);
    let current = checked_out(state);
    let with_current = |msg| fill_template(lang.text(msg), &[("{branch}", &current)]);
    match menu.target {
        MenuTarget::LocalBranch(index) => {
            let branch = state
                .overview
                .as_ref()
                .and_then(|overview| overview.local.get(index));
            let is_current = branch.is_some_and(|found| found.is_current);
            let ahead = branch.is_some_and(|found| found.ahead > 0);
            let mut rows = vec![entry(
                text(Msg::GitViewMenuCheckout),
                MenuAction::Checkout,
                !is_current,
            )];
            if ahead {
                rows.push(entry(text(Msg::GitViewMenuPush), MenuAction::Push, true));
            }
            rows.extend([
                entry(
                    text(Msg::GitViewMenuNewBranchFrom),
                    MenuAction::NewBranchFrom,
                    true,
                ),
                entry(text(Msg::GitViewMenuRename), MenuAction::Rename, true),
                entry(
                    text(Msg::GitViewMenuDelete),
                    MenuAction::Delete,
                    !is_current,
                ),
                MenuRow::Separator,
                entry(
                    with_current(Msg::GitViewMenuMergeInto),
                    MenuAction::MergeInto,
                    !is_current,
                ),
                entry(
                    with_current(Msg::GitViewMenuRebaseOnto),
                    MenuAction::RebaseOnto,
                    !is_current,
                ),
            ]);
            rows
        }
        MenuTarget::RemoteBranch(_) => vec![
            entry(
                text(Msg::GitViewMenuCheckoutAsLocal),
                MenuAction::CheckoutAsLocal,
                true,
            ),
            entry(
                text(Msg::GitViewMenuNewBranchFrom),
                MenuAction::NewBranchFrom,
                true,
            ),
            MenuRow::Separator,
            entry(
                with_current(Msg::GitViewMenuMergeInto),
                MenuAction::MergeInto,
                true,
            ),
            entry(
                text(Msg::GitViewMenuDeleteOnRemote),
                MenuAction::DeleteOnRemote,
                true,
            ),
        ],
        MenuTarget::Stash(_) => vec![
            entry(
                text(Msg::GitViewMenuStashApply),
                MenuAction::StashApply,
                true,
            ),
            entry(
                text(Msg::GitViewMenuStashApplyDrop),
                MenuAction::StashApplyAndDrop,
                true,
            ),
            MenuRow::Separator,
            entry(text(Msg::GitViewMenuDelete), MenuAction::StashDrop, true),
        ],
        MenuTarget::Conflict(index) => {
            let conflict = state
                .actions
                .merge
                .as_ref()
                .and_then(|merge| merge.conflicts.get(index));
            let resolved = conflict.is_some_and(|found| found.resolved);
            let edited = conflict.is_some_and(|found| found.edited);
            let choice = |msg, action: ConflictAction, enabled| {
                row(
                    ctx.keys,
                    text(msg),
                    Shortcut::of_conflict(action),
                    Message::ConflictPressed(index, action),
                    enabled,
                )
            };
            vec![
                choice(
                    Msg::GitViewConflictTakeMine,
                    ConflictAction::Take(Side::Mine),
                    !resolved,
                ),
                choice(
                    Msg::GitViewConflictTakeTheirs,
                    ConflictAction::Take(Side::Theirs),
                    !resolved,
                ),
                choice(
                    Msg::GitViewConflictTakeBoth,
                    ConflictAction::Take(Side::Both),
                    !resolved,
                ),
                MenuRow::Separator,
                choice(
                    Msg::GitViewConflictOpenEditor,
                    ConflictAction::OpenEditor,
                    true,
                ),
                choice(
                    Msg::GitViewConflictMarkResolved,
                    ConflictAction::MarkResolved,
                    edited && !resolved,
                ),
                choice(Msg::GitViewConflictReset, ConflictAction::Reset, resolved),
            ]
        }
    }
}

/// Where the menu of `target` hangs: under its row, in the left list or among
/// the files in conflict.
fn anchor(state: &State, target: MenuTarget, ctx: ViewCtx<'_>) -> Point {
    let top = super::body_top(state);
    match target {
        MenuTarget::Conflict(index) => Point::new(
            ctx.window.width - super::detail_width(state, ctx) + ROW_OVERHEAD,
            top + (index + 1) as f32 * size::ROW_TALL,
        ),
        _ => {
            let row = state
                .sidebar_items()
                .iter()
                .position(|item| menu_target_of(*item) == Some(target))
                .unwrap_or_default();
            Point::new(
                LIST_INSET + ROW_OVERHEAD + 2.0 * TREE_INDENT,
                top + LIST_INSET + (row + 1) as f32 * size::ROW,
            )
        }
    }
}

fn menu<'a>(state: &State, open: OpenMenu, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let rows = rows_of(state, open, ctx);
    let separators = rows
        .iter()
        .filter(|line| matches!(line, MenuRow::Separator))
        .count();
    let height = popup_rows_height(rows.len() - separators, separators);
    let mut origin = anchor(state, open.target, ctx);
    origin.y = origin.y.min(ctx.window.height - height).max(0.0);
    popup_layer(
        popup_menu_rows(rows, MENU_WIDTH, origin, ctx.palette),
        Some(Message::CloseMenu),
        RightPress::Dismiss,
    )
}

fn new_branch_dialog<'a>(draft: &'a NewBranch, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let mut name_field = column![
        paragraph(lang.text(Msg::GitViewNewBranchName), palette),
        text_field(
            lang.text(Msg::GitViewNewBranchNamePlaceholder),
            &draft.name,
            NAME_ID,
            Some(Message::NewBranchNameChanged),
            Message::CreateBranch,
            palette,
        ),
    ]
    .spacing(spacing::XS);
    if let Some(problem) = &draft.problem {
        name_field = name_field.push(error_line(problem.as_str(), palette));
    }
    let content = column![
        name_field,
        dim(
            fill_template(
                lang.text(Msg::GitViewNewBranchFrom),
                &[("{from}", &draft.from)],
            ),
            palette,
        ),
        labelled_checkbox(
            if draft.checkout {
                Check::On
            } else {
                Check::Off
            },
            Some(Message::ToggleNewBranchCheckout),
            Some(lang.text(Msg::GitViewNewBranchCheckout)),
            palette,
        ),
    ]
    .spacing(spacing::M);
    Frame::new(
        lang.text(Msg::GitViewNewBranchTitle),
        content,
        DialogSize::Medium,
        ctx.window,
        lang,
        palette,
    )
    .buttons(vec![
        button(
            lang.text(Msg::GitViewDialogCancel),
            Some(Message::CloseDialog),
            Tone::Secondary,
            palette,
        ),
        enter_button(
            lang.text(Msg::GitViewNewBranchCreate),
            draft.problem.is_none().then_some(Message::CreateBranch),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(Message::CloseDialog)
    .into_element()
}

/// The dialog with a cancel button and a confirm button that Enter presses.
fn dialog_frame<'a>(
    title: &str,
    content: Element<'a, Message>,
    confirm: &str,
    confirm_message: Option<Message>,
    tone: Tone,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    Frame::new(
        title,
        content,
        DialogSize::Medium,
        ctx.window,
        lang,
        palette,
    )
    .buttons(vec![
        button(
            lang.text(Msg::GitViewDialogCancel),
            Some(Message::CloseDialog),
            Tone::Secondary,
            palette,
        ),
        enter_button(confirm, confirm_message, tone, palette),
    ])
    .on_close(Message::CloseDialog)
    .into_element()
}

fn question_dialog<'a>(question: &'a Question, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let (title, text, button_label) = match question {
        Question::DiscardFile { path } => (
            Msg::GitViewAskDiscardFileTitle,
            fill_template(
                lang.text(Msg::GitViewAskDiscardFileText),
                &[("{name}", path)],
            ),
            Msg::GitViewAskDiscardButton,
        ),
        Question::DiscardAll { count } => (
            Msg::GitViewAskDiscardAllTitle,
            fill_template(
                lang.text(Msg::GitViewAskDiscardAllText),
                &[("{count}", &count.to_string())],
            ),
            Msg::GitViewAskDiscardButton,
        ),
        Question::DeleteBranch { name } => (
            Msg::GitViewAskDeleteBranchTitle,
            fill_template(
                lang.text(Msg::GitViewAskDeleteBranchText),
                &[("{name}", name)],
            ),
            Msg::GitViewAskDeleteButton,
        ),
        Question::DeleteOnRemote { remote, branch } => (
            Msg::GitViewAskDeleteRemoteTitle,
            fill_template(
                lang.text(Msg::GitViewAskDeleteRemoteText),
                &[("{branch}", branch), ("{remote}", remote)],
            ),
            Msg::GitViewAskDeleteButton,
        ),
        Question::DropStash { message, .. } => (
            Msg::GitViewAskDropStashTitle,
            fill_template(
                lang.text(Msg::GitViewAskDropStashText),
                &[("{name}", message)],
            ),
            Msg::GitViewAskDeleteButton,
        ),
    };
    dialog_frame(
        lang.text(title),
        paragraph(text, ctx.palette),
        lang.text(button_label),
        Some(Message::ConfirmDialog),
        Tone::Danger,
        ctx,
    )
}

fn rename_dialog<'a>(rename: &'a RenameBranch, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let content = column![
        paragraph(lang.text(Msg::GitViewRenameLabel), palette),
        text_field(
            "",
            &rename.name,
            RENAME_ID,
            Some(Message::RenameNameChanged),
            Message::ConfirmDialog,
            palette,
        ),
    ]
    .spacing(spacing::XS);
    let changed = !rename.name.trim().is_empty() && rename.name.trim() != rename.from;
    dialog_frame(
        lang.text(Msg::GitViewRenameTitle),
        content.into(),
        lang.text(Msg::GitViewRenameButton),
        changed.then_some(Message::ConfirmDialog),
        Tone::Primary,
        ctx,
    )
}

pub fn notice_banner<'a>(notice: &'a Notice, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let (headline, text, details, actions) = match notice {
        Notice::PushRejected { branch, behind } => (
            lang.text(Msg::GitViewPushRejected).to_owned(),
            fill_template(
                lang.text(Msg::GitViewPushRejectedText),
                &[("{branch}", branch), ("{count}", &behind.to_string())],
            ),
            None,
            vec![button(
                lang.text(Msg::GitViewPushRejectedAction),
                Some(Message::PullFirst),
                Tone::Primary,
                palette,
            )],
        ),
        Notice::AuthFailed {
            remote,
            output,
            details_open,
        } => (
            fill_template(lang.text(Msg::GitViewAuthFailed), &[("{remote}", remote)]),
            lang.text(Msg::GitViewAuthFailedText).to_owned(),
            details_open.then(|| output.clone()),
            vec![
                button(
                    lang.text(Msg::GitViewNoticeDetails),
                    Some(Message::ToggleNoticeDetails),
                    Tone::Secondary,
                    palette,
                ),
                button(
                    lang.text(Msg::GitViewAuthFailedRetry),
                    Some(Message::Retry),
                    Tone::Primary,
                    palette,
                ),
            ],
        ),
        Notice::NetworkFailed {
            output,
            details_open,
        } => (
            lang.text(Msg::GitViewNetworkFailed).to_owned(),
            lang.text(Msg::GitViewNetworkFailedText).to_owned(),
            details_open.then(|| output.clone()),
            vec![
                button(
                    lang.text(Msg::GitViewNoticeDetails),
                    Some(Message::ToggleNoticeDetails),
                    Tone::Secondary,
                    palette,
                ),
                button(
                    lang.text(Msg::GitViewAuthFailedRetry),
                    Some(Message::Retry),
                    Tone::Primary,
                    palette,
                ),
            ],
        ),
        Notice::BranchNotMerged { branch } => (
            lang.text(Msg::GitViewBranchNotMerged).to_owned(),
            fill_template(
                lang.text(Msg::GitViewBranchNotMergedText),
                &[("{branch}", branch)],
            ),
            None,
            vec![button(
                lang.text(Msg::GitViewBranchNotMergedAction),
                Some(Message::DeleteBranchAnyway),
                Tone::Danger,
                palette,
            )],
        ),
        Notice::NeedsPullStrategy => (
            lang.text(Msg::GitViewNeedsStrategy).to_owned(),
            lang.text(Msg::GitViewNeedsStrategyText).to_owned(),
            None,
            vec![
                button(
                    lang.text(Msg::GitViewNeedsStrategyMerge),
                    Some(Message::PullWith(PullStrategy::Merge)),
                    Tone::Secondary,
                    palette,
                ),
                button(
                    lang.text(Msg::GitViewNeedsStrategyRebase),
                    Some(Message::PullWith(PullStrategy::Rebase)),
                    Tone::Primary,
                    palette,
                ),
            ],
        ),
        Notice::NothingToCommit => (
            lang.text(Msg::GitViewNothingToCommit).to_owned(),
            lang.text(Msg::GitViewNothingToCommitText).to_owned(),
            None,
            Vec::new(),
        ),
        Notice::CommitTitleMissing => (
            lang.text(Msg::GitViewCommitTitleMissing).to_owned(),
            lang.text(Msg::GitViewCommitTitleMissingText).to_owned(),
            None,
            Vec::new(),
        ),
        Notice::UnresolvedConflicts { count } => (
            lang.text(Msg::GitViewUnresolvedConflicts).to_owned(),
            fill_template(
                lang.text(Msg::GitViewUnresolvedConflictsText),
                &[("{count}", &count.to_string())],
            ),
            None,
            Vec::new(),
        ),
        Notice::UndoNotSafe => (
            lang.text(Msg::GitViewUndoNotSafe).to_owned(),
            lang.text(Msg::GitViewUndoNotSafeText).to_owned(),
            None,
            Vec::new(),
        ),
        Notice::InvalidName { name } => (
            lang.text(Msg::GitViewInvalidName).to_owned(),
            fill_template(lang.text(Msg::GitViewInvalidNameText), &[("{name}", name)]),
            None,
            Vec::new(),
        ),
        Notice::Other {
            output,
            details_open,
        } => (
            lang.text(Msg::GitViewOtherFailed).to_owned(),
            lang.text(Msg::GitViewOtherFailedText).to_owned(),
            details_open.then(|| output.clone()),
            vec![button(
                lang.text(Msg::GitViewNoticeDetails),
                Some(Message::ToggleNoticeDetails),
                Tone::Secondary,
                palette,
            )],
        ),
    };
    let asks = matches!(
        notice,
        Notice::BranchNotMerged { .. } | Notice::NeedsPullStrategy
    );
    let mut buttons = vec![button(
        lang.text(if asks {
            Msg::GitViewDialogCancel
        } else {
            Msg::GitViewNoticeDismiss
        }),
        Some(Message::DismissNotice),
        Tone::Secondary,
        palette,
    )];
    buttons.extend(actions);
    banner(
        BannerTone::Failure,
        headline,
        Some(text),
        details,
        buttons,
        palette,
    )
}

/// The page with the menu or the dialog over it, if one is open.
pub fn over<'a>(
    page: Element<'a, Message>,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    if let Some(draft) = &state.actions.new_branch {
        return stack![page, scrim(new_branch_dialog(draft, ctx))].into();
    }
    match &state.actions.dialog {
        Some(Dialog::Ask(question)) => {
            return stack![page, scrim(question_dialog(question, ctx))].into();
        }
        Some(Dialog::Rename(rename)) => {
            return stack![page, scrim(rename_dialog(rename, ctx))].into();
        }
        None => {}
    }
    if let Some(open) = state.actions.menu {
        return stack![page, menu(state, open, ctx)].into();
    }
    page
}
