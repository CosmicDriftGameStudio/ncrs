//! The filing status page, a tab: what the filing module is doing, in two
//! columns. Pure view over [`FilingStatus`].

use std::path::Path;
use std::time::{Duration, SystemTime};

use iced::widget::{column, container, row, scrollable, Column};
use iced::{Element, Length, Padding, Size};
use ncrs_index::HermesCounts;
use ncrs_pipeline::{Load, RunnerState, Task, WorkerSnapshot};

use super::format;
use super::frame;
use super::hermes::{self, Badges};
use super::hermes_kit::{self as page_kit, MenuItem, MenuLook};
use super::kit::{self, Check, Columns};
use super::theme::{self, font_size};
use crate::app::fill_template;
use crate::filing::{AiStatus, IndexFigures};
use crate::filing_status::FilingStatus;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{
    FilingStatusMessage, HermesFocus, HermesOverviewMessage, HermesPage, Message,
};
use crate::palette::Palette;

/// How the width of the step table is shared: the step, then four counts.
const STEP_PORTIONS: [u16; 5] = [2, 1, 1, 1, 1];
/// The worker table: number, file, task, time.
const WORKER_PORTIONS: [u16; 4] = [1, 10, 4, 2];
const NOTHING_YET: &str = "—";
const NOT_READ_YET: &str = "…";
const SECONDS_PER_HOUR: u64 = 3600;
/// What the checkbox, the age and the gaps of an error row take, in characters.
const ERROR_ROW_FIXED_CHARS: usize = 10;
/// A monospace character is this many tenths wider than an average one.
const MONO_WIDTH_TENTHS: usize = 13;
/// The room for the button in the action bar.
const ACTION_BUTTON_ROOM: f32 = 220.0;

fn send(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

/// How many characters the tables of the page hold in one line.
struct LineBudget {
    table_chars: usize,
    /// The note in the action bar.
    note_chars: usize,
    /// The path in the PDF library notice.
    notice_path: f32,
}

impl LineBudget {
    fn of(window: Size) -> Self {
        let column = (window.width - page_kit::NAV_WIDTH) / 2.0 - page_kit::GAP_M - page_kit::GAP_L;
        Self {
            table_chars: frame::chars_that_fit(column),
            note_chars: frame::chars_that_fit(
                window.width - page_kit::NAV_WIDTH - ACTION_BUTTON_ROOM - 3.0 * page_kit::GAP_L,
            ),
            notice_path: column - 4.0 * theme::spacing::M,
        }
    }
}

/// What the page shows besides the pipeline status.
pub struct Setting<'a> {
    pub load: Option<Load>,
    pub index_open: bool,
    /// The place the PDF library was searched in, when it is missing.
    pub pdf_library_missing: Option<&'a Path>,
}

/// The page with its navigation, header, two columns and action bar.
pub fn view<'a>(
    status: &'a FilingStatus,
    setting: Setting<'_>,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let budget = LineBudget::of(window);
    let left = scrollable(
        Column::with_children(left_column(status, &setting, &budget, lang, palette))
            .spacing(page_kit::GAP_S)
            .padding(Padding {
                top: page_kit::GAP_M,
                right: page_kit::GAP_M - page_kit::GAP_S - page_kit::GAP_XS,
                bottom: page_kit::GAP_M,
                left: page_kit::GAP_L,
            })
            .width(Length::Fill),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill);
    let right = scrollable(
        Column::with_children(right_column(
            status, &setting, &budget, badges, lang, palette,
        ))
        .spacing(page_kit::GAP_S)
        .width(Length::Fill),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill);
    let body = row![left, right].width(Length::Fill).height(Length::Fill);
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Inspection, badges, lang, palette),
        header(status, lang, palette),
        body.into(),
        Some(action_bar(status, badges, &budget, lang, palette)),
        palette,
    )
}

