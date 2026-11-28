//! The pane beside the list: what the row under the cursor is, why it can go, and what the
//! checks found.

use iced::widget::{column, container, row, Space};
use iced::{Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Group;
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::DisplayRow;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::texts::{afterwards, ago_text, group_title, made_by, percent_of, row_checks, row_name};

fn fact<'a>(label: &str, value: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    row![
        container(page_line(label, font_size::SMALL, ctx.palette.dim_text)).width(size::FACT_LABEL),
        container(kit::paragraph(value, ctx.palette)).width(Length::Fill),
    ]
    .spacing(spacing::M)
    .into()
}

fn row_pane<'a>(row: &'a DisplayRow, state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let total = state.scan().map_or(0, |scan| scan.volume.total);
    let share = fill_template(
        ctx.lang.text(Msg::SpaceDetailShare),
        &[
            ("{size}", &size_shown(row.bytes, ctx.lang)),
            ("{percent}", &percent_of(row.bytes, total).to_string()),
        ],
    );
    let last_used = row
        .modified
        .map_or_else(|| "\u{2013}".to_owned(), |when| ago_text(ctx.now, when));
    let mut facts = column![
        page_line(row_name(row, ctx.lang), font_size::TITLE, palette.text),
        page_line(
            shown_path(&row.path, ctx.home),
            font_size::SMALL,
            palette.dim_text
        ),
        fact(
            ctx.lang.text(Msg::SpaceDetailGroup),
            group_title(row.group, ctx.lang),
            ctx
        ),
        fact(ctx.lang.text(Msg::SpaceColumnSize), share, ctx),
        fact(ctx.lang.text(Msg::SpaceDetailLastUsed), last_used, ctx),
    ]
    .spacing(spacing::M);
    let made = made_by(row);
    if !made.is_empty() {
        facts = facts.push(fact(ctx.lang.text(Msg::SpaceDetailMadeBy), made, ctx));
    }
    let after = afterwards(row, ctx.lang);
    if !after.is_empty() {
        facts = facts.push(fact(ctx.lang.text(Msg::SpaceDetailAfterwards), after, ctx));
    }
    facts = facts.push(page_line(
        ctx.lang.text(Msg::SpaceDetailChecks),
        font_size::SMALL,
        palette.dim_text,
    ));
    for check in row_checks(row, ctx.processes_known, ctx.lang) {
        facts = facts.push(kit::paragraph(format!("\u{2713} {check}"), palette));
    }
    let mut buttons = row![kit::key_button(
        ctx.lang.text(Msg::SpaceShowInPanel),
        &ctx.keys.label(Shortcut::ShowInPanel),
        Some(Message::ShowInPanelAtCursor),
        Tone::Secondary,
        palette,
    )]
    .spacing(spacing::M);
    if row.group == Group::Decide {
        buttons = buttons.push(kit::button(
            ctx.lang.text(Msg::SpaceViewDetails),
            Some(Message::Enter),
            Tone::Primary,
            palette,
        ));
    }
    if row.is_deletable() {
        buttons = buttons.push(kit::key_button(
            ctx.lang.text(Msg::SpaceDeleteOnlyThis),
            &ctx.keys.label(Shortcut::DeleteOnlyThis),
            (!state.is_measuring() && state.running().is_none()).then_some(Message::DeleteOnlyThis),
            Tone::DangerOutline,
            palette,
        ));
    }
    facts
        .push(Space::new().height(spacing::M))
        .push(buttons)
        .into()
}

fn group_pane<'a>(group: Group, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let hint = match group {
        Group::Safe => Some(Msg::SpacePermanentNote),
        Group::Decide => Some(Msg::SpaceGroupDecideHint),
        Group::InUse => Some(Msg::SpaceGroupInUseHint),
        Group::Worktrees => None,
    };
    let mut pane = column![page_line(
        group_title(group, ctx.lang),
        font_size::TITLE,
        palette.text
    )]
    .spacing(spacing::M);
    if let Some(msg) = hint {
        pane = pane.push(kit::dim(ctx.lang.text(msg), palette));
    }
    pane.into()
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let content: Element<'a, Message> = match (state.cursor_row(), state.cursor_item()) {
        (Some(selected), _) => row_pane(selected, state, ctx),
        (None, Some(crate::rows::Item::Head(group))) => group_pane(group, ctx),
        _ => Space::new().into(),
    };
    row![
        theme::vertical_rule(ctx.palette.border_inactive),
        container(content).padding(spacing::XL).width(Length::Fill),
    ]
    .into()
}
