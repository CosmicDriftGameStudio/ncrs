//! The decide pane: the items of Downloads or of a big folder by reason, and what goes to
//! the trash.

use iced::widget::{column, container, mouse_area, row, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::{shown_path, size_shown, time};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::section_head::{section_head_with, SectionHead};
use ncrs_ui_kit::size_row::{size_row, SizeRow};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::decide::{
    DecideGroup, DecideGroupKind, DecideItem, DecideKey, DecideMessage, DecideNotice, DecideReason,
    DecideState, DECIDE_LIST_ID,
};
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::texts::fill_plural;

fn decide(message: DecideMessage) -> Message {
    Message::Decide(message)
}

fn group_msg(kind: DecideGroupKind) -> Msg {
    match kind {
        DecideGroupKind::Installed => Msg::SpaceDecideGroupInstalled,
        DecideGroupKind::Unpacked => Msg::SpaceDecideGroupUnpacked,
        DecideGroupKind::Duplicates => Msg::SpaceDecideGroupDuplicates,
        DecideGroupKind::UntouchedYear => Msg::SpaceDecideGroupUntouchedYear,
        DecideGroupKind::Rest => Msg::SpaceDecideGroupRest,
    }
}

fn reason_text(reason: &DecideReason, ctx: ViewCtx<'_>) -> String {
    match reason {
        DecideReason::Installed(_) => ctx
            .lang
            .text(Msg::SpaceDecideGroupReasonInstalled)
            .to_owned(),
        DecideReason::Unpacked(_) => ctx
            .lang
            .text(Msg::SpaceDecideGroupReasonUnpacked)
            .to_owned(),
        DecideReason::Duplicate(name) => fill_template(
            ctx.lang.text(Msg::SpaceDecideGroupReasonDuplicate),
            &[("{name}", name)],
        ),
        DecideReason::Untouched => ctx
            .lang
            .text(Msg::SpaceDecideGroupReasonUntouched)
            .to_owned(),
        DecideReason::Rest => String::new(),
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn item_row<'a>(
    entry: &'a DecideItem,
    biggest: u64,
    pane: &'a DecideState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let path = &entry.item.path;
    let is_cursor = pane.cursor == Some(DecideKey::Item(path.clone()));
    let built = size_row(
        SizeRow {
            check: Some(if pane.is_chosen(path) {
                Check::On
            } else {
                Check::Off
            }),
            name: file_name(path),
            path: shown_path(path, ctx.home),
            size_text: size_shown(entry.item.size, ctx.lang),
            fraction: entry.item.size as f32 / biggest.max(1) as f32,
            color: theme::space::decide(),
            reason: reason_text(&entry.reason, ctx),
            outcome: None,
            cursor: is_cursor,
            on_press: Some(decide(DecideMessage::RowPressed(path.clone()))),
            on_toggle: Some(decide(DecideMessage::RowToggled(path.clone()))),
        },
        ctx.palette,
    );
    let framed = if is_cursor {
        container(built).id(FOCUSED_ROW_ID).into()
    } else {
        built
    };
    mouse_area(framed)
        .on_double_click(decide(DecideMessage::RowToggled(path.clone())))
        .into()
}

fn group_view<'a>(
    group: &'a DecideGroup,
    biggest: u64,
    pane: &'a DecideState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let title = ctx.lang.text(group_msg(group.kind));
    let bytes: u64 = group
        .items
        .iter()
        .fold(0, |sum, entry| sum.saturating_add(entry.item.size));
    let total = page_line(
        size_shown(bytes, ctx.lang),
        font_size::SMALL,
        ctx.palette.dim_text,
    );
    let head = section_head_with(
        SectionHead {
            title,
            count: group.items.len(),
            open: !pane.is_folded(group.kind),
            has_focus: pane.cursor == Some(DecideKey::Head(group.kind)),
            on_toggle: decide(DecideMessage::HeadToggled(group.kind)),
        },
        vec![total],
        ctx.palette,
    );
    let mut body = column![head];
    if !pane.is_folded(group.kind) {
        for entry in &group.items {
            body = body.push(item_row(entry, biggest, pane, ctx));
        }
    }
    body.into()
}

fn groups<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    if pane.groups().is_empty() {
        let note = if pane.is_loaded() {
            ctx.lang.text(Msg::SpaceEmpty)
        } else {
            ""
        };
        return container(page_line(note, font_size::BODY, ctx.palette.dim_text))
            .padding(spacing::XXL)
            .into();
    }
    let biggest = pane
        .groups()
        .iter()
        .flat_map(|group| group.items.iter())
        .map(|entry| entry.item.size)
        .max()
        .unwrap_or(0);
    column(
        pane.groups()
            .iter()
            .map(|group| group_view(group, biggest, pane, ctx)),
    )
    .width(Length::Fill)
    .into()
}

fn fact<'a>(label: &str, value: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    row![
        container(page_line(label, font_size::SMALL, ctx.palette.dim_text)).width(size::FACT_LABEL),
        container(kit::paragraph(value, ctx.palette)).width(Length::Fill),
    ]
    .spacing(spacing::M)
    .into()
}

