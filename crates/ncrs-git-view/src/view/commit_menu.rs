//! The menu of a commit (#369), the question before a hard reset and the
//! dialog that asks for a tag name.

use iced::widget::column;
use iced::{Element, Point};
use ncrs_git::ResetMode;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame};
use ncrs_ui_kit::kit::{button, dim, enter_button, error_line, paragraph, text_field, Tone};
use ncrs_ui_kit::popup_menu::{MenuRow, PopupEntry};
use ncrs_ui_kit::theme::{size, spacing};

use crate::actions::{OpenMenu, Question, TagDraft};
use crate::commit_menu::{self, CommitAction, ENTRIES, RESET_ENTRY, RESET_MODES};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;

use super::overlay::MENU_WIDTH;

pub const TAG_ID: &str = "git-tag-name";

/// How far right of the list's edge the menu hangs: under the message column.
const MENU_INSET: f32 = 60.0;

fn shortcut_of(action: CommitAction) -> Option<Shortcut> {
    match action {
        CommitAction::CopySha => Some(Shortcut::CopySha),
        CommitAction::Checkout => Some(Shortcut::Checkout),
        CommitAction::NewBranch => Some(Shortcut::NewBranch),
        CommitAction::CherryPick => Some(Shortcut::CherryPick),
        CommitAction::Tag
        | CommitAction::Revert
        | CommitAction::Reset(_)
        | CommitAction::CompareWorking
        | CommitAction::CompareMarked => None,
    }
}

/// The rows of the menu of the commit in `row`.
pub fn rows(state: &State, open: &OpenMenu, row: usize, ctx: ViewCtx<'_>) -> Vec<MenuRow<Message>> {
    let lang = ctx.lang;
    let branch = commit_menu::branch_label(state, lang);
    let with_branch = |msg: Msg| fill_template(lang.text(msg), &[("{branch}", &branch)]);
    let label_of = |action: CommitAction| match action {
        CommitAction::CopySha => lang.text(Msg::GitViewCommitMenuCopySha).to_owned(),
        CommitAction::Checkout => lang.text(Msg::GitViewCommitMenuCheckout).to_owned(),
        CommitAction::NewBranch => lang.text(Msg::GitViewCommitMenuNewBranch).to_owned(),
        CommitAction::Tag => lang.text(Msg::GitViewCommitMenuTag).to_owned(),
        CommitAction::CherryPick => with_branch(Msg::GitViewCommitMenuCherryPick),
        CommitAction::Revert => lang.text(Msg::GitViewCommitMenuRevert).to_owned(),
        CommitAction::Reset(ResetMode::Soft) => {
            lang.text(Msg::GitViewCommitMenuResetSoft).to_owned()
        }
        CommitAction::Reset(ResetMode::Mixed) => {
            lang.text(Msg::GitViewCommitMenuResetMixed).to_owned()
        }
        CommitAction::Reset(ResetMode::Hard) => {
            lang.text(Msg::GitViewCommitMenuResetHard).to_owned()
        }
        CommitAction::CompareWorking => lang.text(Msg::GitViewCommitMenuCompareWorking).to_owned(),
        CommitAction::CompareMarked => lang.text(Msg::GitViewCommitMenuCompareMarked).to_owned(),
    };
    let mut rows = Vec::new();
    for (index, slot) in ENTRIES.iter().enumerate() {
        if matches!(index, 4 | 6 | 7) {
            rows.push(MenuRow::Separator);
        }
        let Some(action) = *slot else {
            rows.push(reset_submenu(state, open, row, with_branch(Msg::GitViewCommitMenuReset), &label_of));
            continue;
        };
        let enabled = commit_menu::enabled(state, row, action);
        rows.push(MenuRow::Entry(PopupEntry {
            label: label_of(action),
            key: shortcut_of(action)
                .map(|shortcut| shortcut.label(ctx.keys))
                .filter(|key| !key.is_empty()),
            is_selected: open.cursor == index,
            on_press: enabled.then_some(Message::CommitAction(action)),
            on_hover: enabled.then_some(Message::MenuCursor(index)),
        }));
    }
    rows
}

