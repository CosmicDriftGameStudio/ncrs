//! The filing status page: a box over the panels that shows what the filing
//! module is doing. Pure view over [`FilingStatus`].

use std::path::Path;
use std::time::{Duration, SystemTime};

use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Element, Length};
use ncrs_pipeline::{Load, RunnerState, Task, WorkerSnapshot};

use super::format;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::filing::IndexFigures;
use crate::filing_status::FilingStatus;
use crate::i18n::{Language, Msg};
use crate::messages::{FilingStatusMessage, Message};
use crate::palette::Palette;

const WIDTH: f32 = 720.0;
const ERROR_LIST_HEIGHT: f32 = 120.0;
const LABEL_WIDTH: f32 = 130.0;
const WORKER_NUMBER_WIDTH: f32 = 30.0;
const WORKER_TASK_WIDTH: f32 = 170.0;
const DURATION_WIDTH: f32 = 70.0;
const NOTHING_YET: &str = "—";
const NOT_READ_YET: &str = "…";
const SECONDS_PER_HOUR: u64 = 3600;

fn send(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

pub fn view<'a>(
    status: &'a FilingStatus,
    load: Option<Load>,
    index_open: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let title_row = row![
        text(lang.text(Msg::FilingStatusTitle))
            .size(font_size::TITLE)
            .color(palette.accent)
            .width(Length::Fill),
        button(text(lang.text(Msg::FilingStatusClose)))
            .on_press(send(FilingStatusMessage::Close))
            .style(theme::dialog_button(palette, false)),
    ]
    .align_y(iced::alignment::Vertical::Center);

    let body = column![
        title_row,
        text(lang.text(Msg::FilingStatusSubtitle))
            .size(font_size::STATUS)
            .color(palette.dim_text),
        pipeline_section(status, load, lang, palette),
        workers_section(status, lang, palette),
        index_section(status, index_open, lang, palette),
        errors_section(status, lang, palette),
        text(lang.text(Msg::FilingStatusHint))
            .size(font_size::STATUS)
            .color(palette.dim_text),
    ]
    .spacing(spacing::SECTION_GAP);

    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(WIDTH)
        .style(theme::dialog(palette))
        .into()
}

fn heading<'a>(label: Msg, lang: Language, palette: &Palette) -> Element<'a, Message> {
    text(lang.text(label))
        .size(font_size::ROW)
        .color(palette.accent)
        .into()
}

fn labelled<'a>(
    label: Msg,
    value: impl Into<Element<'a, Message>>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        text(lang.text(label))
            .size(font_size::STATUS)
            .color(palette.dim_text)
            .width(LABEL_WIDTH),
        value.into(),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn plain<'a>(value: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(value).size(font_size::STATUS).into()
}

fn state_message(state: RunnerState) -> Msg {
    match state {
        RunnerState::Running => Msg::FilingStatusStateRunning,
        RunnerState::Paused => Msg::FilingStatusStatePaused,
        RunnerState::Idle => Msg::FilingStatusStateIdle,
        RunnerState::Off => Msg::FilingStatusStateOff,
    }
}