fn notice_line<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let text = match pane.notice()? {
        DecideNotice::Trashed { count, bytes } => fill_plural(
            Msg::SpaceDecideTrashedOne,
            Msg::SpaceDecideTrashed,
            count,
            bytes,
            ctx.lang,
        ),
        DecideNotice::Undone { count } => fill_plural(
            Msg::SpaceDecideUndoneOne,
            Msg::SpaceDecideUndone,
            count,
            0,
            ctx.lang,
        ),
    };
    Some(kit::paragraph(text, ctx.palette))
}

/// The item under the cursor, the selection and the buttons that act on it.
fn selection<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let mut side = column![page_line(
        ctx.lang.text(Msg::SpaceDecideSelectionHead),
        font_size::TITLE,
        palette.text
    )]
    .spacing(spacing::M);
    if let Some(entry) = pane.cursor_item() {
        side = side
            .push(page_line(
                file_name(&entry.item.path),
                font_size::BODY,
                palette.text,
            ))
            .push(fact(
                ctx.lang.text(Msg::SpaceDecideDetailSize),
                size_shown(entry.item.size, ctx.lang),
                ctx,
            ))
            .push(fact(
                ctx.lang.text(Msg::SpaceDecideDetailLoaded),
                time(entry.item.modified),
                ctx,
            ))
            .push(fact(
                ctx.lang.text(Msg::SpaceDecideDetailOpened),
                if entry.item.touched.is_some() {
                    time(entry.item.touched)
                } else {
                    ctx.lang.text(Msg::SpaceDecideDetailOpenedNever).to_owned()
                },
                ctx,
            ));
        let reason = reason_text(&entry.reason, ctx);
        if !reason.is_empty() {
            side = side.push(kit::paragraph(format!("\u{2713} {reason}"), palette));
        }
    }
    side = side
        .push(fact(
            ctx.lang.text(Msg::SpaceDecideSelectionChosen),
            fill_plural(
                Msg::SpaceDecideSelectionChosenValueOne,
                Msg::SpaceDecideSelectionChosenValue,
                pane.chosen_count(),
                pane.chosen_bytes(),
                ctx.lang,
            ),
            ctx,
        ))
        .push(fact(
            ctx.lang.text(Msg::SpaceDecideSelectionWhere),
            ctx.lang.text(Msg::SpaceDecideSelectionTrash).to_owned(),
            ctx,
        ))
        .push(kit::dim(ctx.lang.text(Msg::SpaceDecideTrashNote), palette));
    if ctx.trash_can_purge {
        side = side.push(kit::labelled_checkbox(
            if pane.empty_after() {
                Check::On
            } else {
                Check::Off
            },
            Some(decide(DecideMessage::ToggleEmptyTrash)),
            Some(ctx.lang.text(Msg::SpaceDecideEmptyTrash)),
            palette,
        ));
    }
    if let Some(line) = notice_line(pane, ctx) {
        side = side.push(line);
    }
    let trash_label = fill_plural(
        Msg::SpaceDecideTrashButtonOne,
        Msg::SpaceDecideTrashButton,
        pane.chosen_count(),
        pane.chosen_bytes(),
        ctx.lang,
    );
    side.push(Space::new().height(spacing::M))
        .push(
            row![
                kit::key_button(
                    &trash_label,
                    &ctx.keys.label(Shortcut::Trash),
                    (pane.chosen_count() > 0).then_some(decide(DecideMessage::Trash)),
                    Tone::Primary,
                    palette,
                ),
                kit::key_button(
                    ctx.lang.text(Msg::SpaceDecideSelectionUndo),
                    &ctx.keys.label(Shortcut::UndoTrash),
                    pane.can_undo().then_some(decide(DecideMessage::Undo)),
                    Tone::Secondary,
                    palette,
                ),
                kit::key_button(
                    ctx.lang.text(Msg::SpaceDecideSelectionBack),
                    "Esc",
                    Some(Message::Escape),
                    Tone::Secondary,
                    palette,
                ),
            ]
            .spacing(spacing::M),
        )
        .into()
}

pub fn view<'a>(pane: &'a DecideState, state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let name = if pane.is_downloads() {
        ctx.lang.text(Msg::SpaceDecideDownloads).to_owned()
    } else {
        file_name(pane.folder())
    };
    let title = fill_template(ctx.lang.text(Msg::SpaceDecideTitle), &[("{name}", &name)]);
    let summary = fill_template(
        ctx.lang.text(Msg::SpaceDecideMeasured),
        &[
            ("{folder}", &shown_path(pane.folder(), ctx.home)),
            ("{size}", &size_shown(pane.total_bytes(), ctx.lang)),
            ("{time}", &time(state.scan().map(|scan| scan.measured_at))),
        ],
    );
    let scroll = page_scroll_padded(
        DECIDE_LIST_ID,
        groups(pane, ctx),
        Padding::from([0.0, spacing::XL]),
    );
    column![
        container(
            row![
                page_heading(&title, palette),
                page_line(summary, font_size::BODY, palette.dim_text),
            ]
            .spacing(spacing::L)
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::XXL])
        .height(size::HEADER)
        .center_y(size::HEADER),
        theme::horizontal_rule(palette.border_inactive),
        row![
            container(scroll).width(Length::Fill).height(Length::Fill),
            theme::vertical_rule(palette.border_inactive),
            container(selection(pane, ctx))
                .padding(spacing::XL)
                .width(size::DETAIL_PANE),
        ]
        .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
