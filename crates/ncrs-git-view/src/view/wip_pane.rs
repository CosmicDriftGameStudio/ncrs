//! The right side while the working copy row is chosen: the files not staged
//! and staged, each with its buttons, and the commit box.

use iced::widget::{column, container, row, Column, Space};
use iced::{alignment, Element, Length};
use ncrs_git::ChangedFile;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::{action_row, ActionRow};
use ncrs_ui_kit::format::{estimated_width, fit_middle};
use ncrs_ui_kit::git_rows::{ref_chip, RefChipKind};
use ncrs_ui_kit::kit::{focused, small_button, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::section_head::{section_head_with, SectionHead};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use super::changes::kind_letter;
use super::commit_box;
use crate::actions::{ChangeSide, LocalChanges, WipFocus};
use crate::ctx::ViewCtx;
use crate::handlers::current_branch;
use crate::message::Message;
use crate::state::{Area, State};

/// Room the scroll bar and the gaps of a row take from its path.
const ROW_OVERHEAD: f32 = 56.0;

fn file_rows<'a>(
    files: &'a [ChangedFile],
    side: ChangeSide,
    local: &'a LocalChanges,
    (width, active): (f32, bool),
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    Column::with_children(files.iter().enumerate().map(|(index, file)| {
        let mut labels = Vec::new();
        let mut actions = Vec::new();
        let lit = active && local.selected == Some((side, index));
        let mut add = |label: &str, message: Message, tone: Tone| {
            labels.push(label.to_owned());
            let tone = if lit { Tone::OnAccent } else { tone };
            actions.push(small_button(label, Some(message), tone, palette));
        };
        match side {
            ChangeSide::Unstaged => {
                add(
                    lang.text(Msg::GitViewStage),
                    Message::Stage(index),
                    Tone::Secondary,
                );
                if file.untracked {
                    add(
                        lang.text(Msg::GitViewIgnore),
                        Message::IgnoreFile(index),
                        Tone::Secondary,
                    );
                }
                add(
                    lang.text(Msg::GitViewDiscardFile),
                    Message::DiscardFile(index),
                    Tone::DangerOutline,
                );
            }
            ChangeSide::Staged => add(
                lang.text(Msg::GitViewUnstage),
                Message::Unstage(index),
                Tone::Secondary,
            ),
        }
        let trail = vec![
            (format!("+{}", file.added), palette.success),
            (format!("\u{2212}{}", file.removed), palette.error),
        ];
        let taken: f32 = labels
            .iter()
            .map(|label| estimated_width(label, font_size::SMALL) + 2.0 * (spacing::M - 2.0) + spacing::M)
            .chain(trail.iter().map(|(count, _)| estimated_width(count, font_size::SMALL) + spacing::M))
            .sum();
        action_row(
            ActionRow {
                lead: Some((kind_letter(file.kind).to_owned(), palette.text)),
                label: fit_middle(&file.path, width - taken - ROW_OVERHEAD, font_size::BODY),
                trail,
                actions,
                chosen: local.selected == Some((side, index)),
                active,
                on_press: Message::SelectChange(side, index),
            },
            palette,
        )
    }))
    .into()
}

fn empty_line<'a>(text: &str, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    container(page_line(
        text.to_owned(),
        font_size::SMALL,
        ctx.palette.dim_text,
    ))
    .padding([spacing::XS, spacing::M])
    .into()
}

fn header<'a>(
    state: &'a State,
    local: &'a LocalChanges,
    width: f32,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let mut summary = fill_template(
        lang.text(Msg::GitViewWipSummary),
        &[
            (
                "{changed}",
                &(local.unstaged.len() + local.staged.len()).to_string(),
            ),
            ("{staged}", &local.staged.len().to_string()),
        ],
    );
    if let Some(worktree) = state.current_worktree() {
        let template = lang.text(Msg::GitViewWipWorktree);
        let frame = fill_template(template, &[("{path}", "")]);
        let room =
            width - 2.0 * spacing::M - estimated_width(&format!("{summary} \u{b7} {frame}"), font_size::SMALL);
        let path = worktree.path.display().to_string();
        summary = format!(
            "{summary} \u{b7} {}",
            fill_template(
                template,
                &[("{path}", &fit_middle(&path, room, font_size::SMALL))],
            )
        );
    }
    let head = column![
        row![
            page_line(
                lang.text(Msg::GitViewWipOn).to_owned(),
                font_size::BODY,
                palette.text
            ),
            ref_chip(current_branch(state), RefChipKind::Head, palette),
        ]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center),
        page_line(summary, font_size::SMALL, palette.dim_text),
    ]
    .spacing(spacing::XS);
    container(head).padding(spacing::M).into()
}

fn heading<'a>(
    side: ChangeSide,
    local: &'a LocalChanges,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let (title, count, actions) = match side {
        ChangeSide::Unstaged => (
            Msg::GitViewChangesUnstaged,
            local.unstaged.len(),
            vec![
                small_button(
                    lang.text(Msg::GitViewDiscardAll),
                    (!local.unstaged.is_empty()).then_some(Message::DiscardAll),
                    Tone::DangerOutline,
                    palette,
                ),
                small_button(
                    lang.text(Msg::GitViewStageAll),
                    (!local.unstaged.is_empty()).then_some(Message::StageAll),
                    Tone::Secondary,
                    palette,
                ),
            ],
        ),
        ChangeSide::Staged => (
            Msg::GitViewChangesStaged,
            local.staged.len(),
            vec![small_button(
                lang.text(Msg::GitViewUnstageAll),
                (!local.staged.is_empty()).then_some(Message::UnstageAll),
                Tone::Secondary,
                palette,
            )],
        ),
    };
    section_head_with(
        SectionHead {
            title: lang.text(title),
            count,
            open: !local.folded.contains(&side),
            has_focus: false,
            on_toggle: Message::ToggleWipSection(side),
        },
        actions,
        palette,
    )
}

pub fn view<'a>(
    state: &'a State,
    local: &'a LocalChanges,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let rule = || theme::horizontal_rule(palette.border_inactive);
    let has_keys = state.area == Area::Detail;
    let list_ring = |stop: WipFocus| has_keys && !local.diff_open && local.focus == stop;
    let nothing = local.unstaged.is_empty() && local.staged.is_empty();
    let width = super::detail_width(state, ctx);
    let list = |side: ChangeSide, files: &'a [ChangedFile], stop: WipFocus, empty: Msg| {
        let body: Element<'a, Message> = if local.folded.contains(&side) {
            Space::new().into()
        } else if files.is_empty() {
            let text = if nothing && side == ChangeSide::Unstaged {
                Msg::GitViewNothingToCommit
            } else {
                empty
            };
            empty_line(lang.text(text), ctx)
        } else {
            file_rows(files, side, local, (width, list_ring(stop)), ctx)
        };
        focused(
            column![heading(side, local, ctx), body],
            list_ring(stop),
            palette,
        )
    };
    column![
        header(state, local, width, ctx),
        rule(),
        list(
            ChangeSide::Unstaged,
            &local.unstaged,
            WipFocus::Unstaged,
            Msg::GitViewNothingToCommit
        ),
        rule(),
        list(
            ChangeSide::Staged,
            &local.staged,
            WipFocus::Staged,
            Msg::GitViewStagedEmpty
        ),
        rule(),
        Space::new().height(Length::Fill),
        rule(),
        commit_box::view(local, has_keys, ctx),
    ]
    .height(Length::Fill)
    .into()
}