fn pipeline_section<'a>(
    status: &'a FilingStatus,
    load: Option<Load>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let snapshot = status.snapshot();
    let running = snapshot.state != RunnerState::Off;
    let pause_label = if snapshot.state == RunnerState::Paused {
        Msg::FilingStatusResume
    } else {
        Msg::FilingStatusPause
    };
    let state_row = row![
        plain(lang.text(state_message(snapshot.state))),
        button(text(lang.text(pause_label)))
            .on_press_maybe(running.then(|| send(FilingStatusMessage::TogglePause)))
            .style(theme::dialog_button(palette, false)),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center);

    let load_button = |label: Msg, target: Load| {
        button(text(lang.text(label)))
            .on_press_maybe(running.then(|| send(FilingStatusMessage::SetLoad(target))))
            .style(theme::dialog_button(palette, load == Some(target)))
    };
    let load_row = row![
        load_button(Msg::FilingStatusLoadGentle, Load::Gentle),
        load_button(Msg::FilingStatusLoadBalanced, Load::Balanced),
        load_button(Msg::FilingStatusLoadFull, Load::Full),
    ]
    .spacing(spacing::CELL_PADDING_X);

    let step_button = |label: &'static str, delta: i8| {
        button(text(label))
            .on_press_maybe(running.then(|| send(FilingStatusMessage::StepWorkers(delta))))
            .style(theme::dialog_button(palette, false))
    };
    let workers_row = row![
        step_button("-", -1),
        plain(snapshot.worker_target.to_string()),
        step_button("+", 1),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center);

    let figure = |found: Option<String>| -> Element<'a, Message> {
        match found {
            Some(text_found) => plain(text_found),
            None if status.awaiting_first_minute() => text(lang.text(Msg::FilingStatusFirstMinute))
                .size(font_size::STATUS)
                .color(palette.dim_text)
                .into(),
            None => plain(NOTHING_YET),
        }
    };
    let throughput = figure(status.documents_per_minute().map(|count| {
        fill_template(
            lang.text(Msg::FilingStatusThroughput),
            &[("{count}", &count.to_string())],
        )
    }));
    let remaining = figure(status.remaining_estimate().map(|left| {
        fill_template(
            lang.text(Msg::FilingStatusRemaining),
            &[("{minutes}", &left.as_secs().div_euclid(60).to_string())],
        )
    }));

    let steps = snapshot.steps.iter().map(|step| {
        let line = fill_template(
            lang.text(Msg::FilingStatusStepCounts),
            &[
                ("{name}", &step_label(step.name, lang)),
                ("{waiting}", &step.waiting.to_string()),
                ("{done}", &step.done.to_string()),
                ("{failed}", &step.failed.to_string()),
                ("{unchanged}", &step.unchanged.to_string()),
            ],
        );
        plain(line)
    });

    column![
        heading(Msg::FilingStatusSectionPipeline, lang, palette),
        labelled(Msg::FilingStatusStateLabel, state_row, lang, palette),
        labelled(Msg::FilingStatusLoadLabel, load_row, lang, palette),
        labelled(Msg::FilingStatusWorkersLabel, workers_row, lang, palette),
        text(lang.text(Msg::FilingStatusLoadExplain))
            .size(font_size::STATUS)
            .color(palette.dim_text),
        text(lang.text(Msg::FilingStatusLoadNote))
            .size(font_size::STATUS)
            .color(palette.dim_text),
        Column::with_children(steps),
        labelled(Msg::FilingStatusThroughputLabel, throughput, lang, palette),
        labelled(Msg::FilingStatusRemainingLabel, remaining, lang, palette),
    ]
    .spacing(spacing::SECTION_GAP / 2.0)
    .into()
}

/// The name a step is shown under. The step's own name is an identifier; one
/// this build has no word for is shown as it is.
fn step_label(name: &str, lang: Language) -> String {
    match name {
        "intake" => lang.text(Msg::FilingStatusStepIntake).to_owned(),
        "review" => lang.text(Msg::FilingStatusStepReview).to_owned(),
        other => other.to_owned(),
    }
}

fn task_label(task: Task, lang: Language) -> String {
    match task {
        Task::Workers => lang.text(Msg::FilingStatusTaskWorkers).to_owned(),
        Task::Watch => lang.text(Msg::FilingStatusTaskWatch).to_owned(),
        Task::Scan => lang.text(Msg::FilingStatusTaskScan).to_owned(),
        Task::Step(name) => step_label(name, lang),
    }
}

