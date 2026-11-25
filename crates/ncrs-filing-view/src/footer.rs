//! The bar under both halves: the figures, the box for the duplicates and the
//! buttons.

use iced::widget::row;
use iced::{alignment, Color, Element};
use ncrs_filing::sorting::RowState;
use ncrs_i18n::{Language, Msg};
use ncrs_index::TargetId;
use ncrs_intake::LocationTransfer;
use ncrs_ui_kit::compare_bar::{mode_tally_bar, Tally};
use ncrs_ui_kit::kit::{
    button, checkbox, key_button, paragraph, Check, Tone, ENTER_KEY,
};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

use crate::message::Message;
use crate::state::{Change, Session};
use crate::texts::{count, open_color, say, target_list};

#[derive(Debug, Default)]
struct Counts {
    read: usize,
    by_rule: usize,
    present: usize,
    no_rule: usize,
    no_text: usize,
    problems: usize,
    /// How many files go to each set of targets.
    several: Vec<(Vec<TargetId>, usize)>,
}

fn counts(session: &Session) -> Counts {
    let mut counts = Counts::default();
    for row in &session.rows {
        if !matches!(row.state, RowState::NotRead(_)) {
            counts.read += 1;
        }
        match session.changes.get(&row.relative) {
            Some(Change::Stay) => continue,
            Some(Change::Places(_)) => {
                counts.by_rule += 1;
                continue;
            }
            None => {}
        }
        match &row.state {
            RowState::Place(_) => counts.by_rule += 1,
            RowState::AlreadyThere { .. } => counts.present += 1,
            RowState::NoRule => counts.no_rule += 1,
            RowState::NoText => counts.no_text += 1,
            RowState::Several(places) => {
                let ids: Vec<TargetId> = places.iter().map(|place| place.target.clone()).collect();
                match counts.several.iter_mut().find(|(known, _)| *known == ids) {
                    Some((_, files)) => *files += 1,
                    None => counts.several.push((ids, 1)),
                }
            }
            RowState::NotRead(_) if session.is_being_read(row) => {}
            RowState::NotRead(_)
            | RowState::NameTaken { .. }
            | RowState::MissingValue { .. }
            | RowState::BadName { .. } => counts.problems += 1,
            RowState::SetAside => {}
        }
    }
    counts
}

/// The figures of the bar, in the order shown.
pub fn figures(session: &Session, lang: Language, palette: &Palette) -> Vec<(String, Color)> {
    if let Some(running) = &session.running {
        return vec![(
            progress(lang, Msg::FilingViewRunFiling, running.done(), running.total()),
            palette.text,
        )];
    }
    if let Some(outcome) = &session.outcome {
        let mut shown = vec![(
            count(lang, Msg::FilingViewRunFiled, outcome.filed),
            palette.success,
        )];
        if outcome.trashed > 0 {
            shown.push((
                count(lang, Msg::FilingViewRunTrashed, outcome.trashed),
                palette.text,
            ));
        }
        if outcome.left > 0 {
            shown.push((
                count(lang, Msg::FilingViewRunLeft, outcome.left),
                palette.text,
            ));
        }
        return shown;
    }
    let counts = counts(session);
    let read = if session.reading {
        progress(
            lang,
            Msg::FilingViewFigureReading,
            session.progress.done(),
            session.progress.total(),
        )
    } else {
        count(lang, Msg::SortingFigureRead, counts.read)
    };
    let mut shown = vec![(read, palette.text)];
    let tallies = [
        (Msg::SortingFigureByRule, counts.by_rule, palette.success),
        (Msg::SortingFigurePresent, counts.present, palette.dim_text),
        (Msg::SortingFigureNoRule, counts.no_rule, open_color(palette)),
        (Msg::SortingFigureNoText, counts.no_text, palette.error),
    ];
    shown.extend(
        tallies
            .into_iter()
            .filter(|(_, files, _)| session.once || *files > 0)
            .map(|(msg, files, color)| (count(lang, msg, files), color)),
    );
    for (ids, files) in counts.several {
        shown.push((
            say(
                lang,
                Msg::SortingFigureSeveral,
                &[
                    ("count", &files.to_string()),
                    ("targets", &target_list(lang, session, ids.iter())),
                ],
            ),
            open_color(palette),
        ));
    }
    if counts.problems > 0 {
        shown.push((
            count(lang, Msg::FilingViewFigureProblems, counts.problems),
            palette.error,
        ));
    }
    shown
}

fn progress(lang: Language, msg: Msg, done: usize, total: usize) -> String {
    say(
        lang,
        msg,
        &[
            ("done", &done.to_string()),
            ("total", &total.to_string()),
        ],
    )
}

fn carry_button<'a>(
    session: &Session,
    transfer: LocationTransfer,
    files: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let label = match transfer {
        LocationTransfer::Copy => Msg::SortingButtonCopy,
        LocationTransfer::Move => Msg::SortingButtonMove,
    };
    let (key, tone) = if transfer == session.source.transfer {
        (ENTER_KEY, Tone::Primary)
    } else {
        ("⇧↵", Tone::Secondary)
    };
    key_button(
        &count(lang, label, files),
        key,
        session.can_file().then_some(Message::File(transfer)),
        tone,
        palette,
    )
}

pub fn footer<'a>(session: &Session, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let tallies = figures(session, lang, palette)
        .into_iter()
        .map(|(text, color)| Tally { text, color })
        .collect();
    let files = session.entries().len();
    let is_over = session.outcome.is_some() && files == 0;
    let mut controls: Vec<Element<'a, Message>> = Vec::new();
    if session.once && !is_over {
        let possible =
            !session.copy_only() && session.trash_can_restore && session.running.is_none();
        controls.push(
            row![
                checkbox(
                    if session.trash_dupes {
                        Check::On
                    } else {
                        Check::Off
                    },
                    possible.then_some(Message::ToggleTrashDupes),
                    palette,
                ),
                paragraph(lang.text(Msg::FilingViewTrashDupes), palette),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center)
            .into(),
        );
    }
    if !session.waiting.is_empty() || session.error.is_some() {
        controls.push(button(
            lang.text(Msg::FilingViewRecheck),
            (!session.planning && !session.is_filing()).then_some(Message::Recheck),
            Tone::Secondary,
            palette,
        ));
    }
    let close = if session.once && session.outcome.is_none() {
        Msg::DialogCancel
    } else {
        Msg::DialogClose
    };
    controls.push(button(
        lang.text(close),
        Some(Message::Cancel),
        Tone::Secondary,
        palette,
    ));
    if session.once && !is_over {
        for transfer in [LocationTransfer::Copy, LocationTransfer::Move] {
            controls.push(carry_button(session, transfer, files, lang, palette));
        }
    }
    mode_tally_bar(lang.text(Msg::FilingViewMode), tallies, controls, palette)
}