fn header<'a>(status: &FilingStatus, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let toggle = if status.steps_shown() {
        Msg::FilingStatusHideWorkers
    } else {
        Msg::FilingStatusShowWorkers
    };
    let more = page_kit::menu_button(
        "",
        &[MenuItem::new(
            lang.text(toggle),
            send(FilingStatusMessage::ToggleSteps),
        )],
        MenuLook::Icon,
        palette,
    );
    page_kit::page_header(
        lang.text(Msg::HermesNavInspection),
        lang.futurama_name(FuturamaName::HermesNavInspection),
        Some(lang.text(Msg::FilingStatusExplain).to_owned()),
        None,
        vec![more],
        palette,
    )
}

fn action_bar<'a>(
    status: &FilingStatus,
    badges: Badges,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let again = !status.files_to_process_again().is_empty();
    page_kit::action_bar(
        badges
            .counts
            .map(|counts| format::ellipsize_end(&seen_sentence(&counts, lang), budget.note_chars)),
        vec![page_kit::primary(
            lang.text(Msg::FilingStatusProcessAgain),
            Some(kit::ENTER_KEY),
            again.then(|| send(FilingStatusMessage::ProcessAgain)),
            palette,
        )],
        palette,
    )
}

fn left_column<'a>(
    status: &'a FilingStatus,
    setting: &Setting<'_>,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut children = Vec::new();
    if let Some(searched) = setting.pdf_library_missing {
        children.push(kit::warning_box(
            format!(
                "{}\n{}",
                lang.text(Msg::FilingStatusPdfMissing),
                fill_template(
                    lang.text(Msg::FilingStatusPdfSearched),
                    &[(
                        "{path}",
                        &format::fit_middle(
                            &searched.display().to_string(),
                            budget.notice_path,
                            font_size::BODY,
                        ),
                    )],
                ),
            ),
            palette,
        ));
    }
    children.push(page_kit::section(
        lang.text(Msg::FilingStatusSectionPipeline),
        palette,
    ));
    children.extend(pipeline_rows(status, setting.load, lang, palette));
    children.push(page_kit::section(
        lang.text(Msg::FilingStatusSectionThroughput),
        palette,
    ));
    children.extend(throughput_rows(status, lang, palette));
    children
}

fn right_column<'a>(
    status: &'a FilingStatus,
    setting: &Setting<'_>,
    budget: &LineBudget,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let inset = Padding {
        top: 0.0,
        right: page_kit::GAP_L,
        bottom: 0.0,
        left: page_kit::GAP_M - page_kit::GAP_S - page_kit::GAP_XS,
    };
    let padded = |content: Element<'a, Message>| -> Element<'a, Message> {
        container(content).padding(inset).width(Length::Fill).into()
    };
    let mut children = vec![container(page_kit::section(
        lang.text(Msg::FilingStatusSectionIndex),
        palette,
    ))
    .padding(Padding {
        top: page_kit::GAP_M,
        ..inset
    })
    .width(Length::Fill)
    .into()];
    children.extend(
        index_rows(status, setting.index_open, lang, palette)
            .into_iter()
            .map(padded),
    );
    children.push(padded(page_kit::section(
        lang.text(Msg::FilingStatusSectionErrors),
        palette,
    )));
    children.extend(error_rows(status, badges, budget, lang, palette));
    if let Some(full) = selected_error_line(status, lang, palette) {
        children.push(padded(full));
    }
    if status.steps_shown() {
        children.push(padded(page_kit::section(
            lang.text(Msg::FilingStatusSectionSteps),
            palette,
        )));
        children.push(padded(steps_table(status, budget, lang, palette)));
        children.push(padded(page_kit::section(
            lang.text(Msg::FilingStatusSectionWorkers),
            palette,
        )));
        children.push(padded(workers_table(status, budget, lang, palette)));
    }
    children.push(
        container(iced::widget::Space::new())
            .height(page_kit::GAP_M)
            .into(),
    );
    children
}

