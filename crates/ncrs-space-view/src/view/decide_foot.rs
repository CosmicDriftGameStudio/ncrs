//! The foot of the decide pane: what goes to the trash and what it means, the choice to empty
//! the trash afterwards, how far the choice gets, and the buttons.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format::size_shown;
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::ctx::ViewCtx;
use crate::decide::{DecideMessage, DecideNotice, DecideState};
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::texts::{fill_count_size, fill_plural};

fn notice_line<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let text = match pane.notice()? {
        DecideNotice::Trashed {
            count,
            bytes,
            emptied: true,
        } => fill_count_size(Msg::SpaceDecideTrashedEmptied, count, bytes, ctx.lang),
        DecideNotice::Trashed { count, bytes, .. } => fill_plural(
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

fn note<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let mut lines =
        column![kit::dim(ctx.lang.text(Msg::SpaceDecideTrashNote), palette)].spacing(spacing::XS);
    if ctx.trash_can_purge {
        lines = lines.push(
            row![
                kit::labelled_checkbox(
                    if pane.empty_after() {
                        Check::On
                    } else {
                        Check::Off
                    },
                    Some(Message::Decide(DecideMessage::ToggleEmptyTrash)),
                    Some(ctx.lang.text(Msg::SpaceDecideEmptyTrash)),
                    palette,
                ),
                kit::key_cap(&ctx.keys.label(Shortcut::EmptyTrash), palette),
            ]
            .spacing(spacing::M)
            .align_y(alignment::Vertical::Center),
        );
    }
    if let Some(line) = notice_line(pane, ctx) {
        lines = lines.push(line);
    }
    container(lines)
        .padding([spacing::M, spacing::XXL])
        .width(Length::Fill)
        .into()
}

fn summary(pane: &DecideState, state: &State, ctx: ViewCtx<'_>) -> String {
    let (groups, total) = pane.groups_chosen();
    let free = state.scan().map_or(0, |scan| {
        scan.volume.available.saturating_add(pane.chosen_bytes())
    });
    ncrs_i18n::fill_template(
        ctx.lang.text(Msg::SpaceDecideSummary),
        &[
            ("{groups}", &groups.to_string()),
            ("{total}", &total.to_string()),
            ("{free}", &size_shown(free, ctx.lang)),
        ],
    )
}

pub(super) fn view<'a>(
    pane: &'a DecideState,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let trash_label = fill_plural(
        Msg::SpaceDecideTrashButtonOne,
        Msg::SpaceDecideTrashButton,
        pane.chosen_count(),
        pane.chosen_bytes(),
        ctx.lang,
    );
    let foot = row![
        page_line(
            summary(pane, state, ctx),
            font_size::SMALL,
            palette.dim_text
        ),
        Space::new().width(Length::Fill),
        kit::key_button(
            ctx.lang.text(Msg::DialogCancel),
            "Esc",
            Some(Message::Escape),
            Tone::Secondary,
            palette,
        ),
        kit::key_button(
            &trash_label,
            &ctx.keys.label(Shortcut::Trash),
            (pane.chosen_count() > 0).then_some(Message::Decide(DecideMessage::Trash)),
            Tone::Primary,
            palette,
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    column![
        theme::horizontal_rule(palette.border_inactive),
        note(pane, ctx),
        theme::horizontal_rule(palette.border_inactive),
        container(foot)
            .padding([spacing::M, spacing::XXL])
            .width(Length::Fill),
    ]
    .into()
}
