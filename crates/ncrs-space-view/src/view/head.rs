//! The top of the tab: title with the measured line, the bar of the drive with its legend,
//! and the buttons for what the groups hold.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Color, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Group;
use ncrs_ui_kit::color_chip::color_chip;
use ncrs_ui_kit::format::{clock_minutes, size_shown};
use ncrs_ui_kit::kit::{self, text_field, Tone};
use ncrs_ui_kit::meter::{meter_by_level, meter_level_from, Meter};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::stacked_bar::{stacked_bar, stacked_bar_striped, BarSegment};
use ncrs_ui_kit::theme::{font_size, size, spacing};

use super::list::group_color;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::{ResultState, State};
use crate::texts::{fill_size, percent_of};
use crate::update::FILTER_ID;

/// After a run the newer measure says when the numbers are from; else the measure of the list.
fn measured_text(state: &State, ctx: ViewCtx<'_>) -> String {
    match (state.result(), state.pending_scan()) {
        (Some(_), Some(scan)) => fill_template(
            ctx.lang.text(Msg::SpaceResultMeasuredAgain),
            &[
                ("{total}", &size_shown(scan.volume.total, ctx.lang)),
                ("{time}", &clock_minutes(Some(scan.measured_at))),
            ],
        ),
        _ => super::measured_line(state, ctx),
    }
}

pub fn title<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let info = state.measuring().map_or_else(
        || measured_text(state, ctx),
        |measuring| fill_size(Msg::SpaceMeasuring, measuring.bytes, ctx.lang),
    );
    let heading = state
        .scan()
        .map(|scan| scan.volume_name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| ctx.lang.text(Msg::SpaceHeading));
    let partial = state.scan().is_some_and(|scan| scan.partial) && !state.is_measuring();
    let filter = text_field(
        ctx.lang.text(Msg::SpaceFilterPlaceholder),
        state.filter(),
        FILTER_ID,
        Some(Message::FilterChanged),
        Message::FilterSubmitted,
        palette,
    );
    let rescan = kit::key_button(
        ctx.lang.text(Msg::SpaceRescan),
        &ctx.keys.label(Shortcut::Rescan),
        (state.running().is_none()).then_some(Message::Rescan),
        Tone::Secondary,
        palette,
    );
    let mut line = row![
        page_heading(heading, palette),
        page_line(info, font_size::BODY, palette.dim_text),
    ];
    if partial {
        line = line.push(page_line(
            ctx.lang.text(Msg::SpaceMeasuredPartly),
            font_size::BODY,
            palette.dim_text,
        ));
    }
    container(
        line.push(Space::new().width(Length::Fill))
            .push(rescan)
            .push(container(filter).width(size::FIELD_WIDE))
            .spacing(spacing::L)
            .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::XXL])
    .height(size::HEADER)
    .center_y(size::HEADER)
    .into()
}

