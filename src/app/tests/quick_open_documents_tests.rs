use crate::app::*;
use crate::filing::{FoundDocument, TextSource};
use crate::quick_open::DocumentsState;

const FILES: &[&str] = &["src/config.rs", "docs/config.md"];

fn hermes_app() -> App {
    let mut app = App::with_fixed_panels().with_ready_hermes_for_test();
    app.open_quick_open_for_test(FILES, "");
    app
}

fn name_hits(app: &App) -> Vec<String> {
    let quick_open = app.quick_open_for_test().unwrap();
    quick_open
        .hits()
        .iter()
        .map(|hit| quick_open.relative_path(hit).to_owned())
        .collect()
}

fn scan() -> FoundDocument {
    FoundDocument {
        path: PathBuf::from("/inbox/scan.pdf"),
        read_by: TextSource::Pdf,
    }
}

#[test]
fn name_hits_stand_while_the_full_text_search_runs_and_when_it_fails() {
    let mut app = hermes_app();
    app.type_into_quick_open_for_test("config");
    let names = name_hits(&app);
    assert_eq!(names.len(), 2);
    assert_eq!(
        app.quick_open_for_test().unwrap().documents_state(),
        DocumentsState::Searching
    );

    app.deliver_documents_for_test(Err("index is busy".to_owned()));

    assert_eq!(
        app.quick_open_for_test().unwrap().documents_state(),
        DocumentsState::Failed
    );
    assert!(app.quick_open_for_test().unwrap().documents().is_empty());
    assert_eq!(name_hits(&app), names);
}

#[test]
fn found_documents_come_after_the_name_hits_and_enter_reveals_one() {
    let mut app = hermes_app();
    app.type_into_quick_open_for_test("config");
    let names = name_hits(&app);

    app.deliver_documents_for_test(Ok(vec![scan()]));
    assert_eq!(name_hits(&app), names);
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Move(2))));
    assert_eq!(
        app.quick_open_for_test().unwrap().selected_path(),
        Some(PathBuf::from("/inbox/scan.pdf"))
    );

    drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
    assert!(app.quick_open_for_test().is_none());
}

#[test]
fn without_hermes_there_is_no_documents_section() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(FILES, "");
    app.type_into_quick_open_for_test("config");

    let quick_open = app.quick_open_for_test().unwrap();
    assert_eq!(quick_open.documents_state(), DocumentsState::Idle);
    assert_eq!(quick_open.hits().len(), 2);
    app.deliver_documents_for_test(Ok(vec![scan()]));
    assert!(app.quick_open_for_test().unwrap().documents().is_empty());
}

#[test]
fn a_greater_than_sign_first_switches_to_the_command_list_with_settings() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(FILES, "");

    drop(
        app.update(Message::QuickOpen(QuickOpenMessage::QueryChanged(
            ">language".to_owned(),
        ))),
    );

    assert!(app.quick_open_for_test().is_none());
    let palette = app.command_palette_for_test().unwrap();
    assert_eq!(palette.query(), "language");
    assert!(palette.hits().any(|hit| matches!(
        palette.entry(hit).map(|entry| &entry.target),
        Some(ncrs_palette::command_list::Target::Setting { .. })
    )));
}
