//! The left half: every file of the source with what happens to it.

use std::path::PathBuf;

use iced::widget::{column, row};
use iced::{alignment, Color, Element};
use ncrs_filing::sorting::{RowPlace, RowState};
use ncrs_i18n::{Language, Msg};
use ncrs_index::{Field, TargetId};
use ncrs_ui_kit::format::{ellipsize_middle, shown_path};
use ncrs_ui_kit::frame::{chars_that_fit, AVERAGE_GLYPH_WIDTH};
use ncrs_ui_kit::hermes_kit::target_chip;
use ncrs_ui_kit::kit::dim_line;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::plan_rows::{
    marked_row, pane_scroll, plan_action_row, plan_header, plan_side_line, PlanActionRow,
    PlanFileRow, PlanLink,
};
use ncrs_ui_kit::theme::spacing;

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::scroll::{LEFT_ROW_ID, LEFT_SCROLL_ID};
use crate::state::{Change, Link, Row, Session};
use crate::texts::{
    below_target, field_name, open_color, read_problem, say, target_list, HEADER_MARGIN,
};

/// Past every colour of the targets: the plain grey chip.
const GREY: usize = usize::MAX;

/// What happens to a file, as a line.
pub fn what_happens(
    session: &Session,
    row: &Row,
    lang: Language,
    palette: &Palette,
) -> (String, Color) {
    match session.changes.get(&row.relative) {
        Some(Change::Stay) => (
            by_user(lang, lang.text(Msg::FilingViewStateStay)),
            palette.dim_text,
        ),
        Some(Change::Places(places)) => (
            by_user(lang, &chosen_places(session, places)),
            palette.success,
        ),
        None => planned(session, row, lang, palette),
    }
}

fn by_user(lang: Language, line: &str) -> String {
    format!("{line}{}", lang.text(Msg::FilingViewByUser))
}

fn chosen_places(session: &Session, places: &[(TargetId, PathBuf)]) -> String {
    let shown: Vec<String> = places
        .iter()
        .map(|(id, relative)| {
            format!(
                "\u{2192} {} {}",
                session.target_number(id),
                relative.display()
            )
        })
        .collect();
    shown.join("  ")
}

fn place_line(session: &Session, place: &RowPlace, lang: Language) -> String {
    say(
        lang,
        Msg::SortingStateRule,
        &[
            ("rule", session.rule_name(place.rule)),
            ("target", &session.target_number(&place.target)),
            ("path", &place.relative.display().to_string()),
        ],
    )
}

fn planned(session: &Session, row: &Row, lang: Language, palette: &Palette) -> (String, Color) {
    let plain = |msg: Msg, color: Color| (lang.text(msg).to_owned(), color);
    match &row.state {
        RowState::Place(place) => (place_line(session, place, lang), palette.success),
        RowState::Several(places) => (
            say(
                lang,
                Msg::SortingStateSeveral,
                &[(
                    "targets",
                    &target_list(lang, session, places.iter().map(|place| &place.target)),
                )],
            ),
            if session.copies_to_all() {
                palette.success
            } else {
                open_color(palette)
            },
        ),
        RowState::NameTaken { place, .. } => (
            say(
                lang,
                Msg::FilingViewStateNameTaken,
                &[
                    ("target", &session.target_number(&place.target)),
                    ("path", &place.relative.display().to_string()),
                ],
            ),
            palette.error,
        ),
        RowState::NoRule if session.once => plain(Msg::SortingStateNoRule, open_color(palette)),
        // More links follow there.
        RowState::NoRule => plain(Msg::FilingViewStateNoRuleShort, open_color(palette)),
        RowState::NoText if session.asks_without_text() => {
            plain(Msg::SortingStateNoTextAsk, palette.error)
        }
        RowState::NoText => plain(Msg::SortingStateNoText, palette.error),
        RowState::MissingValue { field, .. } => (missing_value(*field, lang), open_color(palette)),
        RowState::BadName { rule, .. } => (
            say(
                lang,
                Msg::FilingViewStateBadName,
                &[("rule", session.rule_name(*rule))],
            ),
            palette.error,
        ),
        RowState::AlreadyThere { target, path } => (
            say(
                lang,
                Msg::SortingStatePresent,
                &[
                    ("target", &session.target_number(target)),
                    ("path", &below_target(session, target, path)),
                ],
            ),
            palette.dim_text,
        ),
        RowState::NotRead(_) if session.is_being_read(row) => {
            plain(Msg::FilingViewStateLooking, palette.text)
        }
        RowState::NotRead(problem) => (
            say(
                lang,
                Msg::FilingViewStateNotRead,
                &[("reason", &read_problem(lang, problem))],
            ),
            palette.error,
        ),
        RowState::SetAside => (
            lang.text(Msg::FilingViewStateSetAside).to_owned(),
            palette.dim_text,
        ),
    }
}

