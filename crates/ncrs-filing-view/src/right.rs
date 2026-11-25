//! The right half: a tree per target with the folders that get something.

use iced::widget::{column, row, Space};
use iced::{alignment, Color, Element, Length};
use ncrs_i18n::Msg;
use ncrs_index::TargetId;
use ncrs_intake::SortTarget;
use ncrs_ui_kit::format::{ellipsize_middle, shown_path};
use ncrs_ui_kit::frame::chars_that_fit;
use ncrs_ui_kit::hermes_kit::target_chip;
use ncrs_ui_kit::kit::{checkbox, dim_line, one_line, text_field, Check};
use ncrs_ui_kit::plan_rows::{
    editing_row, marked_row, new_file_row, pane_scroll, plan_side_line, NewFileRow,
};
use ncrs_ui_kit::theme::spacing;
use ncrs_ui_kit::tree_list::{tree_row, TreeRow, TREE_INDENT};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::scroll::{RIGHT_ROW_ID, RIGHT_SCROLL_ID};
use crate::state::Session;
use crate::texts::{count, open_color, wait_reason};
use crate::tree::{tree_items, Item};
use crate::update::RENAME_FIELD;

/// What the count and the state of a target take behind its path.
const ROOT_TRAIL: f32 = 220.0;

/// The steps of the tree a level is set in by.
const LEVEL: usize = 2;

fn root_label(
    session: &Session,
    target: &SortTarget,
    half_width: f32,
    ctx: ViewCtx<'_>,
) -> String {
    let label = format!(
        "{} {} · {}",
        session.target_number(&target.id),
        target.name,
        shown_path(&target.path, ctx.home)
    );
    ellipsize_middle(&label, chars_that_fit(half_width - ROOT_TRAIL))
}

fn folder_trail(
    session: &Session,
    target: &TargetId,
    is_root: bool,
    new_files: usize,
    ctx: ViewCtx<'_>,
) -> Vec<(String, Color)> {
    let waiting = session
        .waiting
        .iter()
        .find(|(id, _)| is_root && id == target)
        .map(|(_, reason)| {
            let reason = ctx.lang.text(wait_reason(*reason));
            (reason.to_owned(), open_color(ctx.palette))
        });
    let new = (new_files > 0).then(|| (format!("+{new_files}"), ctx.palette.success));
    waiting.into_iter().chain(new).collect()
}

fn item_row<'a>(
    session: &'a Session,
    target: &SortTarget,
    item: Item,
    half_width: f32,
    ctx: ViewCtx<'_>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let there = |depth: usize, label, folder| {
        tree_row(
            TreeRow {
                depth: depth * LEVEL,
                open: None,
                label,
                chosen: false,
                dim: true,
                lead: None,
                trail: Vec::new(),
                on_press: Message::ToggleFolder(target.id.clone(), folder),
            },
            palette,
        )
    };
    match item {
        Item::Folder {
            path,
            label,
            depth,
            open,
            new_files,
        } => tree_row(
            TreeRow {
                depth: depth * LEVEL,
                open: Some(open),
                label: if depth == 0 {
                    root_label(session, target, half_width, ctx)
                } else {
                    let indent = (depth * LEVEL) as f32 * TREE_INDENT;
                    ellipsize_middle(&label, chars_that_fit(half_width - ROOT_TRAIL - indent))
                },
                chosen: false,
                dim: false,
                lead: None,
                trail: folder_trail(session, &target.id, depth == 0, new_files, ctx),
                on_press: Message::ToggleFolder(target.id.clone(), path),
            },
            palette,
        ),
        Item::NewFile {
            from,
            depth,
            new_name,
            old_name,
        } => {
            let chosen = session.selected.as_ref() == Some(&from);
            let line = match (&session.renaming, chosen) {
                (Some(typed), true) => editing_row(
                    depth * LEVEL,
                    text_field(
                        ctx.lang.text(Msg::FilingViewRenamePlaceholder),
                        typed,
                        RENAME_FIELD,
                        Some(Message::RenameInput),
                        Message::RenameSubmit,
                        palette,
                    ),
                ),
                _ => new_file_row(
                    NewFileRow {
                        depth: depth * LEVEL,
                        new_name,
                        old_name,
                        chosen,
                        on_press: Message::Select(from),
                        on_double_press: Message::StartRename,
                    },
                    palette,
                ),
            };
            if chosen {
                marked_row(line, RIGHT_ROW_ID)
            } else {
                line
            }
        }
        Item::Existing {
            folder,
            depth,
            name,
        } => there(depth, name, folder),
        Item::More {
            folder,
            depth,
            count: more,
        } => there(
            depth,
            count(ctx.lang, Msg::FilingViewMoreThere, more),
            folder,
        ),
    }
}

/// The field for a new name of a file that has no line in the tree, like one
/// whose name is taken.
fn rename_outside_tree<'a>(session: &'a Session, ctx: ViewCtx<'_>) -> Option<Element<'a, Message>> {
    let typed = session.renaming.as_ref()?;
    let row = session.selected_row()?;
    session.places(row).is_empty().then(|| {
        editing_row(
            0,
            text_field(
                ctx.lang.text(Msg::FilingViewRenamePlaceholder),
                typed,
                RENAME_FIELD,
                Some(Message::RenameInput),
                Message::RenameSubmit,
                ctx.palette,
            ),
        )
    })
}

fn header<'a>(session: &Session, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let mut line = row![dim_line(
        ctx.lang.text(Msg::SortingPlanTargets).to_owned(),
        ctx.palette
    )];
    for (index, target) in session.source_targets() {
        let label = format!("{} {}", session.target_number(&target.id), target.name);
        line = line.push(target_chip(index, &label, ctx.palette));
    }
    let line = line
        .push(Space::new().width(Length::Fill))
        .push(checkbox(
            if session.only_new {
                Check::On
            } else {
                Check::Off
            },
            Some(Message::ToggleOnlyNew),
            ctx.palette,
        ))
        .push(one_line(
            ctx.lang.text(Msg::FilingViewOnlyNew).to_owned(),
            ctx.palette,
        ))
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    plan_side_line(line.into())
}

pub fn right<'a>(session: &'a Session, half_width: f32, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> =
        rename_outside_tree(session, ctx).into_iter().collect();
    for (_, target) in session.source_targets() {
        rows.extend(
            tree_items(session, &target.id)
                .into_iter()
                .map(|item| item_row(session, target, item, half_width, ctx)),
        );
    }
    column![header(session, ctx), pane_scroll(RIGHT_SCROLL_ID, rows)].into()
}
