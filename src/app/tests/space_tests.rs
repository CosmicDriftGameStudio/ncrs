//! The Platz tab: opened by its key, the commands of the other tabs that open it,
//! and the keys that must not be taken for a tap of Option.

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
        let mut entries = self
            .entries
            .lock()
            .map_err(|poisoned| poisoned.to_string())?;
        entries.push(fs::TrashedItem {
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
        let entries = self
            .entries
            .lock()
            .map_err(|poisoned| poisoned.to_string())?;
        Ok(entries.clone())
    }

    fn restore(&self, item: &fs::TrashedItem) -> Result<(), fs::RestoreError> {
        std::fs::rename(self.folder.join(&item.key), &item.original)
            .map_err(|err| fs::RestoreError::Failed(err.to_string()))
    }
}

fn own_item(path: &Path) -> ncrs_filing::execute::OwnTrashItem {
    let meta = std::fs::metadata(path).unwrap();
    ncrs_filing::execute::OwnTrashItem {
        path: path.to_path_buf(),
        size: meta.len(),
        modified: meta.modified().ok(),
    }
}

fn empty_scan() -> Arc<ncrs_space::Scan> {
    Arc::new(ncrs_space::Scan {
        volume: ncrs_os_probe::VolumeSpace {
            total: 100,
            available: 50,
        },
        volume_name: String::new(),
        worktree_roots: Vec::new(),
        entries: Vec::new(),
        partial: false,
        processes_known: true,
        measured_at: std::time::SystemTime::UNIX_EPOCH,
        took: std::time::Duration::ZERO,
    })
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
    let items: Vec<_> = paths.iter().map(|path| own_item(path)).collect();
    let trash = FolderTrash {
        folder: bin.path().to_path_buf(),
        entries: std::sync::Mutex::default(),
    };
    let run =
        ncrs_filing::execute::trash_own_items(None, "Cleared", work.path(), &items, &trash, false);
    assert_eq!(run.gone, paths);
    assert!(run.refused.is_empty());
    assert!(paths.iter().all(|path| !path.exists()));

    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSpace));
    drop(app.space_trashed(run.gone, None, None));
    assert_eq!(app.last_own_trash, paths);

    assert_eq!(
        ncrs_filing::execute::restore_own_items(&trash, &app.last_own_trash),
        Ok(2)
    );
    assert!(paths.iter().all(|path| path.exists()));
}

#[tokio::test]
async fn a_folder_that_changed_inside_is_not_trashed_by_the_pane() {
    let work = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let inner = work.path().join("app/main.rs");
    std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
    std::fs::write(&inner, "fn main() {}").unwrap();
    let item = own_item(inner.parent().unwrap());
    std::fs::write(&inner, "fn main() { changed() }").unwrap();
    let trash = FolderTrash {
        folder: bin.path().to_path_buf(),
        entries: std::sync::Mutex::default(),
    };
    let run =
        ncrs_filing::execute::trash_own_items(None, "Cleared", work.path(), &[item], &trash, true);
    assert!(run.gone.is_empty());
    assert!(inner.exists());
}

#[tokio::test]
async fn a_cancelled_chat_run_is_not_asked_again_and_a_manual_run_is_not_answered() {
    let mut app = App::with_fixed_panels();
    drop(
        app.run_hermes_chat_action(ncrs_hermes_chat::Action::SpaceRun(
            ncrs_hermes_chat::SpaceRunKind::DeleteSafe,
        )),
    );
    drop(app.run_space_action(ncrs_space_view::Action::Measured(empty_scan())));
    assert_eq!(app.space_run_ask, None);
    assert!(
        !app.space_chat_confirm,
        "no confirm opened: nothing to delete"
    );

    drop(app.run_space_action(ncrs_space_view::Action::Measured(empty_scan())));
    assert_eq!(app.space_run_ask, None);
    assert!(!app.space_chat_confirm);

    drop(
        app.run_space_action(ncrs_space_view::Action::RunDone(Arc::new(
            ncrs_space::RunLog {
                started: std::time::SystemTime::UNIX_EPOCH,
                finished: std::time::SystemTime::UNIX_EPOCH,
                rows: Vec::new(),
            },
        ))),
    );
    assert!(!app.space_chat_run);
}
