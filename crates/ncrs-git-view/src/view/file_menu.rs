//! The menu of a file: its rows, and where it hangs.

use iced::Point;
use ncrs_i18n::Msg;
use ncrs_ui_kit::format::estimated_width;
use ncrs_ui_kit::popup_menu::{MenuRow, PopupEntry};
use ncrs_ui_kit::theme::{font_size, size, spacing};
use ncrs_ui_kit::tree_list::{LIST_INSET, ROW_OVERHEAD};

use crate::actions::{LocalChanges, OpenMenu};
use crate::ctx::ViewCtx;
use crate::file_views::{self, FileAction};
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;

/// The height of a line of text per pixel of its size.
const LINE_HEIGHT: f32 = 1.3;

pub fn row(
    open: &OpenMenu,
    index: usize,
    (action, enabled): (FileAction, bool),
    ctx: ViewCtx<'_>,
) -> MenuRow<Message> {
    let (label, shortcut) = match action {
        FileAction::History => (Msg::GitViewHistory, Shortcut::FileHistory),
        FileAction::Blame => (Msg::GitViewBlame, Shortcut::Blame),
        FileAction::OpenEditor => (Msg::GitViewOpenEditor, Shortcut::OpenEditor),
        FileAction::Ignore => (Msg::GitViewIgnore, Shortcut::Ignore),
    };
    MenuRow::Entry(PopupEntry {
        label: ctx.lang.text(label).to_owned(),
        key: Some(shortcut.label(ctx.keys)).filter(|key| !key.is_empty()),
        is_selected: open.cursor == index,
        on_press: enabled.then_some(Message::FileMenu(action)),
        on_hover: enabled.then_some(Message::MenuCursor(index)),
    })
}

/// About how tall the facts over the file list of a commit are: text that wraps
/// is measured by its estimated width.
fn facts_height(state: &State, ctx: ViewCtx<'_>) -> f32 {
    let room = super::detail_width(state, ctx) - 2.0 * spacing::M;
    let text = |content: &str, size: f32| {
        let lines = (estimated_width(content, size) / room).ceil().max(1.0);
        lines * size * LINE_HEIGHT + spacing::XS
    };
    let info = state
        .detail
        .as_ref()
        .and_then(|detail| detail.info.as_ref());
    let Some(commit_info) = info else {
        return text("", font_size::ROW);
    };
    let has_committer = commit_info.committer != commit_info.author
        || commit_info.committer_time != commit_info.author_time;
    let has_branches = state
        .containing
        .as_ref()
        .is_some_and(|(id, labels)| *id == commit_info.id && !labels.is_empty());
    let facts = 2
        + usize::from(has_committer)
        + usize::from(!commit_info.parents.is_empty())
        + usize::from(has_branches);
    let mut lines = commit_info.message.lines();
    let title = text(lines.next().unwrap_or_default(), font_size::ROW);
    let body: f32 = lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| text(line, font_size::BODY))
        .sum();
    title + facts as f32 * text("", font_size::SMALL) + body
}

/// Where the rows of the local changes begin, and how tall what lies between
/// the start and row `row` is.
fn local_row_top(local: &LocalChanges, row: usize) -> f32 {
    let head =
        2.0 * spacing::M + (font_size::BODY + font_size::SMALL) * LINE_HEIGHT + 2.0 * spacing::XS;
    let sections = if row < local.unstaged.len() { 1.0 } else { 2.0 };
    head + sections * size::SECTION_HEAD + row as f32 * size::ROW_TALL
}

/// Under the row of the file in the pane at the right.
pub fn anchor(state: &State, path: &str, ctx: ViewCtx<'_>) -> Point {
    let row = file_views::row_of_file(state, path).unwrap_or_default();
    let below = match &state.actions.local {
        Some(local) if state.changes_shown() => local_row_top(local, row) + size::ROW_TALL,
        _ if state.diff_has_own_list() => spacing::XL + (row + 1) as f32 * size::ROW,
        _ => {
            let toggle = size::ROW + 2.0 * spacing::M;
            let head = facts_height(state, ctx) + 2.0 * spacing::M + toggle + LIST_INSET;
            head + (row + 1) as f32 * size::ROW
        }
    };
    Point::new(
        ctx.window.width - super::detail_width(state, ctx) + ROW_OVERHEAD,
        super::body_top(state) + below,
    )
}
