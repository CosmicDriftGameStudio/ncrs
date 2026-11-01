use crate::app::*;
use crate::clipboard_paste::RgbaImage;
use crate::fs::FileEntry;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use std::cell::RefCell;
use std::ffi::OsString;
use std::rc::Rc;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

struct Pasting {
    app: App,
    _root: tempfile::TempDir,
    target: PathBuf,
}

fn pasting() -> Pasting {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = root.path().join("left");
    app.panels.right.path = target.clone();
    app.panels.active = PanelSide::Right;
    Pasting {
        app,
        _root: root,
        target,
    }
}

async fn deliver(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                queue.push(reported);
            }
        }
    }
}

fn pasted_text() -> Option<PastedContent> {
    Some(PastedContent::Text("zwei\nZeilen \u{fc}\n".to_owned()))
}

fn pasted_picture() -> Option<PastedContent> {
    clipboard_paste::choose(
        None,
        Some(RgbaImage {
            width: 1,
            height: 1,
            rgba: vec![0, 128, 255, 255],
        }),
        None,
    )
}

fn only_file_in(folder: &Path) -> PathBuf {
    let mut found: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

#[tokio::test]
async fn pasted_text_asks_for_a_name_and_ends_as_a_txt_file_with_exactly_that_text() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());

    deliver(&mut case.app, Message::Paste).await;

    let name = case
        .app
        .ops
        .prompt
        .as_ref()
        .expect("no prompt")
        .name()
        .to_owned();
    assert!(name.starts_with("Pasted "), "{name}");
    assert!(name.ends_with(".txt"), "{name}");
    assert_eq!(
        case.app.ops.prompt.as_ref().unwrap().kind(),
        PromptKind::Paste
    );

    deliver(&mut case.app, Message::PromptSubmit).await;

    assert!(case.app.ops.prompt.is_none());
    assert!(case.app.ops.pending_paste.is_none());
    let created = only_file_in(&case.target);
    assert_eq!(created.file_name().unwrap().to_string_lossy(), name);
    assert_eq!(
        std::fs::read(created).unwrap(),
        "zwei\nZeilen \u{fc}\n".as_bytes()
    );
}

#[tokio::test]
async fn a_typed_name_is_the_name_of_the_pasted_file() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());
    deliver(&mut case.app, Message::Paste).await;

    drop(case.app.update(Message::PromptInput("notes.txt".into())));
    deliver(&mut case.app, Message::PromptSubmit).await;

    assert_eq!(
        std::fs::read_to_string(case.target.join("notes.txt")).unwrap(),
        "zwei\nZeilen \u{fc}\n"
    );
}

#[tokio::test]
async fn a_pasted_picture_ends_as_a_png_file() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_picture());

    deliver(&mut case.app, Message::Paste).await;
    let name = case
        .app
        .ops
        .prompt
        .as_ref()
        .expect("no prompt")
        .name()
        .to_owned();
    assert!(name.ends_with(".png"), "{name}");
    deliver(&mut case.app, Message::PromptSubmit).await;

    let bytes = std::fs::read(only_file_in(&case.target)).unwrap();
    assert!(bytes.starts_with(&PNG_SIGNATURE));
}

#[tokio::test]
async fn a_name_with_a_separator_is_refused_and_nothing_is_written() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());
    deliver(&mut case.app, Message::Paste).await;

    drop(
        case.app
            .update(Message::PromptInput("../escape.txt".into())),
    );
    deliver(&mut case.app, Message::PromptSubmit).await;

    assert!(case.app.ops.prompt.as_ref().unwrap().error().is_some());
    assert!(case.app.ops.paste_staging.is_none());
    assert_eq!(std::fs::read_dir(&case.target).unwrap().count(), 0);
}

#[tokio::test]
async fn escape_drops_the_pasted_content() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());
    deliver(&mut case.app, Message::Paste).await;
    assert!(case.app.ops.pending_paste.is_some());

    drop(case.app.update(Message::PromptCancel));

    assert!(case.app.ops.prompt.is_none());
    assert!(case.app.ops.pending_paste.is_none());
    assert_eq!(std::fs::read_dir(&case.target).unwrap().count(), 0);
}

#[tokio::test]
async fn a_taken_name_in_the_panel_asks_what_to_do() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());
    deliver(&mut case.app, Message::Paste).await;
    drop(case.app.update(Message::PromptInput("note.txt".into())));
    std::fs::write(case.target.join("note.txt"), "old").unwrap();

    deliver(&mut case.app, Message::PromptSubmit).await;

    assert!(
        case.app.ops.pending_conflict.is_some(),
        "no conflict dialog"
    );
}

