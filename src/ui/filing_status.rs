//! The filing status page: a box over the panels that shows what the filing
//! module is doing. Pure view over [`FilingStatus`].

use std::path::Path;
use std::time::{Duration, SystemTime};

use iced::widget::{column, container, row, scrollable, Column};
use iced::{Element, Length, Size};
use ncrs_pipeline::{Load, RunnerState, Task, WorkerSnapshot};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints, PageHint};
use super::hermes::{self, Badges};
use super::kit::{self, dim, first_section, form_row, one_line, paragraph, section, Columns, Tone};
use super::theme::{size, spacing};
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
/// The load control has three long labels.
const LOAD_CONTROL_WIDTH: f32 = 380.0;
/// How the width of the step table is shared: the step, then four counts.
const STEP_PORTIONS: [u16; 5] = [2, 1, 1, 1, 1];
/// The worker table: number, file, task, time.
const WORKER_PORTIONS: [u16; 4] = [1, 10, 4, 2];
const NOTHING_YET: &str = "—";
const NOT_READ_YET: &str = "…";
const SECONDS_PER_HOUR: u64 = 3600;

/// The keys of the page, named in the footer where there is room.
const PAGE_HINTS: &[PageHint] = &[
    PageHint {
        key: "P",
        verb: Msg::FilingStatusHintPause,
    },
    PageHint {
        key: "1 2 3",
        verb: Msg::FilingStatusHintLoad,
    },
    PageHint {
        key: "+ -",
        verb: Msg::FilingStatusHintWorkers,
    },
    PageHint {
        key: "↑ ↓",
        verb: Msg::FilingStatusHintErrors,
    },
];

fn send(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

/// How many characters the one-line texts of the page hold, from the width
/// the frame gives its content. These texts are cut, never wrapped.
struct LineBudget {
    table_chars: usize,
    index_path: usize,
    error_file: usize,
    error_message: usize,
}

impl LineBudget {
    fn of(window: Size) -> Self {
        let width = frame::content_width(SIZE, window) - frame::SIDEBAR_ROOM;
        let gap = spacing::CELL_PADDING_X;
        // Three parts of seven each for file and message, one for the task; the
        // button around the row pads both sides.
        let error_width = width - 2.0 * spacing::CELL_PADDING_X - 2.0 * gap;
        Self {
            table_chars: frame::chars_that_fit(width),
            index_path: frame::chars_that_fit(size::CONTROL_WIDTH),
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
        throughput_section(status, lang, palette),
        steps_section(status, &budget, lang, palette),
        workers_section(status, &budget, lang, palette),
        index_section(status, index_open, &budget, lang, palette),
        errors_section(status, &budget, lang, palette),
    ];

    Frame::new(
        lang.text(Msg::HermesNavInspection),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::HermesNavInspectionTheme))
    .sidebar(hermes::nav(HermesPage::Inspection, badges, lang, palette))
    .on_close(send(FilingStatusMessage::Close))
    // Enter opens the selected error, which the content defines.
    .hints(KeyHints::default())
    .page_hints(PAGE_HINTS)
    .into_element()
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
    .spacing(spacing::M)
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
    .spacing(spacing::M)
    .align_y(iced::alignment::Vertical::Center);

    let mut rows = column![
        form_row(
            lang.text(Msg::FilingStatusStateLabel),
            None,
            None,
            state_row.into(),
            palette,
        ),
        kit::form_row_wide(
            lang.text(Msg::FilingStatusLoadLabel),
            None,
            Some(lang.text(Msg::FilingStatusLoadExplain)),
            load_row,
            LOAD_CONTROL_WIDTH,
            palette,
        ),
        form_row(
            lang.text(Msg::FilingStatusWorkersLabel),
            None,
            Some(lang.text(Msg::FilingStatusLoadNote)),
            workers_row.into(),
            palette,
        ),
    ];
    if let Some(ai) = ai_value(status.ai(), lang) {
        rows = rows.push(form_row(
            lang.text(Msg::FilingStatusAiLabel),
            None,
            None,
            paragraph(ai, palette),
            palette,
        ));
    }
    first_section(lang.text(Msg::FilingStatusSectionPipeline), rows, palette)
}

/// The throughput and the time left, as figures.
fn throughput_section<'a>(
    status: &'a FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let figure = |found: Option<String>| match found {
        Some(text_found) => text_found,
        None if status.awaiting_first_minute() => {
            lang.text(Msg::FilingStatusFirstMinute).to_owned()
        }
        None => NOTHING_YET.to_owned(),
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
    section(
        lang.text(Msg::FilingStatusThroughputLabel),
        kit::figures(
            &[
                (lang.text(Msg::FilingStatusThroughputLabel), throughput),
                (lang.text(Msg::FilingStatusRemainingLabel), remaining),
            ],
            palette,
        ),
        palette,
    )
}

