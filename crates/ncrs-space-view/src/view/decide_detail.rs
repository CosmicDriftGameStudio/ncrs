//! The right side of the decide pane: the item under the cursor with the reasons Hermes has for
//! offering it, and the selection with the way back.

use iced::widget::{column, container, row};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::git_lines::fact_row;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, spacing};
use ncrs_ui_kit::APP_FONT_BOLD;

use super::decide_text::{day, file_name, group_msg, kind_text, why_lines};
use crate::ctx::ViewCtx;
use crate::decide::{DecideItem, DecideMessage, DecideState};
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::texts::fill_plural;

fn heading<'a>(label: &str, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    iced::widget::text(label.to_uppercase())
        .size(font_size::SMALL)
        .font(APP_FONT_BOLD)
        .color(ctx.palette.dim_text)
        .into()
}

fn fact<'a>(label: &str, value: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    fact_row(label, kit::paragraph(value, ctx.palette), ctx.palette)
}

fn opened_text(entry: &DecideItem, ctx: ViewCtx<'_>) -> String {
    let at = entry.item.last_opened.or(entry.item.touched);
    if at.is_none() {
        return ctx.lang.text(Msg::SpaceDecideDetailOpenedNever).to_owned();
    }
    let date = day(at);
    match entry.item.open_count {
        Some(1) => fill_template(
            ctx.lang.text(Msg::SpaceDecideDetailOpenedOnce),
            &[("{date}", &date)],
        ),
        Some(count) => fill_template(
            ctx.lang.text(Msg::SpaceDecideDetailOpenedTimes),
            &[("{date}", &date), ("{count}", &count.to_string())],
        ),
        None => date,
    }
}

/// "If you want to keep it: [Space] takes it out of the selection."
fn keep_hint<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let template = ctx.lang.text(Msg::SpaceDecideKeepHint);
    let (before, after) = template.split_once("{key}").unwrap_or((template, ""));
    row![
        page_line(before.to_owned(), font_size::SMALL, ctx.palette.dim_text),
        kit::key_cap(ctx.lang.text(Msg::SpaceDecideKeySpace), ctx.palette),
        page_line(after.to_owned(), font_size::SMALL, ctx.palette.dim_text),
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn item_section<'a>(
    pane: &'a DecideState,
    entry: &'a DecideItem,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let mut side = column![
        iced::widget::text(file_name(&entry.item.path))
            .size(font_size::BODY)
            .font(APP_FONT_BOLD)
            .color(palette.text),
        page_line(
            shown_path(&entry.item.path, ctx.home),
            font_size::SMALL,
            palette.dim_text
        ),
        fact(
            ctx.lang.text(Msg::SpaceDecideDetailSize),
            size_shown(entry.item.size, ctx.lang),
            ctx
        ),
        fact(
            ctx.lang.text(Msg::SpaceDecideDetailLoaded),
            day(entry.item.modified),
            ctx
        ),
        fact(
            ctx.lang.text(Msg::SpaceDecideDetailOpened),
            opened_text(entry, ctx),
            ctx
        ),
        fact(
            ctx.lang.text(Msg::SpaceDecideDetailKind),
            kind_text(&entry.item.path, entry.item.is_folder, ctx),
            ctx
        ),
    ]
    .spacing(spacing::M);
    if let Some(kind) = pane.cursor_group() {
        side = side.push(fact(
            ctx.lang.text(Msg::SpaceDecideDetailGroup),
            ctx.lang.text(group_msg(kind)).to_owned(),
            ctx,
        ));
    }
    let lines = why_lines(&entry.reason, &entry.item.path, ctx);
    if !lines.is_empty() {
        side = side
            .push(iced::widget::Space::new().height(spacing::XL))
            .push(heading(ctx.lang.text(Msg::SpaceDecideWhyHead), ctx));
        for line in lines {
            side = side.push(
                row![
                    page_line("\u{2713}", font_size::BODY, palette.success),
                    kit::paragraph(line, palette),
                ]
                .spacing(spacing::M),
            );
        }
        if pane.is_chosen(&entry.item.path) {
            side = side.push(keep_hint(ctx));
        }
    }
    side.into()
}

fn selection_section<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    column![
        heading(ctx.lang.text(Msg::SpaceDecideSelectionHead), ctx),
        fact(
            ctx.lang.text(Msg::SpaceDecideSelectionChosen),
            fill_plural(
                Msg::SpaceDecideSelectionChosenValueOne,
                Msg::SpaceDecideSelectionChosenValue,
                pane.chosen_count(),
                pane.chosen_bytes(),
                ctx.lang,
            ),
            ctx,
        ),
        fact(
            ctx.lang.text(Msg::SpaceDecideSelectionWhere),
            ctx.lang.text(Msg::SpaceDecideSelectionTrash).to_owned(),
            ctx,
        ),
        fact_row(
            ctx.lang.text(Msg::SpaceDecideSelectionBack),
            kit::key_button(
                ctx.lang.text(Msg::SpaceDecideSelectionUndo),
                &ctx.keys.label(Shortcut::UndoTrash),
                pane.can_undo()
                    .then_some(Message::Decide(DecideMessage::Undo)),
                Tone::Secondary,
                palette,
            ),
            palette,
        ),
    ]
    .spacing(spacing::M)
    .into()
}

pub(super) fn view<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let mut sections = column![].width(Length::Fill);
    if let Some(entry) = pane.cursor_item() {
        sections = sections
            .push(container(item_section(pane, entry, ctx)).padding(spacing::XL))
            .push(theme::horizontal_rule(palette.border_inactive));
    }
    sections
        .push(container(selection_section(pane, ctx)).padding(spacing::XL))
        .into()
}
