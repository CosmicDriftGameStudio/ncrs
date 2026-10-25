//! The reorganisation plan view: the entries of the plan under the kind of
//! finding they belong to, each with its current path, its proposed path and
//! the reason in plain text, and the folders the selection touches before and
//! after. Modal, like the rule suggestions. Pure view over
//! [`ReorganisationView`]; nothing here touches a file.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Size};
use ncrs_index::{ConflictReason, Finding, PlanOutcome};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, paragraph, Columns, Tone};
use super::theme::{size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, ReorganisationPlanMessage};
use crate::palette::Palette;
use crate::reorganisation_plan::{
    target, Group, Line, ReorganisationView, TreeLine, VISIBLE_LINES,
};
use crate::reorganisation_work::{deciding_finding, PlannedItem};

/// Most lines of a tree shown; the rest is counted.
pub const TREE_LINES: usize = 6;

const SIZE: DialogSize = DialogSize::Card;

/// How the width of an entry is shared: current path, proposed path, reason.
const PORTIONS: [u16; 3] = [5, 5, 7];
/// What the checkbox of a line takes: the box and the room after it.
const CHECK_WIDTH: f32 = 20.0;
/// What an entry keeps free before its columns: one level of indent and its
/// checkbox.
const BOX_WIDTH: f32 = spacing::INDENT + CHECK_WIDTH;
/// [`BOX_WIDTH`] in characters.
const BOX_CHARS: usize = 7;

fn send(message: ReorganisationPlanMessage) -> Message {
    Message::ReorganisationPlan(message)
}

pub fn view<'a>(
    state: &'a ReorganisationView,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![paragraph(
        lang.text(Msg::ReorganisationPlanExplain),
        palette
    )]
    .spacing(spacing::LINE_GAP);

    if state.lines().is_empty() {
        body = body.push(dim(
            lang.text(if state.is_busy() {
                Msg::ReorganisationPlanPlanning
            } else {
                Msg::ReorganisationPlanEmpty
            }),
            palette,
        ));
    } else {
        body = body.push(list(state, line_chars, badges.focus, lang, palette));
        if state.is_busy() {
            body = body.push(dim(lang.text(Msg::ReorganisationPlanPlanning), palette));
        }
        body = body.push(
            container(trees(
                state,
                line_chars.checked_div(2).unwrap_or_default(),
                lang,
                palette,
            ))
            .padding(iced::Padding {
                top: spacing::SECTION_LEAD,
                ..iced::Padding::ZERO
            }),
        );
    }
    if let Some(detail) = state.problem() {
        body = body.push(error_line(
            fill_template(
                lang.text(Msg::ReorganisationPlanProblemFailed),
                &[("{detail}", detail)],
            ),
            palette,
        ));
    }

    Frame::new(
        lang.text(Msg::ActionReorganisationPlan),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::ActionReorganisationPlanTheme))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(
        HermesPage::Reorganisation,
        badges,
        lang,
        palette,
    ))
    .on_close(send(ReorganisationPlanMessage::Close))
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}

/// The room before the columns, empty.
fn box_gap<'a>() -> Element<'a, Message> {
    Space::new().width(BOX_WIDTH).into()
}

/// The checkbox of a line, or its room where the line cannot be chosen.
fn check_cell<'a>(state: Option<kit::Check>, palette: &Palette) -> Element<'a, Message> {
    let content: Element<'a, Message> = match state {
        Some(checked) => kit::checkbox(checked, None, palette),
        None => Space::new().into(),
    };
    container(content).width(CHECK_WIDTH).into()
}

fn group_name(group: Group) -> Msg {
    match group {
        Group::Duplicate => Msg::ReorganisationPlanGroupDuplicate,
        Group::Misplaced => Msg::ReorganisationPlanGroupMisplaced,
        Group::YearMismatch => Msg::ReorganisationPlanGroupYear,
        Group::NameOutlier => Msg::ReorganisationPlanGroupName,
        Group::Blocked => Msg::ReorganisationPlanGroupBlocked,
    }
}

