//! The Rules, Tags, Reports and Read errors pages as pages of the Settings:
//! what their dialogs and rows do, and the AI line on the Index page.

#![allow(clippy::panic_in_result_fn)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use iced::widget::Id;
use iced_test::Simulator;
use ncrs_ai::Compute;
use ncrs_filing::filing::AiStatus;
use ncrs_pipeline::{ErrorEntry, Snapshot, Task};

use super::settings_pages_tests::{settle, TestResult};
use crate::app::*;
use crate::config::from_text;
use crate::i18n::Language;
use crate::messages::SettingsFeature;
use crate::rule_form::RuleSave;
use crate::rules_page::RulesPage;
use crate::rules_work::RulesLoad;
use crate::saved_queries::fixtures::THIS_YEAR;

const MIN_VALUE_WIDTH: f32 = 240.0;
const WINDOW: iced::Size = iced::Size::new(1200.0, 760.0);

fn simulator_of(app: &App, size: iced::Size) -> Simulator<'_, Message, iced::Theme> {
    let settings = iced::Settings {
        default_font: ncrs_ui_kit::APP_FONT,
        ..iced::Settings::default()
    };
    Simulator::with_size(settings, size, app.view())
}

fn shows(app: &App, label: &str) -> bool {
    simulator_of(app, WINDOW).find(label).is_ok()
}

fn app_with_new_rule_form() -> App {
    let mut page = RulesPage::new();
    page.loaded(RulesLoad::default());
    page.begin_new();
    App::with_rules(page)
}

#[test]
fn the_value_of_a_condition_has_room_even_in_a_small_window() {
    let app = app_with_new_rule_form();
    for size in [
        WINDOW,
        iced::Size::new(800.0, 600.0),
        iced::Size::new(720.0, 600.0),
    ] {
        let width = simulator_of(&app, size)
            .find(Id::new("condition-value-0"))
            .expect("the value field of the first condition")
            .bounds()
            .width;
        assert!(
            width >= MIN_VALUE_WIDTH,
            "{width} wide in a window of {size:?}"
        );
    }
}

#[test]
fn typing_a_value_makes_the_rule_saveable_and_is_saved() {
    let mut app = app_with_new_rule_form();
    assert!(!app.rules_for_test().unwrap().form().unwrap().is_valid());

    let messages: Vec<Message> = {
        let mut simulator = simulator_of(&app, WINDOW);
        simulator
            .click(Id::new("condition-value-0"))
            .expect("the value field");
        let _ = simulator.typewrite("firma-a");
        simulator.into_messages().collect()
    };
    for message in messages {
        drop(app.update(message));
    }

    let form = app.hermes.rules.as_mut().unwrap().form_mut().unwrap();
    assert!(form.is_valid());
    let request = form.save_request();
    assert!(
        matches!(&request, Some(RuleSave::Create { spec, .. }) if format!("{spec:?}").contains("firma-a")),
        "{request:?}"
    );
}

fn read_errors_app(count: usize) -> App {
    let taken_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let mut snapshot = Snapshot::off();
    snapshot.errors = (0..count)
        .map(|sequence| ErrorEntry {
            file: PathBuf::from(format!("/tmp/inbox/scan-{sequence}.pdf")),
            task: Task::Step("probe"),
            message: "no text could be read".to_owned(),
            at: taken_at,
            sequence: sequence as u64,
        })
        .collect();
    App::with_filing_status_page(snapshot, taken_at)
}

#[test]
fn process_all_again_comes_with_two_errors_and_reads_every_shown_file_again() {
    let app = read_errors_app(2);
    let mut simulator = simulator_of(&app, WINDOW);
    simulator
        .click("Process all again")
        .expect("the button for all");
    assert!(simulator.into_messages().any(|message| matches!(
        message,
        Message::FilingStatus(FilingStatusMessage::ProcessAllAgain)
    )));

    let mut reading = read_errors_app(2);
    let (_, actions) = reading.with_hermes(Vec::new(), |hermes, ctx| {
        ncrs_hermes_ui::update_filing_status(hermes, FilingStatusMessage::ProcessAllAgain, ctx)
    });
    let mut files: Vec<PathBuf> = actions
        .into_iter()
        .flat_map(|action| match action {
            ncrs_hermes_ui::Action::ReadPathsAgain(paths) => paths,
            _ => Vec::new(),
        })
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            PathBuf::from("/tmp/inbox/scan-0.pdf"),
            PathBuf::from("/tmp/inbox/scan-1.pdf")
        ]
    );
}

