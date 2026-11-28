//! The Platz tab: opened by its key, the commands of the other tabs that open it,
//! and the keys that must not be taken for a tap of Option.

use crate::app::space::{restore_items, trash_items};
use crate::app::*;
use iced::keyboard::key::Named;
use iced::{event, keyboard, Event};

fn modifiers(held: Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::ModifiersChanged(held))
}

fn key_pressed(app: &App, key: Key, held: Modifiers) -> Option<Message> {
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: held,
        text: None,
        repeat: false,
    });
    app.input.lock().handle(&event, event::Status::Ignored)
}

fn command_alt() -> Modifiers {
    Modifiers::COMMAND | Modifiers::ALT
}

#[tokio::test]
async fn the_key_of_the_tab_opens_it_and_escape_closes_it() {
    let mut app = App::with_fixed_panels();
    let pressed = route_key(
        &app.prompt_key_state(),
        Key::Character("p".into()),
        command_alt(),
    );
    assert_eq!(pressed, Some(Message::OpenSpace));
    drop(app.update(Message::OpenSpace));
    assert!(app.space.is_some());
    assert_eq!(app.shown_tab, ShownTab::Space);

    app.publish_key_state();
    let escape = key_pressed(&app, Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(
        escape,
        Some(Message::Space(ncrs_space_view::Message::Escape))
    );
    // The scan is still running in this test, so Escape stops it first and only then asks to close.
    drop(app.update(escape.unwrap()));
    assert!(app.space.is_some());
    drop(app.run_space_action(ncrs_space_view::Action::Closed));
    assert!(app.space.is_none());
    assert_eq!(app.shown_tab, ShownTab::Panels);
}

#[tokio::test]
async fn the_key_of_the_tab_works_while_another_tab_is_shown() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenRepos));
    let pressed = route_key(
        &app.prompt_key_state(),
        Key::Character("p".into()),
        command_alt(),
    );
    assert_eq!(pressed, Some(Message::OpenSpace));
    drop(app.update(Message::OpenSpace));
    assert_eq!(app.shown_tab, ShownTab::Space);
    drop(app.update(Message::OpenSpace));
    assert_eq!(app.shown_tab, ShownTab::Space);
}

#[tokio::test]
async fn the_system_page_opens_the_tab() {
    let mut app = App::with_fixed_panels();
    drop(app.run_monitor_action(ncrs_monitor::Action::OpenSpace));
    assert!(app.space.is_some());
    assert_eq!(app.shown_tab, ShownTab::Space);
}

#[tokio::test]
async fn the_chat_opens_the_tab_from_its_card() {
    let mut app = App::with_fixed_panels();
    drop(app.run_hermes_chat_action(ncrs_hermes_chat::Action::OpenSpace));
    assert_eq!(app.shown_tab, ShownTab::Space);
}

#[tokio::test]
async fn a_run_the_chat_asked_for_keeps_the_tab_open_on_its_confirm_kind() {
    let mut app = App::with_fixed_panels();
    drop(
        app.run_hermes_chat_action(ncrs_hermes_chat::Action::SpaceRun(
            ncrs_hermes_chat::SpaceRunKind::DeleteSafe,
        )),
    );
    assert_eq!(app.shown_tab, ShownTab::Space);
    assert_eq!(app.space_run_ask, Some(ncrs_space_view::ConfirmKind::Safe));
}

#[tokio::test]
async fn holding_option_for_the_key_of_the_tab_is_no_tap() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSpace));
    app.publish_key_state();
    // Option first, then Command, then the letter, then everything released.
    assert_eq!(
        app.input
            .lock()
            .handle(&modifiers(Modifiers::ALT), event::Status::Ignored),
        None
    );
    assert_eq!(
        app.input
            .lock()
            .handle(&modifiers(command_alt()), event::Status::Ignored),
        None
    );
    drop(key_pressed(&app, Key::Character("p".into()), command_alt()));
    for held in [command_alt(), Modifiers::ALT, Modifiers::empty()] {
        assert_eq!(
            app.input
                .lock()
                .handle(&modifiers(held), event::Status::Ignored),
            None,
            "released down to {held:?}"
        );
    }
}

#[tokio::test]
async fn a_tap_of_option_alone_opens_the_menu_of_the_row() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSpace));
    app.publish_key_state();
    assert_eq!(
        app.input
            .lock()
            .handle(&modifiers(Modifiers::ALT), event::Status::Ignored),
        None
    );
    let tapped = app
        .input
        .lock()
        .handle(&modifiers(Modifiers::empty()), event::Status::Ignored);
    assert_eq!(
        tapped,
        Some(Message::Space(ncrs_space_view::Message::OpenMenuKey))
    );
}

/// A trash that is a folder, remembering where each entry came from.
struct FolderTrash {
    folder: PathBuf,
    entries: std::sync::Mutex<Vec<fs::TrashedItem>>,
}

impl fs::Trash for FolderTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().ok_or("no file name")?;
        std::fs::rename(path, self.folder.join(name)).map_err(|err| err.to_string())?;
        self.entries.lock().unwrap().push(fs::TrashedItem {
            original: path.to_path_buf(),
            deleted_at: 0,
            key: name.to_owned(),
        });
        Ok(())
    }

    fn reach(&self) -> fs::TrashReach {
        fs::TrashReach::Everything
    }

    fn list(&self) -> Result<Vec<fs::TrashedItem>, String> {
        Ok(self.entries.lock().unwrap().clone())
    }

    fn restore(&self, item: &fs::TrashedItem) -> Result<(), fs::RestoreError> {
        std::fs::rename(self.folder.join(&item.key), &item.original)
            .map_err(|err| fs::RestoreError::Failed(err.to_string()))
    }
}

#[tokio::test]
async fn own_files_go_to_the_trash_and_come_back_without_hermes() {
    let work = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = ["a.txt", "b.txt"]
        .iter()
        .map(|name| work.path().join(name))
        .collect();
    for path in &paths {
        std::fs::write(path, "x").unwrap();
    }
    let items: Vec<_> = paths
        .iter()
        .map(|path| ncrs_filing::execute::OwnTrashItem {
            path: path.clone(),
            size: 1,
            modified: None,
        })
        .collect();
    let trash = FolderTrash {
        folder: bin.path().to_path_buf(),
        entries: std::sync::Mutex::default(),
    };
    let (gone, failed) = trash_items(&trash, &items, false);
    assert_eq!(gone, paths);
    assert!(failed.is_none());
    assert!(paths.iter().all(|path| !path.exists()));

    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSpace));
    drop(app.space_trashed(gone, None));
    assert_eq!(app.last_own_trash, paths);

    assert_eq!(restore_items(&trash, &app.last_own_trash), Ok(2));
    assert!(paths.iter().all(|path| path.exists()));
}

#[test]
fn a_file_changed_since_the_measure_is_not_trashed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("model.bin");
    std::fs::write(&path, b"a").expect("write");
    let measured = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let item = |modified| ncrs_filing::execute::OwnTrashItem {
        path: path.clone(),
        size: 1,
        modified,
    };
    assert!(!crate::app::space::changed_since_measure(&item(measured)));
    assert!(!crate::app::space::changed_since_measure(&item(None)));
    let earlier = measured.map(|time| time - std::time::Duration::from_secs(60));
    assert!(crate::app::space::changed_since_measure(&item(earlier)));
}