/// What each step of the pipeline has waiting, done, failed and unchanged.
fn steps_section<'a>(
    status: &'a FilingStatus,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &STEP_PORTIONS,
        line_chars: budget.table_chars,
    };
    let header = columns.header(
        &[
            "",
            lang.text(Msg::FilingStatusStepWaiting),
            lang.text(Msg::FilingStatusStepDone),
            lang.text(Msg::FilingStatusStepFailed),
            lang.text(Msg::FilingStatusStepUnchanged),
        ],
        palette,
    );
    let lines = status.snapshot().steps.iter().map(|step| {
        columns.row(
            &[
                &step_label(step.name, lang),
                &step.waiting.to_string(),
                &step.done.to_string(),
                &step.failed.to_string(),
                &step.unchanged.to_string(),
            ],
            palette.text,
        )
    });
    section(
        lang.text(Msg::FilingStatusSectionSteps),
        column![header, Column::with_children(lines)],
        palette,
    )
}

/// Not there at all while the AI is off, so a setup without it looks as before.
fn ai_value(ai: &AiStatus, lang: Language) -> Option<String> {
    Some(match ai {
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
    })
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
    columns: &Columns<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let number = (worker.id + 1).to_string();
    let Some(busy) = &worker.busy else {
        return columns.row_colored(&[
            (number.as_str(), palette.text),
            (lang.text(Msg::FilingStatusWorkerIdle), palette.dim_text),
        ]);
    };
    let elapsed = taken_at.duration_since(busy.started_at).unwrap_or_default();
    columns.row(
        &[
            &number,
            &file_label(&busy.file),
            &task_label(busy.task, lang),
            &elapsed_label(elapsed),
        ],
        palette.text,
    )
}

fn workers_section<'a>(
    status: &'a FilingStatus,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &WORKER_PORTIONS,
        line_chars: budget.table_chars,
    };
    let rows = status
        .snapshot()
        .workers
        .iter()
        .map(|worker| worker_row(worker, status.taken_at(), &columns, lang, palette));
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
    let value = |content: String| paragraph(content, palette);
    let row_of = |label: Msg, shown: Element<'a, Message>| {
        form_row(lang.text(label), None, None, shown, palette)
    };
    vec![
        kit::figures(
            &[
                (
                    lang.text(Msg::FilingStatusIndexDocuments),
                    statistics.document_count.to_string(),
                ),
                (
                    lang.text(Msg::FilingStatusIndexFullText),
                    format::size(statistics.full_text_bytes),
                ),
                (
                    lang.text(Msg::FilingStatusIndexReviews),
                    statistics.open_reviews.to_string(),
                ),
            ],
            palette,
        ),
        dim(
            format!(
                "{}: {}",
                lang.text(Msg::FilingStatusIndexReviews),
                lang.text(Msg::FilingStatusIndexReviewsExplain)
            ),
            palette,
        ),
        row_of(
            Msg::FilingStatusIndexPath,
            one_line(
                format::ellipsize_middle(&figures.path.display().to_string(), budget.index_path),
                palette,
            ),
        ),
        row_of(
            Msg::FilingStatusIndexSize,
            value(
                figures
                    .file_bytes
                    .map_or_else(|| NOTHING_YET.to_owned(), format::size),
            ),
        ),
        row_of(
            Msg::FilingStatusIndexEncrypted,
            value(yes_no(figures.encrypted, lang).to_owned()),
        ),
        row_of(
            Msg::FilingStatusIndexSchema,
            value(statistics.schema_version.to_string()),
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
        kit::stack(rows),
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