/// The first sentence of `counts_sentences`: the documents seen and where they are.
fn seen_sentence(counts: &HermesCounts, lang: Language) -> String {
    let inbox = counts.inbox;
    let seen = inbox.new + inbox.planned + inbox.in_review + inbox.filed;
    fill_template(
        lang.text(Msg::FilingStatusCountsSeen),
        &[
            ("{seen}", &format::grouped(seen)),
            ("{filed}", &format::grouped(inbox.filed)),
            ("{waiting}", &format::grouped(counts.open_reviews)),
        ],
    )
}

/// What Hermes has done so far, in plain sentences: the documents seen and
/// where they are, and why the rest waits.
#[cfg(test)]
pub fn counts_sentences(counts: &HermesCounts, lang: Language) -> String {
    let count = |number: u64| format::grouped(number);
    let mut sentences = vec![seen_sentence(counts, lang)];
    if counts.open_reviews_without_text > 0 {
        sentences.push(fill_template(
            lang.text(Msg::FilingStatusCountsWithoutText),
            &[("{count}", &count(counts.open_reviews_without_text))],
        ));
    }
    if counts.confirmed_rules == 0 && counts.open_reviews > counts.open_reviews_without_text {
        sentences.push(lang.text(Msg::FilingStatusCountsNoRules).to_owned());
    }
    if counts.archive_documents > 0 {
        sentences.push(fill_template(
            lang.text(Msg::FilingStatusCountsArchive),
            &[
                ("{with_text}", &count(counts.archive_documents_with_text)),
                ("{total}", &count(counts.archive_documents)),
            ],
        ));
    }
    sentences.join(" ")
}

fn state_message(state: RunnerState) -> Msg {
    match state {
        RunnerState::Running => Msg::FilingStatusStateRunning,
        RunnerState::Paused => Msg::FilingStatusStatePaused,
        RunnerState::Idle => Msg::FilingStatusStateIdle,
        RunnerState::Off => Msg::FilingStatusStateOff,
    }
}

fn state_word(status: &FilingStatus, lang: Language) -> String {
    let snapshot = status.snapshot();
    let word = lang.text(state_message(snapshot.state));
    let open = snapshot.open_files();
    if snapshot.state == RunnerState::Running && open > 0 {
        let busy = fill_template(
            lang.text(Msg::HermesOverviewInProgress),
            &[("{count}", &open.to_string())],
        );
        format!("{word} · {busy}")
    } else {
        word.to_owned()
    }
}

const LOADS: [(Load, Msg, FuturamaName); 3] = [
    (
        Load::Gentle,
        Msg::FilingStatusLoadGentle,
        FuturamaName::FilingStatusLoadGentle,
    ),
    (
        Load::Balanced,
        Msg::FilingStatusLoadBalanced,
        FuturamaName::FilingStatusLoadBalanced,
    ),
    (
        Load::Full,
        Msg::FilingStatusLoadFull,
        FuturamaName::FilingStatusLoadFull,
    ),
];

/// The name of a load as the buttons show it: plain, and in the theme
/// languages the theme name behind it.
fn load_name(load: Load, lang: Language) -> String {
    match LOADS.iter().find(|(listed, ..)| *listed == load) {
        Some((_, plain, themed)) => lang.titled(*plain, Some(*themed)),
        None => match load {
            Load::Workers(count) => fill_template(
                lang.text(Msg::SettingsLoadWorkers),
                &[("{count}", &count.to_string())],
            ),
            _ => NOTHING_YET.to_owned(),
        },
    }
}