fn group_check(selected: usize, selectable: usize) -> Option<kit::Check> {
    if selectable == 0 {
        None
    } else if selected == 0 {
        Some(kit::Check::Off)
    } else if selected == selectable {
        Some(kit::Check::On)
    } else {
        Some(kit::Check::Mixed)
    }
}

/// The proposed path of an entry: its target, or the one that was in the way.
fn proposed_text(planned: &PlannedItem) -> String {
    let target = match &planned.item.outcome {
        PlanOutcome::Move { target } | PlanOutcome::Rename { target } => Some(target),
        PlanOutcome::Conflict { target, .. } => target.as_ref(),
        PlanOutcome::NoAction(_) => None,
    };
    target
        .map(|proposed| proposed.display().to_string())
        .unwrap_or_default()
}

/// Why the entry is in the plan, or why it cannot be run.
pub fn reason_text(planned: &PlannedItem, lang: Language) -> String {
    match &planned.item.outcome {
        PlanOutcome::Conflict { reason, .. } => lang
            .text(match reason {
                ConflictReason::TargetExists => Msg::ReorganisationPlanReasonTargetExists,
                ConflictReason::TargetTakenByOtherItem => Msg::ReorganisationPlanReasonTargetTaken,
                ConflictReason::TargetLeavesLocation => Msg::ReorganisationPlanReasonTargetLeaves,
            })
            .to_owned(),
        PlanOutcome::NoAction(_) => lang
            .text(Msg::ReorganisationPlanReasonMissingFields)
            .to_owned(),
        PlanOutcome::Move { .. } | PlanOutcome::Rename { .. } => {
            match deciding_finding(&planned.item) {
                Some(Finding::Duplicate { keeper }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonDuplicate),
                    &[("{keeper}", &keeper.display().to_string())],
                ),
                Some(Finding::Misplaced { .. }) => lang
                    .text(if planned.unconfirmed_rule {
                        Msg::ReorganisationPlanReasonMisplacedUnconfirmed
                    } else {
                        Msg::ReorganisationPlanReasonMisplaced
                    })
                    .to_owned(),
                Some(Finding::YearMismatch {
                    folder_year,
                    date_year,
                }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonYear),
                    &[
                        ("{folder_year}", &folder_year.to_string()),
                        ("{date_year}", &date_year.to_string()),
                    ],
                ),
                Some(Finding::NameOutlier { pattern }) => fill_template(
                    lang.text(Msg::ReorganisationPlanReasonName),
                    &[("{pattern}", pattern)],
                ),
                None => String::new(),
            }
        }
    }
}