fn reset_submenu(
    state: &State,
    open: &OpenMenu,
    row: usize,
    label: String,
    label_of: &impl Fn(CommitAction) -> String,
) -> MenuRow<Message> {
    let enabled = commit_menu::enabled(state, row, CommitAction::Reset(ResetMode::Soft));
    let mut children = Vec::new();
    for (index, mode) in RESET_MODES.iter().enumerate() {
        let action = CommitAction::Reset(*mode);
        if *mode == ResetMode::Hard {
            children.push(MenuRow::Separator);
        }
        let entry = PopupEntry {
            label: label_of(action),
            key: None,
            is_selected: open.submenu == Some(index),
            on_press: enabled.then_some(Message::CommitAction(action)),
            on_hover: enabled.then_some(Message::MenuSubmenuCursor(index)),
        };
        children.push(if *mode == ResetMode::Hard {
            MenuRow::Danger(entry)
        } else {
            MenuRow::Entry(entry)
        });
    }
    MenuRow::Submenu(
        PopupEntry {
            label,
            key: None,
            is_selected: open.cursor == RESET_ENTRY && open.submenu.is_some(),
            on_press: enabled.then_some(Message::MenuCursor(RESET_ENTRY)),
            on_hover: enabled.then_some(Message::MenuCursor(RESET_ENTRY)),
        },
        children,
    )
}

/// Where the menu of the commit in `row` hangs: under its row, where the list is scrolled to.
pub fn anchor(state: &State, row: usize, ctx: ViewCtx<'_>) -> Point {
    let list_top = super::body_top(state) + size::ROW + spacing::BORDER_WIDTH + spacing::XL;
    let y = list_top + (row + 1) as f32 * size::ROW - state.commits_scroll;
    let x = super::sidebar_width(ctx, state) + size::SPLITTER + MENU_INSET;
    Point::new(
        x.min(ctx.window.width - MENU_WIDTH).max(0.0),
        y.max(list_top),
    )
}

/// The question before a hard reset: Enter presses Cancel, the red button needs a click.
pub fn reset_dialog<'a>(question: &'a Question, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let Question::ResetHard {
        branch,
        sha,
        commits,
        changed_files,
    } = question
    else {
        return column![].into();
    };
    let lang = ctx.lang;
    let short: String = sha.chars().take(7).collect();
    let text = fill_template(
        lang.text(Msg::GitViewAskResetHardText),
        &[
            ("{branch}", branch),
            ("{sha}", &short),
            ("{commits}", &commits.to_string()),
            ("{files}", &changed_files.to_string()),
        ],
    );
    Frame::new(
        fill_template(
            lang.text(Msg::GitViewAskResetHardTitle),
            &[("{branch}", branch)],
        ),
        paragraph(text, ctx.palette),
        DialogSize::Medium,
        ctx.window,
        lang,
        ctx.palette,
    )
    .buttons(vec![
        enter_button(
            lang.text(Msg::GitViewDialogCancel),
            Some(Message::CloseDialog),
            Tone::Secondary,
            ctx.palette,
        ),
        button(
            lang.text(Msg::GitViewAskResetHardButton),
            Some(Message::ConfirmDialog),
            Tone::Danger,
            ctx.palette,
        ),
    ])
    .on_close(Message::CloseDialog)
    .into_element()
}

pub fn tag_dialog<'a>(draft: &'a TagDraft, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let short: String = draft.sha.chars().take(7).collect();
    let mut name_field = column![
        paragraph(lang.text(Msg::GitViewTagName), palette),
        text_field(
            lang.text(Msg::GitViewTagNamePlaceholder),
            &draft.name,
            TAG_ID,
            Some(Message::TagNameChanged),
            Message::CreateTag,
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
            fill_template(lang.text(Msg::GitViewTagAt), &[("{sha}", &short)]),
            palette
        ),
    ]
    .spacing(spacing::M);
    let ready = draft.problem.is_none() && !draft.name.trim().is_empty();
    Frame::new(
        lang.text(Msg::GitViewTagTitle),
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
            lang.text(Msg::GitViewTagCreate),
            ready.then_some(Message::CreateTag),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(Message::CloseDialog)
    .into_element()
}
