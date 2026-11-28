//! The head of the decide pane: where in the drive the pane is, the bar of the drive with its
//! legend, and the buttons to measure again and to show the folder in the panel.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Group;
use ncrs_ui_kit::color_chip::color_chip;
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::stacked_bar::{stacked_bar, BarSegment};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use super::list::group_color;
use crate::ctx::ViewCtx;
use crate::decide::DecideState;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;

/// The bar of the drive in the head.
const DRIVE_BAR_WIDTH: f32 = 420.0;

fn part<'a>(label: &str, value: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    column![
        page_line(label, font_size::SMALL, ctx.palette.dim_text),
        page_line(value, font_size::BODY, ctx.palette.text),
    ]
    .into()
}

fn breadcrumb<'a>(pane: &DecideState, state: &State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let drive = state
        .scan()
        .map(|scan| scan.volume_name.clone())
        .unwrap_or_default();
    let separator = || page_line("\u{203a}", font_size::BODY, ctx.palette.dim_text);
    row![
        part(
            ctx.lang.text(Msg::SpaceDriveLabel),
            format!("{drive} \u{25be}"),
            ctx
        ),
        separator(),
        part(
            ctx.lang.text(Msg::SpaceDecideBreadcrumbGroup),
            ctx.lang.text(Msg::SpaceGroupDecide).to_owned(),
            ctx
        ),
        separator(),
        part(
            ctx.lang.text(Msg::SpaceDecideBreadcrumbFolder),
            shown_path(pane.folder(), ctx.home),
            ctx
        ),
    ]
    .spacing(spacing::L)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The bar cut by group, its legend and how full the drive is, small enough for the head.
fn drive<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(scan) = state.scan() else {
        return Space::new().into();
    };
    let total = scan.volume.total;
    let free = scan.volume.available.min(total);
    let used = total - free;
    let groups = [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse];
    let sizes: Vec<u64> = groups
        .iter()
        .map(|group| state.group_bytes(*group))
        .collect();
    let counted: u64 = sizes.iter().sum();
    let mut segments: Vec<BarSegment> = groups
        .iter()
        .zip(&sizes)
        .map(|(group, bytes)| BarSegment {
            portion: *bytes,
            color: group_color(*group, palette),
        })
        .collect();
    segments.push(BarSegment {
        portion: used.saturating_sub(counted),
        color: theme::space::rest(palette),
    });
    segments.push(BarSegment {
        portion: free,
        color: theme::space::free(palette),
    });
    let labels = [
        Msg::SpaceLegendSafe,
        Msg::SpaceLegendWorktrees,
        Msg::SpaceLegendDecide,
        Msg::SpaceLegendInUse,
    ];
    let chips = row(groups
        .iter()
        .zip(labels)
        .zip(&sizes)
        .filter(|((group, _), bytes)| {
            **bytes > 0 && !(ctx.store_build && **group == Group::Worktrees)
        })
        .map(|((group, label), bytes)| {
            let text = fill_template(
                ctx.lang.text(Msg::SpaceLegendEntry),
                &[
                    ("{label}", ctx.lang.text(label)),
                    ("{size}", &size_shown(*bytes, ctx.lang)),
                ],
            );
            color_chip(&text, group_color(*group, palette), None::<Message>)
        }))
    .spacing(spacing::XS);
    let used_of = fill_template(
        ctx.lang.text(Msg::SpaceUsedOf),
        &[
            ("{used}", &size_shown(used, ctx.lang)),
            ("{total}", &size_shown(total, ctx.lang)),
            ("{free}", &size_shown(free, ctx.lang)),
        ],
    );
    column![
        container(stacked_bar(&segments, palette)).width(DRIVE_BAR_WIDTH),
        row![
            chips,
            page_line(used_of, font_size::SMALL, palette.dim_text)
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(spacing::XS)
    .into()
}

pub fn view<'a>(pane: &'a DecideState, state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    container(
        row![
            breadcrumb(pane, state, ctx),
            drive(state, ctx),
            Space::new().width(Length::Fill),
            kit::key_button(
                ctx.lang.text(Msg::SpaceRescan),
                &ctx.keys.label(Shortcut::Rescan),
                (state.running().is_none()).then_some(Message::Rescan),
                Tone::Secondary,
                palette,
            ),
            kit::key_button(
                ctx.lang.text(Msg::SpaceShowInPanel),
                &ctx.keys.label(Shortcut::ShowInPanel),
                Some(Message::ShowInPanelAtCursor),
                Tone::Secondary,
                palette,
            ),
        ]
        .spacing(spacing::XL)
        .align_y(alignment::Vertical::Center),
    )
    .padding([spacing::M, spacing::XXL])
    .width(Length::Fill)
    .into()
}
