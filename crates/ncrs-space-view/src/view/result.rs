//! The banner over the list after a run: what it freed, what it deleted and skipped, how
//! long it took, and the way on.

use iced::widget::{container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Group;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::confirm::ConfirmKind;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::{ResultState, State};
use crate::texts::{fill_count, fill_size, percent_of, took_text};

fn title(done: &ResultState, ctx: ViewCtx<'_>) -> &'static str {
    ctx.lang.text(match done.kind {
        ConfirmKind::Safe => Msg::SpaceResultTitleSafe,
        ConfirmKind::Worktrees => Msg::SpaceResultTitleWorktrees,
        ConfirmKind::One => Msg::SpaceResultTitleOne,
    })
}

/// The rows of the list the run deleted, not the folders inside them.
fn deleted_rows(state: &State) -> usize {
    state.rows().iter().filter(|row| state.is_done(row)).count()
}

fn skipped_rows(state: &State, done: &ResultState) -> usize {
    state
        .rows()
        .iter()
        .filter(|row| {
            done.logged(&row.path)
                .is_some_and(|logged| logged.outcome.is_err())
        })
        .count()
}

pub fn banner<'a>(
    state: &'a State,
    done: &'a ResultState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let figure = |text: String, ink| page_line(text, font_size::FIGURE, ink);
    let skipped = skipped_rows(state, done);
    let mut line = row![
        page_heading(title(done, ctx), palette),
        figure(
            fill_size(Msg::SpaceResultFreed, done.freed(), ctx.lang),
            palette.success
        ),
        figure(
            fill_count(Msg::SpaceResultDeleted, deleted_rows(state), ctx.lang),
            palette.text
        ),
        figure(
            fill_count(Msg::SpaceResultSkipped, skipped, ctx.lang),
            if skipped > 0 {
                palette.error
            } else {
                palette.dim_text
            },
        ),
        page_line(
            took_text(
                done.log
                    .finished
                    .duration_since(done.log.started)
                    .unwrap_or_default(),
            ),
            font_size::BODY,
            palette.dim_text,
        ),
    ]
    .spacing(spacing::XXL)
    .align_y(alignment::Vertical::Center);
    if let Some(free_after) = done.free_after {
        let total = state.scan().map_or(0, |scan| scan.volume.total);
        line = line
            .push(page_line(
                fill_size(Msg::SpaceResultFreeNow, free_after, ctx.lang),
                font_size::BODY,
                palette.text,
            ))
            .push(page_line(
                fill_template(
                    ctx.lang.text(Msg::SpaceResultPercentUsed),
                    &[(
                        "{percent}",
                        &percent_of(total.saturating_sub(free_after), total).to_string(),
                    )],
                ),
                font_size::BODY,
                palette.text,
            ))
            .push(page_line(
                fill_size(Msg::SpaceResultBefore, done.free_before, ctx.lang),
                font_size::BODY,
                palette.dim_text,
            ));
    }
    line = line.push(Space::new().width(Length::Fill));
    let idle = !state.is_measuring() && state.running().is_none();
    if !ctx.store_build && state.open_ticked_count(Group::Worktrees) > 0 {
        line = line.push(kit::key_button(
            &fill_size(
                Msg::SpaceRemoveWorktreesAlso,
                state.open_ticked_bytes(Group::Worktrees),
                ctx.lang,
            ),
            &ctx.keys.label(Shortcut::RemoveWorktrees),
            idle.then_some(Message::RemoveWorktrees),
            Tone::Primary,
            palette,
        ));
    }
    line = line.push(kit::key_button(
        ctx.lang.text(Msg::SpaceCloseResult),
        ctx.lang.text(Msg::SpaceKeyNameEscape),
        Some(Message::CloseResult),
        Tone::Secondary,
        palette,
    ));
    container(
        container(line)
            .padding([spacing::M, spacing::XL])
            .width(Length::Fill)
            .style(theme::outlined(palette)),
    )
    .padding([spacing::M, spacing::XL])
    .into()
}