#[test]
fn a_single_error_has_no_button_for_all() {
    let app = read_errors_app(1);
    assert!(shows(&app, "Process again"));
    assert!(!shows(&app, "Process all again"));
}

#[test]
fn a_new_report_starts_from_a_template_or_from_nothing() {
    let mut list = crate::saved_queries::SavedQueries::new(THIS_YEAR);
    list.loaded(Vec::new());
    let mut app = App::with_saved_queries(list);
    drop(app.update(Message::SavedQueries(SavedQueriesMessage::New)));

    let state = app.saved_queries_for_test().unwrap();
    assert!(state.unsaved().is_some());
    assert_eq!(state.template_choice(), 0);
    assert_eq!(state.name(), "");
    assert!(shows(&app, "New report"));
    assert!(shows(&app, "Template"));

    drop(app.update(Message::SavedQueries(SavedQueriesMessage::Template(1))));
    let chosen = app.saved_queries_for_test().unwrap();
    assert_eq!(chosen.template_choice(), 1);
    assert_ne!(chosen.name(), "", "the template names the report");

    drop(app.update(Message::SavedQueries(SavedQueriesMessage::Template(0))));
    assert_eq!(app.saved_queries_for_test().unwrap().name(), "");
}

#[test]
fn the_question_to_remove_a_report_can_be_cancelled() {
    let mut list = crate::saved_queries::fixtures::loaded_list();
    list.select(0);
    list.ask_delete();
    let app = App::with_saved_queries(list);
    let mut simulator = simulator_of(&app, WINDOW);
    simulator.click("Cancel").expect("the cancel button");
    assert!(simulator.into_messages().any(|message| matches!(
        message,
        Message::SavedQueries(SavedQueriesMessage::CancelStep)
    )));
}

#[test]
fn the_settings_search_finds_the_four_pages() {
    for page in ["Rules", "Tags", "Reports", "Read errors"] {
        let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());
        drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
            page.to_owned(),
        ))));
        assert!(shows(&app, page), "{page} is listed");
        assert!(
            !shows(&app, "Keyboard shortcuts"),
            "{page} narrows the list"
        );
    }
}

fn matches_ai_snapshot(status: &AiStatus, name: &str) -> TestResult {
    let app = App::with_settings_file(SettingsFeature::Index, Ok(from_text("")?))
        .in_language(Language::German);
    let state = app.settings.page.as_ref().ok_or("no settings page")?;
    let mut view = app.settings_view(state);
    view.facts.ai = ncrs_settings::facts::AiState::Ready;
    view.facts.ai_detail = crate::app::settings_facts::ai_detail(status, Language::German);
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, 1100.0),
        crate::ui::settings::view(&view),
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)?
        .matches_image(format!("tests/snapshots/{name}.png"))?;
    assert!(
        matches,
        "the page does not match tests/snapshots/{name}.png"
    );
    Ok(())
}

#[test]
fn the_index_page_keeps_the_ai_line_on_the_graphics_card() -> TestResult {
    matches_ai_snapshot(
        &AiStatus::Loaded {
            name: "Qwen3 4B".to_owned(),
            memory_bytes: 2_700_000_000,
            compute: Compute::Gpu("Apple M2 Pro".to_owned()),
        },
        "settings_index_ai_gpu_de",
    )
}

#[test]
fn the_index_page_keeps_the_ai_line_on_the_processor() -> TestResult {
    matches_ai_snapshot(
        &AiStatus::Loaded {
            name: "Qwen3 4B".to_owned(),
            memory_bytes: 2_700_000_000,
            compute: Compute::Cpu,
        },
        "settings_index_ai_cpu_de",
    )
}