fn list<'a>(
    state: &'a ReorganisationView,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: line_chars.saturating_sub(BOX_CHARS),
    };
    let whole_line = Columns {
        portions: &[1],
        line_chars: line_chars.saturating_sub(BOX_CHARS),
    };
    let header = row![
        box_gap(),
        columns.cell(
            0,
            lang.text(Msg::ReorganisationPlanColumnCurrent),
            palette.dim_text
        ),
        columns.cell(
            1,
            lang.text(Msg::ReorganisationPlanColumnProposed),
            palette.dim_text
        ),
        columns.cell(
            2,
            lang.text(Msg::ReorganisationPlanColumnReason),
            palette.dim_text
        ),
    ]
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(size::ROW)
    .align_y(alignment::Vertical::Center);

    let first = state.first_visible();
    let shown = state
        .lines()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_LINES);
    let lines = shown.map(|(position, line)| {
        let on_cursor = position == state.cursor();
        let color = if on_cursor && focus == HermesFocus::Content {
            palette.cursor_text
        } else {
            palette.text
        };
        let content = match *line {
            Line::Header(group) => {
                let (selected, selectable) = state.counts(group);
                let mut title = lang.text(group_name(group)).to_owned();
                if selectable > 0 {
                    title = format!(
                        "{title}  {}",
                        fill_template(
                            lang.text(Msg::ReorganisationPlanGroupCount),
                            &[
                                ("{selected}", &selected.to_string()),
                                ("{count}", &selectable.to_string()),
                            ],
                        )
                    );
                }
                row![
                    check_cell(group_check(selected, selectable), palette),
                    whole_line.cell(0, &title, color),
                ]
            }
            Line::Item(position_in_plan) => {
                state
                    .rows()
                    .get(position_in_plan)
                    .map_or_else(row::Row::new, |planned| {
                        let own_check = target(planned).is_some().then(|| {
                            if state.is_selected(position_in_plan) {
                                kit::Check::On
                            } else {
                                kit::Check::Off
                            }
                        });
                        row![
                            Space::new().width(spacing::INDENT),
                            check_cell(own_check, palette),
                            columns.path_cell(0, &planned.item.source.display().to_string(), color),
                            columns.path_cell(1, &proposed_text(planned), color),
                            columns.cell(2, &reason_text(planned, lang), color),
                        ]
                    })
            }
        }
        .align_y(alignment::Vertical::Center);
        kit::list_row(
            content.into(),
            send(ReorganisationPlanMessage::Toggle(position)),
            on_cursor,
            focus == HermesFocus::Content,
            palette,
        )
    });
    kit::scroll_list(
        header.into(),
        lines.collect(),
        kit::ListView {
            len: state.lines().len(),
            visible: VISIBLE_LINES,
            offset: first,
        },
        |delta| send(ReorganisationPlanMessage::Scrolled(delta)),
        palette,
    )
}

fn trees<'a>(
    state: &'a ReorganisationView,
    chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        tree(
            lang.text(Msg::ReorganisationPlanBefore),
            &state.before_tree(),
            chars,
            lang,
            palette
        ),
        tree(
            lang.text(Msg::ReorganisationPlanAfter),
            &state.after_tree(),
            chars,
            lang,
            palette
        ),
    ]
    .spacing(spacing::LINE_GAP)
    .into()
}

fn tree<'a>(
    heading: &'a str,
    lines: &[TreeLine],
    chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut shown = column![dim(heading, palette)]
        .spacing(spacing::XS / 2.0)
        .width(Length::FillPortion(1));
    if lines.is_empty() {
        shown = shown.push(dim(lang.text(Msg::ReorganisationPlanTreeEmpty), palette));
    }
    for line in lines.iter().take(TREE_LINES) {
        let slash = if line.is_folder { "/" } else { "" };
        let content = format!("{}{slash}", line.name);
        let indent = spacing::INDENT * line.depth as f32;
        let room = chars.saturating_sub(line.depth * 3);
        let name = format::ellipsize_end(&content, room);
        shown = shown.push(
            container(if line.is_folder {
                dim(name, palette)
            } else {
                kit::one_line(name, palette)
            })
            .padding(iced::Padding {
                left: indent,
                ..iced::Padding::ZERO
            }),
        );
    }
    let rest = lines.len().saturating_sub(TREE_LINES);
    if rest > 0 {
        shown = shown.push(dim(
            fill_template(
                lang.text(Msg::ReorganisationPlanTreeMore),
                &[("{count}", &rest.to_string())],
            ),
            palette,
        ));
    }
    shown.into()
}

fn buttons<'a>(
    state: &ReorganisationView,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let runnable = state.has_selection() && !state.is_busy();
    vec![
        kit::button(
            lang.text(Msg::ReorganisationPlanClose),
            Some(send(ReorganisationPlanMessage::Close)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::ReorganisationPlanReplan),
            (!state.is_busy()).then(|| send(ReorganisationPlanMessage::Replan)),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::ReorganisationPlanExecute),
            runnable.then(|| Message::ExecuteReorganisation(state.selection())),
            Tone::Primary,
            palette,
        ),
    ]
}
