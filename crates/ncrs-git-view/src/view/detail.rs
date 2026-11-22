//! The right side: what the commit says and the files it changed.

use iced::widget::{column, container, row, Column, Space};
use iced::{Element, Length};
use ncrs_git::ChangeKind;
use ncrs_i18n::Msg;
use ncrs_ui_kit::kit::segmented;
use ncrs_ui_kit::page_text::{page_heading_wrapped, page_line, page_paragraph};
use ncrs_ui_kit::theme::{font_size, spacing};
use ncrs_ui_kit::tree_list::{tree_row, tree_scroll, TreeRow};

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
    let mut head = Column::new().spacing(spacing::XS).width(Length::Fill);
    if let Some(info) = &detail.info {
        head = head
            .push(page_heading_wrapped(
                info.message.lines().next().unwrap_or_default(),
                palette,
            ))
            .push(page_paragraph(
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
            head = head.push(page_paragraph(
                line.to_owned(),
                font_size::BODY,
                palette.text,
            ));
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
    let count = ncrs_ui_kit::format::files_counted(detail.files.len(), ctx.lang);
    let files = state.detail_rows().into_iter().map(|entry| match entry {
        DetailRow::Folder { depth, name } => tree_row(
            TreeRow {
                depth,
                open: None,
                label: name,
                chosen: false,
                dim: true,
                lead: None,
                trail: Vec::new(),
                on_press: Message::FolderPressed,
            },
            palette,
        ),
        DetailRow::File { depth, name, index } => match detail.files.get(index) {
            Some(file) => tree_row(
                TreeRow {
                    depth,
                    open: None,
                    label: name,
                    chosen: state.area == Area::Detail && state.file_cursor == index,
                    dim: false,
                    lead: Some((kind_letter(file.kind).to_owned(), palette.text)),
                    trail: if file.binary {
                        Vec::new()
                    } else {
                        vec![
                            (format!("+{}", file.added), palette.success),
                            (format!("\u{2212}{}", file.removed), palette.error),
                        ]
                    },
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
        tree_scroll(DETAIL_ID, list.into()),
    ]
    .into()
}

fn kind_letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
    }
}