/// `m:ss`, and `h:mm:ss` from one hour on.
fn elapsed_label(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let hours = total.div_euclid(SECONDS_PER_HOUR);
    let minutes = total.rem_euclid(SECONDS_PER_HOUR).div_euclid(60);
    let seconds = total.rem_euclid(60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn file_label(file: &Path) -> String {
    file.file_name().map_or_else(
        || file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn worker_row<'a>(
    worker: &WorkerSnapshot,
    taken_at: SystemTime,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let number = text((worker.id + 1).to_string())
        .size(font_size::STATUS)
        .width(WORKER_NUMBER_WIDTH);
    let Some(busy) = &worker.busy else {
        return row![
            number,
            text(lang.text(Msg::FilingStatusWorkerIdle))
                .size(font_size::STATUS)
                .color(palette.dim_text)
        ]
        .spacing(spacing::CELL_PADDING_X)
        .into();
    };
    let elapsed = taken_at.duration_since(busy.started_at).unwrap_or_default();
    row![
        number,
        container(plain(file_label(&busy.file)))
            .width(Length::Fill)
            .clip(true),
        text(task_label(busy.task, lang))
            .size(font_size::STATUS)
            .width(WORKER_TASK_WIDTH),
        text(elapsed_label(elapsed))
            .size(font_size::STATUS)
            .width(DURATION_WIDTH)
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .into()
}

fn workers_section<'a>(
    status: &'a FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rows = status
        .snapshot()
        .workers
        .iter()
        .map(|worker| worker_row(worker, status.taken_at(), lang, palette));
    column![
        heading(Msg::FilingStatusSectionWorkers, lang, palette),
        Column::with_children(rows),
    ]
    .spacing(spacing::SECTION_GAP / 2.0)
    .into()
}

fn yes_no(answer: bool, lang: Language) -> &'static str {
    lang.text(if answer {
        Msg::FilingStatusYes
    } else {
        Msg::FilingStatusNo
    })
}

fn index_rows<'a>(
    figures: &IndexFigures,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let statistics = &figures.statistics;
    vec![
        labelled(
            Msg::FilingStatusIndexPath,
            plain(figures.path.display().to_string()),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexSize,
            plain(
                figures
                    .file_bytes
                    .map_or_else(|| NOTHING_YET.to_owned(), format::size),
            ),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexEncrypted,
            plain(yes_no(figures.encrypted, lang)),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexSchema,
            plain(statistics.schema_version.to_string()),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexDocuments,
            plain(statistics.document_count.to_string()),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexFullText,
            plain(format::size(statistics.full_text_bytes)),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexReviews,
            row![
                plain(format!("{} ·", statistics.open_reviews)),
                text(lang.text(Msg::FilingStatusIndexReviewsExplain))
                    .size(font_size::STATUS)
                    .color(palette.dim_text),
            ]
            .spacing(spacing::CELL_PADDING_X / 2.0),
            lang,
            palette,
        ),
    ]
}

fn index_section<'a>(
    status: &'a FilingStatus,
    index_open: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = match status.index_figures() {
        _ if !index_open => vec![plain(lang.text(Msg::FilingStatusIndexNotOpen))],
        None => vec![plain(NOT_READ_YET)],
        Some(Err(reason)) => vec![plain(fill_template(
            lang.text(Msg::FilingStatusIndexUnavailable),
            &[("{reason}", reason)],
        ))],
        Some(Ok(figures)) => index_rows(figures, lang, palette),
    };
    column![
        heading(Msg::FilingStatusSectionIndex, lang, palette),
        Column::with_children(rows),
    ]
    .spacing(spacing::SECTION_GAP / 2.0)
    .into()
}

fn errors_section<'a>(
    status: &'a FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = status.selected_error();
    let rows: Vec<Element<'a, Message>> = status
        .errors_newest_first()
        .enumerate()
        .map(|(position, error)| {
            button(
                row![
                    container(plain(error.file.display().to_string()))
                        .width(Length::FillPortion(3))
                        .clip(true),
                    text(task_label(error.task, lang))
                        .size(font_size::STATUS)
                        .width(Length::FillPortion(1)),
                    container(plain(error.message.as_str()))
                        .width(Length::FillPortion(3))
                        .clip(true),
                ]
                .spacing(spacing::CELL_PADDING_X),
            )
            .width(Length::Fill)
            .on_press(send(FilingStatusMessage::ErrorActivated(
                error.file.clone(),
            )))
            .style(theme::dialog_button(palette, selected == Some(position)))
            .into()
        })
        .collect();
    let list: Element<'a, Message> = if rows.is_empty() {
        plain(lang.text(Msg::FilingStatusErrorsNone))
    } else {
        scrollable(Column::with_children(rows))
            .height(ERROR_LIST_HEIGHT)
            .into()
    };
    column![heading(Msg::FilingStatusSectionErrors, lang, palette), list,]
        .spacing(spacing::SECTION_GAP / 2.0)
        .into()
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_steps_have_visible_names_and_others_keep_theirs() {
        assert_eq!(step_label("intake", Language::English), "Intake");
        assert_eq!(step_label("intake", Language::German), "Annahme");
        assert_eq!(step_label("review", Language::English), "Pending");
        assert_eq!(step_label("review", Language::German), "Wiedervorlage");
        assert_eq!(step_label("some_step", Language::English), "some_step");
        assert_eq!(step_label("some_step", Language::German), "some_step");
    }
}
