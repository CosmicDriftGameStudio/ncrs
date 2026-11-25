//! The right side: what the commit says and the files it changed.

use iced::widget::{column, container, row, Column, Row, Space};
use iced::{Element, Length};
use ncrs_git::{ChangeKind, CommitDetail};
use ncrs_i18n::Msg;
use ncrs_ui_kit::git_lines::{fact_row, purple_link, summarize_refs};
use ncrs_ui_kit::git_rows::ref_chip;
use ncrs_ui_kit::kit::{key_cap, segmented};
use ncrs_ui_kit::page_text::{page_heading_wrapped, page_line, page_paragraph};
use ncrs_ui_kit::theme::{self, font_size, spacing};
use ncrs_ui_kit::tree_list::{tree_row, tree_scroll, TreeRow};

use super::commits::{chip_kind, moment};
use super::DETAIL_ID;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::{Area, DetailRow, State};

/// How many characters of a parent's id are written.
const PARENT_SHORT: usize = 7;

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
            .push(facts(state, info, ctx));
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
    let added: u32 = detail.files.iter().map(|file| file.added).sum();
    let removed: u32 = detail.files.iter().map(|file| file.removed).sum();
    let count = format!(
        "{} \u{b7} +{added} \u{2212}{removed}",
        ncrs_ui_kit::format::files_counted(detail.files.len(), ctx.lang)
    );
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
        theme::horizontal_rule(palette.border_inactive),
        container(
            row![
                page_line(count, font_size::SMALL, palette.dim_text),
                Space::new().width(Length::Fill),
                toggle
            ]
            .spacing(spacing::M)
            .align_y(iced::alignment::Vertical::Center)
        )
        .padding(spacing::M),
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

/// The facts under the title: who wrote it and when, who committed it, the
/// parents, the SHA and the branches that contain it.
fn facts<'a>(state: &'a State, info: &'a CommitDetail, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let line = |content: String| -> Element<'a, Message> {
        page_line(content, font_size::SMALL, palette.text).into()
    };
    let mut rows = Column::new().spacing(spacing::XS);
    rows = rows.push(fact_row(
        lang.text(Msg::GitViewDetailAuthor),
        line(format!(
            "{} \u{b7} {}",
            info.author,
            moment(info.author_time, ctx)
        )),
        palette,
    ));
    if info.committer != info.author || info.committer_time != info.author_time {
        rows = rows.push(fact_row(
            lang.text(Msg::GitViewDetailCommitter),
            line(format!(
                "{} \u{b7} {}",
                info.committer,
                moment(info.committer_time, ctx)
            )),
            palette,
        ));
    }
    if !info.parents.is_empty() {
        let links = Row::with_children(info.parents.iter().map(|parent| {
            purple_link(
                parent.chars().take(PARENT_SHORT).collect(),
                Message::JumpToCommit(parent.clone()),
                palette,
            )
        }))
        .spacing(spacing::M);
        let label = if info.parents.len() > 1 {
            Msg::GitViewDetailParents
        } else {
            Msg::GitViewDetailParent
        };
        rows = rows.push(fact_row(lang.text(label), links.into(), palette));
    }
    let copy_key = Shortcut::CopySha.label(ctx.keys);
    let mut sha = Row::new()
        .spacing(spacing::M)
        .align_y(iced::Alignment::Center);
    sha = sha.push(line(info.short.clone()));
    if !copy_key.is_empty() {
        sha = sha.push(key_cap(&copy_key, palette));
    }
    rows = rows.push(fact_row(
        lang.text(Msg::GitViewDetailSha),
        sha.into(),
        palette,
    ));
    if let Some((_, labels)) = state
        .containing
        .as_ref()
        .filter(|(id, labels)| *id == info.id && !labels.is_empty())
    {
        let chips = summarize_refs(
            labels
                .iter()
                .map(|label| (label.name.clone(), chip_kind(label.kind)))
                .collect(),
        );
        let wrapped = Row::with_children(
            chips
                .into_iter()
                .map(|(label, kind)| ref_chip(label, kind, palette)),
        )
        .spacing(spacing::XS)
        .wrap();
        rows = rows.push(fact_row(
            lang.text(Msg::GitViewDetailBranches),
            wrapped.into(),
            palette,
        ));
    }
    rows.into()
}
