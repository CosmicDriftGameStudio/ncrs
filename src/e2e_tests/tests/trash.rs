use crate::e2e_tests::*;
use crate::messages::TrashMessage;
use iced::keyboard::key::Named;

/// The trash folder and a session that deletes into it.
pub(super) fn trash_session(
    dir: &Path,
) -> (
    tempfile::TempDir,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let bin = tempfile::Builder::new()
        .prefix("ncrs-e2e-bin-")
        .tempdir()
        .expect("a trash folder");
    let trash = std::sync::Arc::new(FolderTrash(bin.path().to_path_buf()));
    (bin, session_with_trash(dir, trash))
}

pub(super) const SHIFT_F8: (Named, Modifiers) = (Named::F8, Modifiers::SHIFT);

/// F8 and Enter: the file leaves its place for the trash, and the tagged
/// rows are the ones that go, then the tags are gone with them.
#[test]
fn f8_enter_moves_the_tagged_files_to_the_trash() {
    let dir = scratch("trash");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    write(&dir.path().join("gamma.txt"), "c");
    let (bin, mut session) = trash_session(dir.path());
    for name in ["alpha.txt", "gamma.txt"] {
        cursor_to(&mut session, name);
        session.press(Key::Named(Named::Insert));
    }

    session.press(Key::Named(Named::F8));
    assert!(session.app.delete_dialog_is_open(), "F8 asked nothing");
    assert!(
        dir.path().join("alpha.txt").exists(),
        "deleted before asking"
    );
    session.press(Key::Named(Named::Enter));

    assert!(
        !dir.path().join("alpha.txt").exists(),
        "alpha.txt still there"
    );
    assert!(
        !dir.path().join("gamma.txt").exists(),
        "gamma.txt still there"
    );
    assert!(
        dir.path().join("beta.txt").exists(),
        "an untagged file went"
    );
    assert!(
        bin.path().join("alpha.txt").is_file(),
        "alpha.txt not in the trash"
    );
    assert!(
        bin.path().join("gamma.txt").is_file(),
        "gamma.txt not in the trash"
    );
    assert!(!session.app.delete_dialog_is_open());
    assert!(
        !session.app.panel(PanelSide::Left).selection.any_tagged(),
        "the tags outlived the files"
    );
    assert!(
        !names(&session.app, PanelSide::Left).contains(&"alpha.txt".to_string()),
        "the panel still lists alpha.txt"
    );
}

/// A trash that is a folder, with the record of what the app put in it, as
/// the macOS trash works: the list is the record.
struct Bin {
    folder: tempfile::TempDir,
    journal: tempfile::TempDir,
    trash: std::sync::Arc<crate::fs::JournaledTrash<crate::fs::FolderBackend>>,
}

impl Bin {
    fn new() -> Self {
        let folder = tempfile::Builder::new()
            .prefix("ncrs-e2e-bin-")
            .tempdir()
            .expect("a trash folder");
        let journal = tempfile::Builder::new()
            .prefix("ncrs-e2e-journal-")
            .tempdir()
            .expect("a journal folder");
        let trash = std::sync::Arc::new(crate::fs::JournaledTrash::new(
            journal.path().join("journal.toml"),
            crate::fs::FolderBackend(folder.path().to_path_buf()),
        ));
        Self {
            folder,
            journal,
            trash,
        }
    }

    fn session(
        &self,
        dir: &Path,
    ) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
        let trash = std::sync::Arc::clone(&self.trash);
        session_with_trash(dir, trash)
    }

    fn journal_text(&self) -> String {
        std::fs::read_to_string(self.journal.path().join("journal.toml")).unwrap_or_default()
    }

    /// What lies in the trash folder, as `name -> content`.
    fn contents(&self) -> Vec<String> {
        std::fs::read_dir(self.folder.path())
            .expect("the trash folder")
            .filter_map(|entry| std::fs::read_to_string(entry.expect("an entry").path()).ok())
            .collect()
    }
}

fn open_trash<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    session.send(Message::Trash(TrashMessage::Open));
    assert!(
        session.app.trash_view_has_keys(),
        "the trash view did not open"
    );
}

/// F8 and Enter on the named rows.
fn trash_rows<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    rows: &[&str],
) {
    session.press(Key::Named(Named::Home));
    for name in rows {
        cursor_to(session, name);
        session.press(Key::Named(Named::Insert));
    }
    session.press(Key::Named(Named::F8));
    session.press(Key::Named(Named::Enter));
}

