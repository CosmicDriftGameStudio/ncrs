//! The filing status page: a box over the panels that shows what the filing
//! module is doing. Pure view over [`FilingStatus`].

use std::path::Path;
use std::time::{Duration, SystemTime};

use iced::widget::{column, container, row, scrollable, Column};
use iced::{Element, Length, Size};
use ncrs_pipeline::{Load, RunnerState, Task, WorkerSnapshot};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, one_line, paragraph, section, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::filing::{AiStatus, IndexFigures};
use crate::filing_status::FilingStatus;
use crate::i18n::{Language, Msg};
use crate::messages::{FilingStatusMessage, HermesPage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Card;
const ERROR_LIST_HEIGHT: f32 = 120.0;
/// File and message of an error row take three of seven parts each.
const ERROR_TEXT_SHARE: f32 = 3.0 / 7.0;
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

/// How many characters the one-line texts of the page hold, from the width
/// the frame gives its content. These texts are cut, never wrapped.
struct LineBudget {
    worker_file: usize,
    index_path: usize,
    error_file: usize,
    error_message: usize,
}

impl LineBudget {
    fn of(window: Size) -> Self {
        let width = frame::content_width(SIZE, window) - frame::SIDEBAR_ROOM;
        let gap = spacing::CELL_PADDING_X;
        let worker_fixed = WORKER_NUMBER_WIDTH + WORKER_TASK_WIDTH + DURATION_WIDTH + 3.0 * gap;
        // Three parts of seven each for file and message, one for the task; the
        // button around the row pads both sides.
        let error_width = width - 2.0 * spacing::CELL_PADDING_X - 2.0 * gap;
        Self {
            worker_file: frame::chars_that_fit(width - worker_fixed),
            index_path: frame::chars_that_fit(width - LABEL_WIDTH - gap),
            error_file: frame::chars_that_fit(error_width * ERROR_TEXT_SHARE),
            error_message: frame::chars_that_fit(error_width * ERROR_TEXT_SHARE),
        }
    }
}

pub fn view<'a>(
    status: &'a FilingStatus,
    load: Option<Load>,
    index_open: bool,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let budget = LineBudget::of(window);
    let body = column![
        pipeline_section(status, load, lang, palette),
        workers_section(status, &budget, lang, palette),
        index_section(status, index_open, &budget, lang, palette),
        errors_section(status, &budget, lang, palette),
        dim(lang.text(Msg::FilingStatusHint), palette),
    ]
    .spacing(spacing::SECTION_GAP);

    Frame::new(
        lang.text(Msg::FilingStatusSubtitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .subtitle(lang.text(Msg::FilingStatusTitle))
    .sidebar(hermes::nav(HermesPage::Inspection, badges, lang, palette))
    .on_close(send(FilingStatusMessage::Close))
    // Enter opens the selected error, which the content defines.
    .hints(KeyHints::default())
    .into_element()
}

fn labelled<'a>(
    label: Msg,
    value: impl Into<Element<'a, Message>>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        container(dim(lang.text(label), palette)).width(LABEL_WIDTH),
        value.into(),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center)
    .into()
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
        paragraph(lang.text(state_message(snapshot.state)), palette),
        kit::button(
            lang.text(pause_label),
            running.then(|| send(FilingStatusMessage::TogglePause)),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center);

    let loads = [
        (Msg::FilingStatusLoadGentle, Load::Gentle),
        (Msg::FilingStatusLoadBalanced, Load::Balanced),
        (Msg::FilingStatusLoadFull, Load::Full),
    ];
    let load_options: Vec<(&str, Message)> = loads
        .iter()
        .map(|(label, target)| {
            (
                lang.text(*label),
                send(FilingStatusMessage::SetLoad(*target)),
            )
        })
        .collect();
    let selected_load = loads
        .iter()
        .position(|(_, target)| load == Some(*target))
        .unwrap_or(loads.len());
    let load_row = kit::segmented(&load_options, selected_load, running, palette);

    let step_button = |label: &str, delta: i8| {
        kit::button(
            label,
            running.then(|| send(FilingStatusMessage::StepWorkers(delta))),
            Tone::Secondary,
            palette,
        )
    };
    let workers_row = row![
        step_button("-", -1),
        paragraph(snapshot.worker_target.to_string(), palette),
        step_button("+", 1),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(iced::alignment::Vertical::Center);

    let figure = |found: Option<String>| -> Element<'a, Message> {
        match found {
            Some(text_found) => paragraph(text_found, palette),
            None if status.awaiting_first_minute() => {
                dim(lang.text(Msg::FilingStatusFirstMinute), palette)
            }
            None => paragraph(NOTHING_YET, palette),
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
        paragraph(line, palette)
    });

    let content = column![
        labelled(Msg::FilingStatusStateLabel, state_row, lang, palette),
        labelled(Msg::FilingStatusLoadLabel, load_row, lang, palette),
        labelled(Msg::FilingStatusWorkersLabel, workers_row, lang, palette),
        dim(lang.text(Msg::FilingStatusLoadExplain), palette),
        dim(lang.text(Msg::FilingStatusLoadNote), palette),
        Column::with_children(steps),
        labelled(Msg::FilingStatusThroughputLabel, throughput, lang, palette),
        labelled(Msg::FilingStatusRemainingLabel, remaining, lang, palette),
    ]
    .extend(ai_row(status.ai(), lang, palette))
    .spacing(spacing::SECTION_GAP / 2.0);
    section(
        lang.text(Msg::FilingStatusSectionPipeline),
        content,
        palette,
    )
}

