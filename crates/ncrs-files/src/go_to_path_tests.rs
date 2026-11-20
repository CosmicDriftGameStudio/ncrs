//! The "Go to path" dialog: where Enter in it leads, and what it says when
//! the path leads nowhere.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_i18n::Language;

use crate::action::Action;
use crate::panel::PanelState;
use crate::path_field::{PathChecked, TypedPathCtx};
use crate::state::Panels;

fn panels_in(folder: &Path) -> Panels {
    Panels::new(
        PanelState::new(folder.to_path_buf()),
        PanelState::new(PathBuf::from("/")),
        PanelSide::Left,
    )
}

/// What Enter in the dialog does with `target`, once the disk has answered.
async fn enter(panels: &mut Panels, target: PathBuf) -> Vec<Action> {
    panels.path_check_id = 1;
    let result = ncrs_fs::locate(target.clone()).await;
    panels.finish_go_to_path_check(
        Language::English,
        PathChecked {
            request_id: 1,
            side: PanelSide::Left,
            target,
            result,
        },
    )
}

#[tokio::test]
async fn a_folder_opens_in_the_active_panel_and_closes_the_dialog() {
    let folder = tempfile::tempdir().expect("temp folder");
    fs::create_dir_all(folder.path().join("target")).expect("folder");
    let mut panels = panels_in(Path::new("/"));
    let actions = enter(&mut panels, folder.path().join("target")).await;
    assert_eq!(
        actions,
        vec![
            Action::ClearMessages,
            Action::Load {
                side: PanelSide::Left,
                path: folder.path().join("target"),
                select: None,
            },
            Action::GoToPathAnswer(Ok(())),
        ]
    );
}

#[tokio::test]
async fn a_file_opens_its_folder_with_the_cursor_on_the_file() {
    let folder = tempfile::tempdir().expect("temp folder");
    fs::write(folder.path().join("Rechnung.pdf"), "x").expect("file");
    let mut panels = panels_in(Path::new("/"));
    let actions = enter(&mut panels, folder.path().join("Rechnung.pdf")).await;
    assert_eq!(
        actions,
        vec![
            Action::ClearMessages,
            Action::Load {
                side: PanelSide::Left,
                path: folder.path().to_path_buf(),
                select: Some(OsString::from("Rechnung.pdf")),
            },
            Action::GoToPathAnswer(Ok(())),
        ]
    );
}

#[tokio::test]
async fn a_missing_path_is_an_error_for_the_dialog_and_no_load() {
    let folder = tempfile::tempdir().expect("temp folder");
    let missing = folder.path().join("nowhere");
    let mut panels = panels_in(Path::new("/"));
    let actions = enter(&mut panels, missing.clone()).await;
    let [Action::GoToPathAnswer(Err(text))] = actions.as_slice() else {
        panic!("expected one error answer, got {actions:?}");
    };
    assert!(text.contains(&missing.display().to_string()), "{text}");
}

#[tokio::test]
async fn an_answer_to_an_older_look_is_dropped() {
    let mut panels = panels_in(Path::new("/"));
    panels.path_check_id = 2;
    let actions = panels.finish_go_to_path_check(
        Language::English,
        PathChecked {
            request_id: 1,
            side: PanelSide::Left,
            target: PathBuf::from("/"),
            result: ncrs_fs::locate(PathBuf::from("/")).await,
        },
    );
    assert!(actions.is_empty());
}

#[tokio::test]
async fn a_tilde_is_the_home_folder_and_a_typed_path_starts_a_look_at_the_disk() {
    let home = tempfile::tempdir().expect("temp folder");
    fs::create_dir_all(home.path().join("Dokumente")).expect("folder");
    let mut panels = panels_in(Path::new("/"));
    let before = panels.path_check_id;
    let typed_path = TypedPathCtx {
        lang: Language::English,
        home: home.path(),
        session_home: &|_| None,
    };
    let (_look, immediate) = panels.go_to_path_from_dialog("  ~/Dokumente  ", &typed_path);
    assert!(immediate.is_empty(), "an ordinary path waits for the disk");
    assert_eq!(panels.path_check_id, before.wrapping_add(1));
    assert_eq!(
        ncrs_fs::resolve_typed_path(Path::new("/"), home.path(), "~/Dokumente"),
        home.path().join("Dokumente")
    );
}

#[test]
fn nothing_typed_starts_nothing() {
    let mut panels = panels_in(Path::new("/"));
    let typed_path = TypedPathCtx {
        lang: Language::English,
        home: Path::new("/"),
        session_home: &|_| None,
    };
    let (_look, immediate) = panels.go_to_path_from_dialog("   ", &typed_path);
    assert!(immediate.is_empty());
    assert_eq!(panels.path_check_id, 0);
}