#[tokio::test]
async fn pasted_files_start_a_copy_into_the_active_panel() {
    let mut case = pasting();
    let source = case.target.parent().unwrap().join("outside.txt");
    std::fs::write(&source, "x").unwrap();

    drop(
        case.app
            .update(Message::ClipboardRead(Some(PastedContent::Files(vec![
                source.clone(),
            ])))),
    );

    let transfer = case.app.ops.transfer.as_ref().expect("no transfer started");
    assert_eq!(transfer.kind, TransferKind::Copy);
    assert_eq!(transfer.sources, [source]);
    assert_eq!(transfer.target, case.target.canonicalize().unwrap());
}

#[tokio::test]
async fn an_empty_clipboard_says_so() {
    let mut case = pasting();
    drop(case.app.update(Message::ClipboardRead(None)));

    assert_eq!(
        case.app.notice.as_deref(),
        Some("The clipboard holds no files, image or text.")
    );
    assert!(case.app.ops.prompt.is_none());
    assert!(case.app.ops.transfer.is_none());
}

#[tokio::test]
async fn a_paste_during_a_running_transfer_changes_nothing() {
    let mut case = pasting();
    case.app.read_clipboard = || Task::done(pasted_text());
    let running = Transfer {
        kind: TransferKind::Move,
        sources: vec![PathBuf::from("/busy")],
        target: PathBuf::from("/elsewhere"),
        names: Vec::new(),
    };
    case.app.ops.transfer = Some(running.clone());

    drop(case.app.update(Message::Paste));

    assert_eq!(case.app.ops.transfer.as_ref(), Some(&running));
    assert!(case.app.ops.prompt.is_none());
    assert!(case.app.notice.is_some());
}

fn entry_at(path: PathBuf) -> FileEntry {
    FileEntry {
        name: OsString::from(path.file_name().unwrap()),
        path,
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }
}

fn recording_writer(app: &mut App) -> Rc<RefCell<Vec<Vec<PathBuf>>>> {
    let written = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&written);
    app.write_clipboard_files = Box::new(move |files| {
        record.borrow_mut().push(files.to_vec());
        Ok(())
    });
    written
}

#[test]
fn copy_files_writes_the_cursor_file_and_says_so() {
    let mut case = pasting();
    let file = case.target.join("a.txt");
    case.app.panels.right.entries = vec![entry_at(file.clone())];
    let written = recording_writer(&mut case.app);

    drop(case.app.update(Message::CopyFilesToClipboard));

    assert_eq!(*written.borrow(), [vec![file]]);
    assert_eq!(
        case.app.notice.as_deref(),
        Some("File copied to the clipboard")
    );
}

#[test]
fn copy_files_writes_all_tagged_files() {
    let mut case = pasting();
    let (first, second) = (case.target.join("a.txt"), case.target.join("b.txt"));
    case.app.panels.right.entries = vec![entry_at(first.clone()), entry_at(second.clone())];
    case.app
        .panels
        .right
        .selection
        .tag_all(&case.app.panels.right.entries);
    let written = recording_writer(&mut case.app);

    drop(case.app.update(Message::CopyFilesToClipboard));

    assert_eq!(*written.borrow(), [vec![first, second]]);
    assert_eq!(
        case.app.notice.as_deref(),
        Some("2 files copied to the clipboard")
    );
}

#[test]
fn copy_files_of_an_archive_entry_says_so_and_writes_nothing() {
    let mut case = pasting();
    let archive = case.target.join("fotos.zip");
    std::fs::write(&archive, "").unwrap();
    case.app.panels.right.entries = vec![entry_at(archive.join("inner.txt"))];
    let written = recording_writer(&mut case.app);

    drop(case.app.update(Message::CopyFilesToClipboard));

    assert!(written.borrow().is_empty());
    assert!(case
        .app
        .notice
        .as_deref()
        .is_some_and(|text| text.starts_with("Only files on this computer")));
}

#[test]
fn copy_files_of_an_archive_file_itself_is_a_plain_file() {
    let mut case = pasting();
    let archive = case.target.join("fotos.zip");
    std::fs::write(&archive, "").unwrap();
    case.app.panels.right.entries = vec![entry_at(archive.clone())];
    let written = recording_writer(&mut case.app);

    drop(case.app.update(Message::CopyFilesToClipboard));

    assert_eq!(*written.borrow(), [vec![archive]]);
}

#[test]
fn a_refused_clipboard_shows_the_reason() {
    let mut case = pasting();
    case.app.panels.right.entries = vec![entry_at(case.target.join("a.txt"))];
    case.app.write_clipboard_files = Box::new(|_| Err("no display".to_owned()));

    drop(case.app.update(Message::CopyFilesToClipboard));

    assert!(case.app.notice.is_none());
    assert!(case
        .app
        .status_error()
        .is_some_and(|text| text.contains("no display")));
}
