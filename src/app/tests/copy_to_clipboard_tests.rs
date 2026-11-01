use crate::app::*;
use crate::fs::FileEntry;
use iced::keyboard::key::{Code, Physical};
use std::ffi::OsString;

fn entry(name: &str, is_parent: bool) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: PathBuf::from("/left").join(name),
        is_dir: false,
        is_symlink: false,
        is_parent,
        size: 0,
        modified: None,
        folder_size: None,
    }
}

fn panel() -> PanelState {
    let mut panel = PanelState::new(PathBuf::from("/left/dir"));
    panel.entries = vec![
        entry("..", true),
        entry("zulu.txt", false),
        entry("alpha.txt", false),
    ];
    panel
}

fn text(panel: &PanelState, kind: ClipboardKind) -> Option<String> {
    ncrs_ops::clipboard_text(rows_of(panel), kind).map(|copied| copied.text)
}

/// Joined as the code joins, so Windows gets its own separator.
fn in_dir(name: &str) -> String {
    PathBuf::from("/left/dir").join(name).display().to_string()
}

#[test]
fn the_cursor_row_is_copied_as_path_or_name() {
    let mut panel = panel();
    panel.selected = 1;
    assert_eq!(text(&panel, ClipboardKind::Path), Some(in_dir("zulu.txt")));
    assert_eq!(
        text(&panel, ClipboardKind::Name).as_deref(),
        Some("zulu.txt")
    );
}

#[test]
fn tagged_rows_win_one_per_line_in_listing_order_without_trailing_newline() {
    let mut panel = panel();
    panel.selected = 0;
    panel.selection.toggle(Path::new("/left/alpha.txt"));
    panel.selection.toggle(Path::new("/left/zulu.txt"));
    assert_eq!(
        ncrs_ops::clipboard_text(rows_of(&panel), ClipboardKind::Path),
        Some(ncrs_ops::ClipboardText {
            text: format!("{}\n{}", in_dir("zulu.txt"), in_dir("alpha.txt")),
            count: 2,
        })
    );
    assert_eq!(
        text(&panel, ClipboardKind::Name).as_deref(),
        Some("zulu.txt\nalpha.txt")
    );
}

#[test]
fn the_parent_row_copies_the_panel_directory_itself() {
    let panel = panel();
    assert_eq!(
        text(&panel, ClipboardKind::Path).as_deref(),
        Some("/left/dir")
    );
    assert_eq!(text(&panel, ClipboardKind::Name).as_deref(), Some("dir"));
}

#[test]
fn at_the_root_the_path_is_the_name() {
    let mut panel = panel();
    panel.path = PathBuf::from("/");
    assert_eq!(text(&panel, ClipboardKind::Name).as_deref(), Some("/"));
}

#[test]
fn an_empty_panel_has_nothing_to_copy() {
    let panel = PanelState::new(PathBuf::from("/left"));
    assert_eq!(
        ncrs_ops::clipboard_text(rows_of(&panel), ClipboardKind::Path),
        None
    );
}

fn path_chord_routes(key: &str, physical: Code, modifiers: Modifiers) -> Option<Message> {
    route_press(
        &PromptKeyState::default(),
        Key::Character(key.into()),
        Physical::Code(physical),
        modifiers,
        false,
    )
}

#[test]
fn the_path_chord_routes_on_every_platform() {
    assert_eq!(
        path_chord_routes("c", Code::KeyC, Modifiers::ALT | Modifiers::COMMAND),
        Some(Message::CopyToClipboard(ClipboardKind::Path))
    );
}

/// Option+C types "ç" on a Mac; the binding has to follow the key.
#[cfg(target_os = "macos")]
#[test]
fn option_cmd_c_works_although_the_mac_types_a_cedilla() {
    assert_eq!(
        path_chord_routes("ç", Code::KeyC, Modifiers::ALT | Modifiers::LOGO),
        Some(Message::CopyToClipboard(ClipboardKind::Path))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ctrl_cmd_c_works_although_the_mac_may_type_a_control_character() {
    for typed in ["c", "\u{3}"] {
        assert_eq!(
            path_chord_routes(typed, Code::KeyC, Modifiers::CTRL | Modifiers::LOGO),
            Some(Message::CopyToClipboard(ClipboardKind::Name)),
            "{typed:?}"
        );
    }
}

#[test]
fn ctrl_c_copies_the_files_off_a_mac_and_stops_the_job_on_one() {
    let (command_c, ctrl_c) = if cfg!(target_os = "macos") {
        (Modifiers::LOGO, Message::AbortJob)
    } else {
        (Modifiers::CTRL, Message::CopyFilesToClipboard)
    };
    assert_eq!(
        path_chord_routes("c", Code::KeyC, command_c),
        Some(Message::CopyFilesToClipboard)
    );
    assert_eq!(
        path_chord_routes("c", Code::KeyC, Modifiers::CTRL),
        Some(ctrl_c)
    );
    if !cfg!(target_os = "macos") {
        assert_eq!(
            path_chord_routes("C", Code::KeyC, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Message::CopyToClipboard(ClipboardKind::Name))
        );
    }
}

#[test]
fn a_held_key_does_not_copy_again() {
    let routed = route_press(
        &PromptKeyState::default(),
        Key::Character("c".into()),
        Physical::Code(Code::KeyC),
        Modifiers::ALT | Modifiers::COMMAND,
        true,
    );
    assert_eq!(routed, None);
}

#[tokio::test]
async fn copying_says_so_in_the_status_bar_and_an_overlay_blocks_it() {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.left.entries = vec![entry("a.txt", false)];
    app.panels.left.selected = 0;

    let _open = app.update(Message::Delete { permanent: false });
    assert!(app.overlay_is_open(), "the delete dialog did not open");
    let _blocked = app.update(Message::CopyToClipboard(ClipboardKind::Path));
    assert_eq!(app.notice, None);
    let _closed = app.update(Message::DeleteCancel);

    let _path = app.update(Message::CopyToClipboard(ClipboardKind::Path));
    assert_eq!(app.notice.as_deref(), Some("Path copied"));
    let _name = app.update(Message::CopyToClipboard(ClipboardKind::Name));
    assert_eq!(app.notice.as_deref(), Some("Name copied"));
}
