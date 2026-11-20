//! The right side: what the commit says and the files it changed.

use iced::widget::{column, container, row, Column, Space};
use iced::{Element, Length, Padding};
use ncrs_git::ChangeKind;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::git_rows::{change_row, ChangeRow};
use ncrs_ui_kit::kit::segmented;
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{font_size, spacing};
use ncrs_ui_kit::tree_list::TREE_INDENT;

use super::commits::moment;
use super::DETAIL_ID;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{Area, DetailRow, State};

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(detail) = &state.detail else {
        let hint = if state.row_count() == 0 {
            Msg::GitViewDetailEmpty
        } else {
            Msg::GitViewLoading
        };
        return container(page_line(
            ctx.lang.text(hint),
            font_size::BODY,
            palette.dim_text,
        ))
        .padding(spacing::M)
        .into();
    };
    let mut head = Column::new().spacing(spacing::XS);
    if let Some(info) = &detail.info {
        head = head
            .push(page_heading(
                info.message.lines().next().unwrap_or_default(),
                palette,
            ))
            .push(page_line(
                format!(
                    "{} \u{b7} {} \u{b7} {}",
                    info.short,
                    info.author,
                    moment(info.time, ctx)
                ),
                font_size::SMALL,
                palette.dim_text,
            ));
        for line in info
            .message
            .lines()
            .skip(1)
            .filter(|line| !line.trim().is_empty())
        {
            head = head.push(page_line(line.to_owned(), font_size::BODY, palette.text));
        }
    }
    let toggle = segmented(
        &[
            (
                ctx.lang.text(Msg::GitViewDetailPath),
                Message::SetTreeMode(false),
            ),
            (
                ctx.lang.text(Msg::GitViewDetailTree),
                Message::SetTreeMode(true),
            ),
        ],
        usize::from(state.tree_mode),
        true,
        palette,
    );
    let count = fill_template(
        ctx.lang.text(Msg::GitViewDetailFiles),
        &[("{count}", &detail.files.len().to_string())],
    );
    let files = state.detail_rows().into_iter().map(|entry| match entry {
        DetailRow::Folder { depth, name } => container(page_line(
            format!("{name}/"),
            font_size::BODY,
            palette.dim_text,
        ))
        .padding(Padding {
            left: spacing::CELL_PADDING_X + TREE_INDENT * depth as f32,
            ..Padding::ZERO
        })
        .into(),
        DetailRow::File { depth, name, index } => match detail.files.get(index) {
            Some(file) => change_row(
                ChangeRow {
                    depth,
                    label: label_of(&name, file.kind),
                    added: file.added,
                    removed: file.removed,
                    no_counts: file.binary,
                    chosen: state.area == Area::Detail && state.file_cursor == index,
                    on_press: Message::FilePressed(index),
                },
                palette,
            ),
            None => Space::new().into(),
        },
    });
    let list = column(files).width(Length::Fill);
    column![
        container(head).padding(spacing::M),
        container(
            row![
                toggle,
                Space::new().width(Length::Fill),
                page_line(count, font_size::SMALL, palette.dim_text)
            ]
            .spacing(spacing::M)
            .align_y(iced::alignment::Vertical::Center)
        )
        .padding([0.0, spacing::M]),
        page_scroll(DETAIL_ID, list.into()),
    ]
    .into()
}

fn label_of(name: &str, kind: ChangeKind) -> String {
    let mark = match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
    };
    format!("{mark}  {name}")
}