/// The state with the pause, the load with its buttons, the workers.
fn pipeline_rows<'a>(
    status: &'a FilingStatus,
    load: Option<Load>,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let snapshot = status.snapshot();
    let mut state = vec![page_kit::value(state_word(status, lang), palette)];
    if snapshot.state != RunnerState::Off {
        let label = if snapshot.state == RunnerState::Paused {
            Msg::FilingStatusResume
        } else {
            Msg::FilingStatusPause
        };
        state.push(page_kit::link(
            lang.text(label),
            Some(send(FilingStatusMessage::TogglePause)),
            palette,
        ));
    }
    let names: Vec<String> = LOADS
        .iter()
        .map(|(listed, ..)| load_name(*listed, lang))
        .collect();
    let options: Vec<(&str, Message)> = LOADS
        .iter()
        .zip(&names)
        .map(|((listed, ..), name)| {
            (
                name.as_str(),
                Message::HermesOverview(HermesOverviewMessage::SetLoad(*listed)),
            )
        })
        .collect();
    let selected = LOADS
        .iter()
        .position(|(listed, ..)| load == Some(*listed))
        .unwrap_or(LOADS.len());
    let mut rows = vec![
        page_kit::form_row(
            lang.text(Msg::FilingStatusStateLabel),
            page_kit::cell(state),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::FilingStatusLoadLabel),
            page_kit::segmented(&options, selected, palette),
            Some(page_kit::hint(
                lang.text(Msg::FilingStatusLoadNote),
                palette,
            )),
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::FilingStatusWorkersLabel),
            page_kit::value(snapshot.worker_target.to_string(), palette),
            None,
            palette,
        ),
    ];
    if let Some(ai) = ai_value(status.ai(), lang) {
        rows.push(page_kit::form_row(
            lang.text(Msg::FilingStatusAiLabel),
            page_kit::value(ai, palette),
            None,
            palette,
        ));
    }
    rows
}

/// The throughput and the time left, as figures.
fn throughput_rows<'a>(
    status: &'a FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
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
    vec![
        page_kit::form_row(
            lang.text(Msg::FilingStatusThroughputLabel),
            page_kit::value(throughput, palette),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::FilingStatusRemainingLabel),
            page_kit::value(remaining, palette),
            None,
            palette,
        ),
    ]
}

/// What each step of the pipeline has waiting, done, failed and unchanged.
fn steps_table<'a>(
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
    column![header, Column::with_children(lines)].into()
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
    columns.row_last_right(
        &[
            &number,
            &format::fit_middle(
                &file_label(&busy.file),
                columns.text_width(1),
                font_size::BODY,
            ),
            &task_label(busy.task, lang),
            &elapsed_label(elapsed),
        ],
        palette.text,
    )
}

fn workers_table<'a>(
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
    Column::with_children(rows).into()
}

fn yes_no(answer: bool, lang: Language) -> &'static str {
    lang.text(if answer {
        Msg::FilingStatusYes
    } else {
        Msg::FilingStatusNo
    })
}

fn index_rows<'a>(
    status: &'a FilingStatus,
    index_open: bool,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let note = |content: String| vec![page_kit::hint(content, palette)];
    match status.index_figures() {
        _ if !index_open => note(lang.text(Msg::FilingStatusIndexNotOpen).to_owned()),
        None => note(NOT_READ_YET.to_owned()),
        Some(Err(reason)) => note(fill_template(
            lang.text(Msg::FilingStatusIndexUnavailable),
            &[("{reason}", reason)],
        )),
        Some(Ok(figures)) => index_figure_rows(figures, lang, palette),
    }
}

fn index_figure_rows<'a>(
    figures: &IndexFigures,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let statistics = &figures.statistics;
    let row_of = |label: Msg, shown: String| {
        page_kit::form_row(
            lang.text(label),
            page_kit::value(shown, palette),
            None,
            palette,
        )
    };
    vec![
        row_of(
            Msg::FilingStatusIndexDocuments,
            format::grouped(statistics.document_count),
        ),
        row_of(
            Msg::FilingStatusIndexReviews,
            format::grouped(statistics.open_reviews),
        ),
        row_of(
            Msg::FilingStatusIndexEncrypted,
            yes_no(figures.encrypted, lang).to_owned(),
        ),
        row_of(
            Msg::FilingStatusIndexSize,
            figures
                .file_bytes
                .map_or_else(|| NOTHING_YET.to_owned(), format::size),
        ),
    ]
}