/// The meter of the drive, its bar cut by group, and the legend.
pub fn drive<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(scan) = state.scan() else {
        return Space::new().into();
    };
    let volume = state.volume_now().unwrap_or(scan.volume);
    let total = volume.total;
    let free = volume.available.min(total);
    let used = total - free;
    let percent = percent_of(used, total);
    let value = fill_template(
        ctx.lang.text(Msg::SpaceResultPercentUsed),
        &[("{percent}", &percent.to_string())],
    );
    let detail = fill_template(
        ctx.lang.text(Msg::SpaceUsedOf),
        &[
            ("{used}", &size_shown(used, ctx.lang)),
            ("{total}", &size_shown(total, ctx.lang)),
            ("{free}", &size_shown(free, ctx.lang)),
        ],
    );
    let gauge = meter_by_level(
        Meter {
            title: ctx.lang.text(Msg::SpaceDriveLabel),
            value: &value,
            fraction: used as f32 / total.max(1) as f32,
            level: meter_level_from(percent as f32, f32::from(ctx.warn_from_percent)),
            detail: &detail,
        },
        palette,
    );
    let groups = [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse];
    let group_sizes: Vec<u64> = groups
        .iter()
        .map(|group| state.group_bytes(*group))
        .collect();
    let counted: u64 = group_sizes.iter().sum();
    let mut segments: Vec<BarSegment> = groups
        .iter()
        .zip(&group_sizes)
        .map(|(group, bytes)| BarSegment {
            portion: *bytes,
            color: group_color(*group, palette),
        })
        .collect();
    segments.push(BarSegment {
        portion: used.saturating_sub(counted),
        color: ncrs_ui_kit::theme::space::rest(palette),
    });
    segments.push(BarSegment {
        portion: free,
        color: ncrs_ui_kit::theme::space::free(palette),
    });
    let legend_text = |label: Msg, bytes: u64| {
        fill_template(
            ctx.lang.text(Msg::SpaceLegendEntry),
            &[
                ("{label}", ctx.lang.text(label)),
                ("{size}", &size_shown(bytes, ctx.lang)),
            ],
        )
    };
    let labels = [
        Msg::SpaceLegendSafe,
        Msg::SpaceLegendWorktrees,
        Msg::SpaceLegendDecide,
        Msg::SpaceLegendInUse,
    ];
    let mut legend: Vec<Element<'a, Message>> = groups
        .iter()
        .zip(labels)
        .zip(&group_sizes)
        .filter(|((group, _), bytes)| {
            **bytes > 0 && !(ctx.store_build && **group == Group::Worktrees)
        })
        .map(|((group, label), bytes)| {
            color_chip(
                &legend_text(label, *bytes),
                group_color(*group, palette),
                None::<Message>,
            )
        })
        .collect();
    legend.push(color_chip(
        &legend_text(Msg::SpaceLegendRest, used.saturating_sub(counted)),
        ncrs_ui_kit::theme::space::rest(palette),
        None::<Message>,
    ));
    legend.push(color_chip(
        &legend_text(Msg::SpaceLegendFree, free),
        palette.dim_text,
        None::<Message>,
    ));
    let after_cleanup = free
        .saturating_add(state.open_ticked_bytes(Group::Safe))
        .saturating_add(if ctx.store_build {
            0
        } else {
            state.open_ticked_bytes(Group::Worktrees)
        });
    let mut figures = row![color_chip(
        &fill_size(Msg::SpaceFreeNow, free, ctx.lang),
        palette.text,
        None::<Message>,
    )]
    .spacing(spacing::XS);
    if after_cleanup > free && state.result().is_none() {
        figures = figures.push(color_chip(
            &fill_size(Msg::SpaceFreeAfter, after_cleanup, ctx.lang),
            group_color(Group::Safe, palette),
            None::<Message>,
        ));
    }
    let bar = match state.result() {
        Some(done) => {
            let parts = result_parts(state, done, total, palette);
            legend = parts
                .iter()
                .filter(|(_, bytes, _)| *bytes > 0)
                .map(|(label, bytes, color)| {
                    color_chip(&legend_text(*label, *bytes), *color, None::<Message>)
                })
                .collect();
            result_bar(&parts, palette)
        }
        None => stacked_bar(&segments, palette),
    };
    container(
        column![
            row![gauge, figures]
                .spacing(spacing::XL)
                .align_y(alignment::Vertical::Center),
            bar,
            row(legend).spacing(spacing::XS),
        ]
        .spacing(spacing::M),
    )
    .padding([spacing::M, spacing::XXL])
    .into()
}

