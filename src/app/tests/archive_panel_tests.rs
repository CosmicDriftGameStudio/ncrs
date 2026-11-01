use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn archive_file() -> PathBuf {
    Path::new("/x").join("fotos.zip")
}

fn entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: archive_file().join(name),
        is_dir,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }
}

fn panel_in_archive(entries: Vec<FileEntry>) -> PanelState {
    let mut panel = PanelState::new(archive_file().join("2024"));
    panel.archive = Some(archive_file());
    panel.entries = entries;
    panel
}

#[test]
fn a_panel_outside_an_archive_refuses_nothing() {
    let mut panel = panel_in_archive(vec![entry("inner.zip", false)]);
    panel.archive = None;
    for message in [
        Message::Transfer(TransferKind::Move),
        Message::Panel(PanelMessage::OpenSelected),
        Message::CreateDirPrompt,
    ] {
        assert!(archive_refusal(&panel, &message).is_none());
    }
}

#[test]
fn a_panel_inside_an_archive_refuses_what_would_change_it() {
    let panel = panel_in_archive(vec![entry("a.txt", false)]);
    for message in [
        Message::Transfer(TransferKind::Move),
        Message::CreateDirPrompt,
        Message::CreateFilePrompt,
        Message::OpenSearch,
    ] {
        assert!(
            matches!(
                archive_refusal(&panel, &message),
                Some(Msg::StatusArchiveReadOnly)
            ),
            "{message:?}"
        );
    }
}

#[test]
fn every_action_is_refused_in_an_archive_exactly_when_it_is_not_allowed() {
    use crate::context_menu::{ContextAction, HermesAction};
    use crate::keymap::KeyAction;

    // The panel holds nothing that is an archive, so `Open` is not the
    // nested case, which depends on the selection.
    let panel = panel_in_archive(vec![entry("a.txt", false)]);
    let refused = |message: &Message| archive_refusal(&panel, message).is_some();
    for action in KeyAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_in_archive(),
            "{action:?}"
        );
    }
    for action in ContextAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_in_archive(),
            "{action:?}"
        );
    }
    for action in HermesAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_in_archive(),
            "{action:?}"
        );
    }
}

#[test]
fn adding_the_panel_folder_to_the_filing_setup_is_refused_in_an_archive() {
    use crate::messages::FilingSetupMessage;
    let panel = panel_in_archive(vec![entry("a.txt", false)]);
    let message = Message::FilingSetup(FilingSetupMessage::AddPanelFolder(
        ncrs_intake::LocationMode::Inbox,
    ));
    assert!(matches!(
        archive_refusal(&panel, &message),
        Some(Msg::StatusArchiveReadOnly)
    ));
}

#[test]
fn copy_delete_and_browsing_stay_allowed_inside_an_archive() {
    let panel = panel_in_archive(vec![entry("a.txt", false), entry("sub", true)]);
    for message in [
        Message::Transfer(TransferKind::Copy),
        Message::Delete { permanent: false },
        Message::Delete { permanent: true },
        Message::Panel(PanelMessage::GoUp),
        Message::Panel(PanelMessage::OpenSelected),
    ] {
        assert!(archive_refusal(&panel, &message).is_none(), "{message:?}");
    }
}

#[test]
fn packing_from_or_into_an_archive_is_refused() {
    let panel = panel_in_archive(vec![entry("a.txt", false)]);
    assert!(matches!(
        archive_refusal(&panel, &Message::PackPrompt),
        Some(Msg::StatusPackUnavailable)
    ));
}

#[test]
fn packing_beside_an_archive_suggests_the_own_folder() {
    let mut app = App::with_fixed_panels();
    app.panels.left.entries = vec![entry("fotos", true)];
    app.panels.right = panel_in_archive(Vec::new());
    drop(app.update(Message::PackPrompt));
    assert_eq!(
        app.ops.prompt.as_ref().map(Prompt::archive_path),
        Some(PathBuf::from("/home/test/links/fotos.zip"))
    );
}

#[test]
fn a_shell_command_in_an_archive_runs_in_the_folder_holding_it() {
    let folder = tempfile::tempdir().unwrap();
    let archive = folder.path().join("fotos.zip");
    let mut panel = panel_in_archive(Vec::new());
    panel.path = archive.join("2024");
    panel.archive = Some(archive);
    assert_eq!(shell_directory(&panel), folder.path());

    panel.archive = None;
    panel.path = folder.path().to_path_buf();
    assert_eq!(shell_directory(&panel), folder.path());
}

#[test]
fn an_archive_inside_an_archive_is_not_opened() {
    let panel = panel_in_archive(vec![entry("inner.tar.gz", false)]);
    assert!(matches!(
        archive_refusal(&panel, &Message::Panel(PanelMessage::OpenSelected)),
        Some(Msg::StatusArchiveNested)
    ));
}

#[test]
fn a_panel_in_an_archive_is_recorded_as_standing_on_the_archive_file() {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left = panel_in_archive(vec![entry("a.txt", false)]);

    let record = app.session_state().left;

    assert_eq!(
        record.path,
        session_state::record_path(archive_file().parent().unwrap())
    );
    assert_eq!(record.cursor.as_deref(), Some("fotos.zip"));
}