#[test]
fn the_trash_view_puts_a_file_and_a_folder_back_with_their_content() {
    let dir = scratch("trash-restore");
    write(&dir.path().join("alpha.txt"), "alpha");
    write(&dir.path().join("docs/inner.txt"), "inner");
    let bin = Bin::new();
    let mut session = bin.session(dir.path());
    trash_rows(&mut session, &["docs", "alpha.txt"]);
    assert!(!dir.path().join("alpha.txt").exists());
    assert!(!dir.path().join("docs").exists());

    open_trash(&mut session);
    let root = std::fs::canonicalize(dir.path()).expect("the scratch path");
    let mut listed = session.app.trash_view_originals().expect("an open view");
    listed.sort();
    assert_eq!(listed, [root.join("alpha.txt"), root.join("docs")]);
    session.press(Key::Character("*".into()));
    session.press(Key::Named(Named::Enter));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("alpha.txt")).expect("alpha.txt is back"),
        "alpha"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("docs/inner.txt")).expect("docs is back"),
        "inner"
    );
    assert!(session.app.restore_is_idle());
    assert_eq!(session.app.trash_view_originals(), Some(Vec::new()));
    assert!(
        !bin.journal_text().contains("alpha.txt") && !bin.journal_text().contains("docs"),
        "the journal still lists what came back: {}",
        bin.journal_text()
    );
    assert!(names(&session.app, PanelSide::Left).contains(&"alpha.txt".to_string()));
}

#[test]
fn a_taken_name_asks_and_overwriting_sends_the_occupant_to_the_trash() {
    let dir = scratch("trash-taken");
    write(&dir.path().join("alpha.txt"), "original");
    let bin = Bin::new();
    let mut session = bin.session(dir.path());
    trash_rows(&mut session, &["alpha.txt"]);
    write(&dir.path().join("alpha.txt"), "occupant");
    open_trash(&mut session);

    session.press(Key::Named(Named::Enter));
    assert!(session.app.conflict_is_pending(), "the restore did not ask");
    session.press(Key::Named(Named::Escape));

    assert!(!session.app.conflict_is_pending());
    assert!(session.app.restore_is_idle());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("alpha.txt")).expect("the occupant"),
        "occupant",
        "keeping touched the file in the way"
    );
    assert_eq!(
        session.app.trash_view_originals().map(|list| list.len()),
        Some(1),
        "the entry that stayed is not listed"
    );

    session.press(Key::Named(Named::Enter));
    assert!(session.app.conflict_is_pending());
    session.press(Key::Named(Named::Tab));
    session.press(Key::Named(Named::Enter));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("alpha.txt")).expect("the original"),
        "original"
    );
    assert_eq!(
        bin.contents(),
        ["occupant".to_string()],
        "the occupant was not moved to the trash"
    );
    assert!(session.app.restore_is_idle());
}

#[test]
fn a_missing_original_folder_is_created_on_restore() {
    let dir = scratch("trash-folder");
    write(&dir.path().join("gone/deep/inner.txt"), "inner");
    let bin = Bin::new();
    crate::fs::Trash::trash(&*bin.trash, &dir.path().join("gone/deep/inner.txt"))
        .expect("trashing");
    std::fs::remove_dir_all(dir.path().join("gone")).expect("removing the folders");
    let mut session = bin.session(dir.path());
    open_trash(&mut session);

    session.press(Key::Named(Named::Enter));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("gone/deep/inner.txt")).expect("restored"),
        "inner"
    );
}

#[test]
fn the_restore_key_puts_back_exactly_the_latest_deletion() {
    let dir = scratch("trash-last");
    write(&dir.path().join("older.txt"), "older");
    write(&dir.path().join("alpha.txt"), "alpha");
    write(&dir.path().join("beta.txt"), "beta");
    let bin = Bin::new();
    let mut session = bin.session(dir.path());
    trash_rows(&mut session, &["older.txt"]);
    trash_rows(&mut session, &["alpha.txt", "beta.txt"]);
    session.press_with(Key::Character("z".into()), Modifiers::COMMAND);

    assert!(dir.path().join("alpha.txt").is_file());
    assert!(dir.path().join("beta.txt").is_file());
    assert!(
        !dir.path().join("older.txt").exists(),
        "an earlier deletion came back too"
    );

    // Nothing is left to put back: the second press changes nothing.
    session.press_with(Key::Character("z".into()), Modifiers::COMMAND);
    assert!(!dir.path().join("older.txt").exists());
}

