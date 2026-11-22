//! What lies over the Git tab: the context menus, the dialog for a new branch
//! and the notice about a push or login that failed.

use iced::widget::{column, stack};
use iced::{Element, Point};
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

use crate::actions::{ConflictAction, MenuAction, MenuTarget, NewBranch, Notice, OpenMenu, Side};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::update::menu_target_of;

const MENU_WIDTH: f32 = 300.0;
const NAME_ID: &str = "git-new-branch-name";

fn row(label: String, shortcut: Shortcut, message: Message, enabled: bool) -> MenuRow<Message> {
    MenuRow::Entry(PopupEntry {
        label,
        key: Some(shortcut.label()),
        is_selected: false,
        on_press: enabled.then_some(message),
        on_hover: None,
    })
}

fn entry(label: String, action: MenuAction, enabled: bool) -> MenuRow<Message> {
    row(
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
    };
    let mut buttons = vec![button(
        lang.text(Msg::GitViewNoticeDismiss),
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
    if let Some(open) = state.actions.menu {
        return stack![page, menu(state, open, ctx)].into();
    }
    page
}