fn missing_value(field: Field, lang: Language) -> String {
    let block = format!("{{{}}}", field.as_str());
    if field == Field::Date {
        return say(lang, Msg::SortingStateNoDate, &[("block", &block)]);
    }
    say(
        lang,
        Msg::FilingViewStateMissingValue,
        &[("field", lang.text(field_name(field))), ("block", &block)],
    )
}

/// What a link says and sends.
pub fn link_of(link: Link, row: &Row) -> (Msg, Message) {
    let from = row.relative.clone();
    match link {
        Link::MakeRule => (Msg::SortingLinkMakeRule, Message::MakeRule(from)),
        Link::SuggestRule => (Msg::SortingLinkSuggestRule, Message::SuggestRule(from)),
        Link::Assign => (Msg::FilingViewLinkAssign, Message::Assign(from)),
        Link::Leave => (Msg::FilingViewLinkLeave, Message::Leave(from)),
        Link::Ask => (Msg::SortingLinkAsk, Message::Ask(from)),
    }
}

fn file_row<'a>(
    session: &Session,
    row: &Row,
    half_width: f32,
    ctx: ViewCtx<'_>,
) -> Element<'a, Message> {
    let (what, what_color) = what_happens(session, row, ctx.lang, ctx.palette);
    let chosen = session.selected.as_ref() == Some(&row.relative);
    let links = session
        .links(row)
        .into_iter()
        .map(|link| {
            let (label, on_press) = link_of(link, row);
            PlanLink {
                label: ctx.lang.text(label).to_owned(),
                on_press,
            }
        })
        .collect();
    let shown = plan_action_row(
        PlanActionRow {
            file: PlanFileRow {
                name: row.relative.display().to_string(),
                what,
                what_color,
                chip: None,
                chosen,
                on_press: Message::Select(row.relative.clone()),
            },
            links,
            marked_link: session.marked_link.filter(|_| chosen),
        },
        half_width,
        ctx.palette,
    );
    if chosen {
        marked_row(shown, LEFT_ROW_ID)
    } else {
        shown
    }
}

fn header<'a>(session: &Session, half_width: f32, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let path = format!(
        "{} {}",
        ctx.lang.text(Msg::SortingPlanSource),
        shown_path(&session.source.path, ctx.home)
    );
    let mut line = row![]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    let include = session.source.include.trim();
    let glob_width = include.chars().count() as f32 * AVERAGE_GLYPH_WIDTH + HEADER_MARGIN;
    line = line.push(dim_line(
        ellipsize_middle(&path, chars_that_fit(half_width - glob_width)),
        ctx.palette,
    ));
    if !include.is_empty() {
        line = line.push(target_chip(GREY, include, ctx.palette));
    }
    plan_side_line(line.into())
}

pub fn left<'a>(session: &Session, half_width: f32, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = session
        .rows
        .iter()
        .map(|row| file_row(session, row, half_width, ctx))
        .collect();
    if rows.is_empty() && session.planned && !session.planning {
        rows.push(plan_side_line(dim_line(
            ctx.lang.text(Msg::FilingViewEmpty).to_owned(),
            ctx.palette,
        )));
    }
    column![
        header(session, half_width, ctx),
        plan_header(
            ctx.lang.text(Msg::ColumnName),
            ctx.lang.text(Msg::SortingPlanColumnWhat),
            ctx.palette,
        ),
        pane_scroll(LEFT_SCROLL_ID, rows),
    ]
    .into()
}