/// Not there at all while the AI is off, so a setup without it looks as before.
fn ai_row<'a>(ai: &AiStatus, lang: Language, palette: &Palette) -> Option<Element<'a, Message>> {
    let value = match ai {
        AiStatus::Off => return None,
        AiStatus::NotDownloaded => lang.text(Msg::FilingStatusAiNotDownloaded).to_owned(),
        AiStatus::NotLoadedYet => lang.text(Msg::FilingStatusAiNotLoaded).to_owned(),
        AiStatus::Loaded { name, memory_bytes } => fill_template(
            lang.text(Msg::FilingStatusAiLoaded),
            &[("{name}", name), ("{memory}", &format::size(*memory_bytes))],
        ),
        AiStatus::Failed(reason) => fill_template(
            lang.text(Msg::FilingStatusAiFailed),
            &[("{reason}", reason)],
        ),
        AiStatus::Http { origin, model } => fill_template(
            lang.text(Msg::FilingStatusAiHttp),
            &[("{endpoint}", origin), ("{model}", model)],
        ),
    };
    Some(labelled(
        Msg::FilingStatusAiLabel,
        paragraph(value, palette),
        lang,
        palette,
    ))
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
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let number =
        container(paragraph((worker.id + 1).to_string(), palette)).width(WORKER_NUMBER_WIDTH);
    let Some(busy) = &worker.busy else {
        return row![number, dim(lang.text(Msg::FilingStatusWorkerIdle), palette)]
            .spacing(spacing::CELL_PADDING_X)
            .into();
    };
    let elapsed = taken_at.duration_since(busy.started_at).unwrap_or_default();
    row![
        number,
        container(one_line(
            format::ellipsize_end(&file_label(&busy.file), budget.worker_file),
            palette
        ))
        .width(Length::Fill)
        .clip(true),
        container(one_line(task_label(busy.task, lang), palette)).width(WORKER_TASK_WIDTH),
        container(paragraph(elapsed_label(elapsed), palette))
            .width(DURATION_WIDTH)
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .into()
}

fn workers_section<'a>(
    status: &'a FilingStatus,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rows = status
        .snapshot()
        .workers
        .iter()
        .map(|worker| worker_row(worker, status.taken_at(), budget, lang, palette));
    section(
        lang.text(Msg::FilingStatusSectionWorkers),
        Column::with_children(rows),
        palette,
    )
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
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let statistics = &figures.statistics;
    vec![
        labelled(
            Msg::FilingStatusIndexPath,
            one_line(
                format::ellipsize_middle(&figures.path.display().to_string(), budget.index_path),
                palette,
            ),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexSize,
            paragraph(
                figures
                    .file_bytes
                    .map_or_else(|| NOTHING_YET.to_owned(), format::size),
                palette,
            ),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexEncrypted,
            paragraph(yes_no(figures.encrypted, lang), palette),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexSchema,
            paragraph(statistics.schema_version.to_string(), palette),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexDocuments,
            paragraph(statistics.document_count.to_string(), palette),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexFullText,
            paragraph(format::size(statistics.full_text_bytes), palette),
            lang,
            palette,
        ),
        labelled(
            Msg::FilingStatusIndexReviews,
            row![
                paragraph(format!("{} ·", statistics.open_reviews), palette),
                dim(lang.text(Msg::FilingStatusIndexReviewsExplain), palette),
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
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = match status.index_figures() {
        _ if !index_open => vec![paragraph(lang.text(Msg::FilingStatusIndexNotOpen), palette)],
        None => vec![paragraph(NOT_READ_YET, palette)],
        Some(Err(reason)) => vec![paragraph(
            fill_template(
                lang.text(Msg::FilingStatusIndexUnavailable),
                &[("{reason}", reason)],
            ),
            palette,
        )],
        Some(Ok(figures)) => index_rows(figures, budget, lang, palette),
    };
    section(
        lang.text(Msg::FilingStatusSectionIndex),
        Column::with_children(rows),
        palette,
    )
}

fn errors_section<'a>(
    status: &'a FilingStatus,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = status.selected_error();
    let rows: Vec<Element<'a, Message>> = status
        .errors_newest_first()
        .enumerate()
        .map(|(position, error)| {
            let content = row![
                container(one_line(
                    format::ellipsize_middle(&error.file.display().to_string(), budget.error_file),
                    palette,
                ))
                .width(Length::FillPortion(3))
                .clip(true),
                container(one_line(task_label(error.task, lang), palette))
                    .width(Length::FillPortion(1)),
                container(one_line(
                    format::ellipsize_end(&error.message, budget.error_message),
                    palette,
                ))
                .width(Length::FillPortion(3))
                .clip(true),
            ]
            .spacing(spacing::CELL_PADDING_X)
            .align_y(iced::alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(FilingStatusMessage::ErrorActivated(error.file.clone())),
                selected == Some(position),
                palette,
            )
        })
        .collect();
    let list: Element<'a, Message> = if rows.is_empty() {
        paragraph(lang.text(Msg::FilingStatusErrorsNone), palette)
    } else {
        scrollable(Column::with_children(rows))
            .height(ERROR_LIST_HEIGHT)
            .into()
    };
    section(lang.text(Msg::FilingStatusSectionErrors), list, palette)
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