fn saved_queries_message(message: SavedQueriesMessage) -> Message {
    Message::SavedQueries(message)
}

/// The reports page over a real, empty index, so a run really answers.
fn app_with_reports_and_index(dir: &tempfile::TempDir) -> App {
    let index = ncrs_index::Index::create_plain(&dir.path().join("index.db")).unwrap();
    let mut app = App::with_saved_queries(crate::saved_queries::fixtures::loaded_list());
    app.hermes.filing = Some(FilingModule::in_phase_over_index(Phase::Ready, index));
    app
}

/// The hits are asked for but their answer is not delivered, as when the
/// dialog is cancelled before the index answers.
fn without_hits(app: &mut App) {
    let view = app.hermes.saved_queries.as_mut().unwrap();
    view.begin_run().unwrap();
}

#[tokio::test]
async fn changing_a_report_that_has_no_hits_yet_runs_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_reports_and_index(&dir);
    assert!(app.saved_queries_for_test().unwrap().results().is_none());

    settle(
        &mut app,
        saved_queries_message(SavedQueriesMessage::Edit(0)),
    )
    .await;

    let view = app.saved_queries_for_test().unwrap();
    assert!(view.edit_is_open());
    assert!(view.results().is_some(), "{:?}", view.problem());
}

#[tokio::test]
async fn the_export_opens_after_a_cancelled_delete_question_or_change_dialog() {
    for first in [
        SavedQueriesMessage::AskDelete(0),
        SavedQueriesMessage::Edit(0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_reports_and_index(&dir);
        drop(app.update(saved_queries_message(first)));
        without_hits(&mut app);
        drop(app.update(saved_queries_message(SavedQueriesMessage::CancelStep)));
        assert!(app.saved_queries_for_test().unwrap().results().is_none());

        settle(
            &mut app,
            saved_queries_message(SavedQueriesMessage::StartExport(0)),
        )
        .await;

        assert!(
            app.saved_queries_for_test()
                .unwrap()
                .export_step()
                .is_some(),
            "the export dialog is open"
        );
    }
}

#[tokio::test]
async fn an_export_asked_for_does_not_open_after_another_report_is_started() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_reports_and_index(&dir);
    drop(app.update(saved_queries_message(SavedQueriesMessage::StartExport(0))));

    settle(&mut app, saved_queries_message(SavedQueriesMessage::New)).await;

    let view = app.saved_queries_for_test().unwrap();
    assert!(view.results().is_some(), "the new report has run");
    assert!(view.export_step().is_none());
}

#[tokio::test]
async fn the_export_without_a_file_to_copy_opens_with_the_button_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_reports_and_index(&dir);

    settle(
        &mut app,
        saved_queries_message(SavedQueriesMessage::StartExport(0)),
    )
    .await;

    let view = app.saved_queries_for_test().unwrap();
    assert!(view.export_step().is_some());
    assert_eq!(view.copies(), 0);
    let mut simulator = simulator_of(&app, WINDOW);
    simulator.click("Export").expect("the confirm button");
    assert!(!simulator
        .into_messages()
        .any(|message| matches!(message, Message::SavedQueries(SavedQueriesMessage::Export))));
}

#[test]
fn the_palette_does_not_open_behind_a_dialog_of_a_page() {
    let mut app = app_with_new_rule_form();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    assert!(app.command_palette.is_none());
    assert!(app.rules_for_test().unwrap().form().is_some());

    let mut page = RulesPage::new();
    page.loaded(RulesLoad::default());
    let mut free = App::with_rules(page);
    drop(free.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    assert!(free.command_palette.is_some());
}

#[test]
fn the_problem_of_the_page_shows_in_the_rule_form() {
    let mut page = RulesPage::new();
    page.loaded(RulesLoad::default());
    page.begin_new();
    page.failed("disk full".to_owned());
    let app = App::with_rules(page);

    assert!(shows(
        &app,
        "The rules could not be read or changed: disk full"
    ));
}