/// The buttons for what the ticked rows hold.
pub fn actions<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let idle = !state.is_measuring() && state.running().is_none();
    let safe_ticked = state.open_ticked_count(Group::Safe);
    let worktrees_ticked = state.open_ticked_count(Group::Worktrees);
    let delete_label = if safe_ticked > 0 {
        fill_size(
            Msg::SpaceDeleteAllSafe,
            state.open_ticked_bytes(Group::Safe),
            ctx.lang,
        )
    } else {
        ctx.lang.text(Msg::SpaceDeleteAllSafePlain).to_owned()
    };
    let mut line = row![kit::key_button(
        &delete_label,
        &ctx.keys.label(Shortcut::DeleteSafe),
        (idle && safe_ticked > 0).then_some(Message::DeleteSafe),
        Tone::Danger,
        palette,
    )]
    .spacing(spacing::L)
    .align_y(alignment::Vertical::Center);
    if !ctx.store_build {
        line = line.push(kit::key_button(
            &fill_size(
                Msg::SpaceRemoveWorktrees,
                state.open_ticked_bytes(Group::Worktrees),
                ctx.lang,
            ),
            &ctx.keys.label(Shortcut::RemoveWorktrees),
            (idle && worktrees_ticked > 0).then_some(Message::RemoveWorktrees),
            Tone::DangerOutline,
            palette,
        ));
    }
    if let Some(running) = state.running() {
        let fraction = running.done as f32 / running.total.max(1) as f32;
        line = line.push(container(kit::progress_bar(fraction, palette)).width(size::FIELD_WIDE));
    }
    line = line
        .push(Space::new().width(Length::Fill))
        .push(kit::button(
            ctx.lang.text(Msg::SpaceRulesOpen),
            Some(Message::ShowRules),
            Tone::Secondary,
            palette,
        ))
        .push(kit::key_button(
            ctx.lang.text(Msg::SpaceAsk),
            &ctx.keys.label(Shortcut::AskHermes),
            Some(Message::AskHermes),
            Tone::Secondary,
            palette,
        ));
    container(line)
        .padding([spacing::M, spacing::XXL])
        .height(size::HEADER + spacing::XL)
        .into()
}

/// The parts of the drive after a run, in the order of the bar: what is left of each group,
/// what the run freed, what is not in a group, and what was free before.
fn result_parts(
    state: &State,
    done: &ResultState,
    total: u64,
    palette: &Palette,
) -> Vec<(Msg, u64, Color)> {
    let left = |group: Group| {
        state
            .group_bytes(group)
            .saturating_sub(state.freed_in(group))
    };
    let in_groups: u64 = [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse]
        .iter()
        .map(|group| left(*group))
        .sum();
    let freed = done.freed();
    let was_free = done.free_before;
    let other = total
        .saturating_sub(in_groups)
        .saturating_sub(freed)
        .saturating_sub(was_free);
    vec![
        (
            Msg::SpaceLegendSafeLeft,
            left(Group::Safe),
            group_color(Group::Safe, palette),
        ),
        (
            Msg::SpaceLegendFreed,
            freed,
            group_color(Group::Safe, palette),
        ),
        (
            Msg::SpaceLegendWorktrees,
            left(Group::Worktrees),
            group_color(Group::Worktrees, palette),
        ),
        (
            Msg::SpaceLegendDecide,
            left(Group::Decide),
            group_color(Group::Decide, palette),
        ),
        (
            Msg::SpaceLegendInUse,
            left(Group::InUse),
            group_color(Group::InUse, palette),
        ),
        (
            Msg::SpaceLegendOther,
            other,
            ncrs_ui_kit::theme::space::rest(palette),
        ),
        (Msg::SpaceLegendWasFree, was_free, palette.dim_text),
    ]
}

/// The bar after a run, with what the run freed in stripes.
fn result_bar<'a>(parts: &[(Msg, u64, Color)], palette: &Palette) -> Element<'a, Message> {
    let free_look = ncrs_ui_kit::theme::space::free(palette);
    let segments: Vec<BarSegment> = parts
        .iter()
        .map(|(label, bytes, color)| BarSegment {
            portion: *bytes,
            color: if *label == Msg::SpaceLegendWasFree {
                free_look
            } else {
                *color
            },
        })
        .collect();
    stacked_bar_striped(&segments, Some(1), palette)
}