/// The restore key against the system trash: it must find the entries
/// under the spelling the system recorded, which is not always the path
/// the panel deleted.
#[test]
fn the_restore_key_puts_back_the_latest_deletion_from_the_real_trash() {
    crate::fs::real_trash_tests::on_the_real_trash(
        "e2e_tests::tests::the_restore_key_puts_back_the_latest_deletion_from_the_real_trash",
        |scene| {
            std::fs::create_dir_all(scene.work().join("home")).expect("the right panel");
            let older = scene.write("older.txt", "older");
            let alpha = scene.write("alpha.txt", "alpha");
            let docs = scene.write("docs/inner.txt", "inner");
            let mut session = session_with_trash(scene.work(), scene.trash());
            trash_rows(&mut session, &["older.txt"]);
            trash_rows(&mut session, &["docs", "alpha.txt"]);
            scene.assert_gone(&alpha);
            scene.assert_gone(&docs);

            session.press_with(Key::Character("z".into()), Modifiers::COMMAND);

            assert_eq!(
                std::fs::read_to_string(&alpha).expect("alpha is back"),
                "alpha"
            );
            assert_eq!(
                std::fs::read_to_string(&docs).expect("docs is back"),
                "inner"
            );
            scene.assert_gone(&older);
            assert!(scene.items_of_ours("alpha.txt").is_empty());
            assert!(scene.items_of_ours("docs").is_empty());
            assert_eq!(
                scene.items_of_ours("older.txt").len(),
                1,
                "the earlier deletion left the trash"
            );
            scene.finish();
        },
    );
}

/// Restores `alpha.txt` with the key while another file holds its name, and
/// answers the question with `answer`.
fn restore_onto_a_taken_name(
    test_name: &str,
    answer: &[Named],
    check: impl FnOnce(&crate::fs::real_trash_tests::Scene),
) {
    crate::fs::real_trash_tests::on_the_real_trash(test_name, |scene| {
        std::fs::create_dir_all(scene.work().join("home")).expect("the right panel");
        let alpha = scene.write("alpha.txt", "original");
        let mut session = session_with_trash(scene.work(), scene.trash());
        trash_rows(&mut session, &["alpha.txt"]);
        scene.assert_gone(&alpha);
        scene.write("alpha.txt", "occupant");

        session.press_with(Key::Character("z".into()), Modifiers::COMMAND);
        assert!(session.app.conflict_is_pending(), "the restore did not ask");
        for key in answer {
            session.press(Key::Named(*key));
        }

        assert!(!session.app.conflict_is_pending());
        assert!(session.app.restore_is_idle());
        check(scene);
        scene.finish();
    });
}

#[test]
fn keeping_the_file_in_the_way_leaves_the_deletion_in_the_real_trash() {
    restore_onto_a_taken_name(
        "e2e_tests::tests::keeping_the_file_in_the_way_leaves_the_deletion_in_the_real_trash",
        &[Named::Escape],
        |scene| {
            assert_eq!(
                std::fs::read_to_string(scene.work().join("alpha.txt")).expect("the occupant"),
                "occupant",
                "keeping touched the file in the way"
            );
            assert_eq!(
                scene.items_of_ours("alpha.txt").len(),
                1,
                "the deletion left the trash"
            );
        },
    );
}

#[test]
fn overwriting_sends_the_file_in_the_way_to_the_real_trash() {
    restore_onto_a_taken_name(
        "e2e_tests::tests::overwriting_sends_the_file_in_the_way_to_the_real_trash",
        &[Named::Tab, Named::Enter],
        |scene| {
            assert_eq!(
                std::fs::read_to_string(scene.work().join("alpha.txt")).expect("the original"),
                "original"
            );
            assert_eq!(
                scene.items_of_ours("alpha.txt").len(),
                1,
                "the occupant is not the one entry in the trash"
            );
        },
    );
}

#[test]
fn an_entry_taken_out_of_the_trash_behind_the_apps_back_is_not_listed() {
    let dir = scratch("trash-behind");
    write(&dir.path().join("alpha.txt"), "alpha");
    write(&dir.path().join("beta.txt"), "beta");
    let bin = Bin::new();
    let mut session = bin.session(dir.path());
    trash_rows(&mut session, &["alpha.txt", "beta.txt"]);
    std::fs::remove_file(bin.folder.path().join("alpha.txt")).expect("emptying the trash");

    open_trash(&mut session);

    let root = std::fs::canonicalize(dir.path()).expect("the scratch path");
    assert_eq!(
        session.app.trash_view_originals(),
        Some(vec![root.join("beta.txt")])
    );
}