/// How long ago an error happened, as `m:ss`.
fn age_label(status: &FilingStatus, at: SystemTime) -> String {
    elapsed_label(status.taken_at().duration_since(at).unwrap_or_default())
}

fn error_rows<'a>(
    status: &'a FilingStatus,
    badges: Badges,
    budget: &LineBudget,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let row_chars = budget.table_chars.saturating_sub(ERROR_ROW_FIXED_CHARS);
    let selected = status.selected_error();
    let rows: Vec<Element<'a, Message>> = status
        .errors_newest_first()
        .enumerate()
        .map(|(position, error)| {
            let checked = if status.error_is_checked(&error.file) {
                Check::On
            } else {
                Check::Off
            };
            let name = format::ellipsize_end(&file_label(&error.file), row_chars.div_euclid(2));
            let reason_chars = row_chars
                .saturating_sub((name.chars().count() * MONO_WIDTH_TENTHS).div_euclid(10))
                .saturating_sub(2);
            let content = row![
                page_kit::checkbox(
                    checked,
                    Some(send(FilingStatusMessage::ToggleErrorChecked(
                        error.file.clone()
                    ))),
                    None,
                    palette,
                ),
                page_kit::mono(name, page_kit::TEXT_BODY, palette.text),
                container(page_kit::value_dim(
                    format::ellipsize_end(&error.message, reason_chars),
                    palette,
                ))
                .width(Length::Fill)
                .clip(true),
            ]
            .spacing(page_kit::GAP_S)
            .align_y(iced::alignment::Vertical::Center);
            page_kit::list_row_one(
                content.into(),
                Some(age_label(status, error.at)),
                false,
                selected == Some(position) && badges.focus == HermesFocus::Content,
                Some(send(FilingStatusMessage::SelectError(position))),
                palette,
            )
        })
        .collect();
    if rows.is_empty() {
        let none = container(page_kit::hint(
            lang.text(Msg::FilingStatusErrorsNone),
            palette,
        ))
        .padding([0.0, page_kit::GAP_L]);
        return vec![none.into()];
    }
    rows
}

/// The selected error with its whole reason, and the way to its file.
fn selected_error_line<'a>(
    status: &'a FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let position = status.selected_error()?;
    let error = status.errors_newest_first().nth(position)?;
    Some(
        column![
            page_kit::error_line(
                format!("{}: {}", file_label(&error.file), error.message),
                palette,
            ),
            page_kit::link(
                lang.text(Msg::FilingStatusShowFile),
                Some(send(FilingStatusMessage::ErrorActivated(
                    error.file.clone()
                ))),
                palette,
            ),
        ]
        .spacing(page_kit::GAP_XS)
        .into(),
    )
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
        assert_eq!(step_label("review", Language::English), "Review");
        assert_eq!(step_label("review", Language::German), "Rückfragen");
        assert_eq!(step_label("some_step", Language::English), "some_step");
        assert_eq!(step_label("some_step", Language::German), "some_step");
    }

    #[test]
    fn the_standard_languages_name_the_loads_plainly_and_the_futurama_variants_add_the_theme_name()
    {
        let shown = |load, lang| load_name(load, lang);
        for (load, plain_en, plain_de, theme_en, theme_de) in [
            (
                Load::Gentle,
                "Gentle",
                "Schonend",
                "By the book",
                "Dienst nach Vorschrift",
            ),
            (
                Load::Balanced,
                "Balanced",
                "Ausgewogen",
                "Business as usual",
                "Normalbetrieb",
            ),
            (Load::Full, "Full", "Volle Last", "Grade 36", "Grad 36"),
        ] {
            assert_eq!(shown(load, Language::English), plain_en);
            assert_eq!(shown(load, Language::German), plain_de);
            assert_eq!(
                shown(load, Language::EnglishFuturama),
                format!("{plain_en} ({theme_en})")
            );
            assert_eq!(
                shown(load, Language::GermanFuturama),
                format!("{plain_de} ({theme_de})")
            );
        }
    }
}
