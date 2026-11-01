use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

struct Dropping {
    app: App,
    _root: tempfile::TempDir,
    source_file: PathBuf,
    source_folder: PathBuf,
    target: PathBuf,
}

fn dropping() -> Dropping {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    let source_folder = outside.join("album");
    let source_file = outside.join("note.txt");
    let target = root.path().join("target");
    std::fs::create_dir_all(&source_folder).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(&source_file, "note").unwrap();
    std::fs::write(source_folder.join("a.txt"), "a").unwrap();
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = root.path().join("left");
    app.panels.right.path = target.clone();
    app.panels.active = PanelSide::Right;
    Dropping {
        app,
        _root: root,
        source_file,
        source_folder,
        target,
    }
}

fn drop_both(case: &mut Dropping) -> u64 {
    for path in [case.source_file.clone(), case.source_folder.clone()] {
        drop(case.app.update(Message::FileDropped(path)));
    }
    case.app.ops.drop_generation
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

#[tokio::test]
async fn a_drop_copies_all_files_into_the_active_panel() {
    let mut case = dropping();
    let generation = drop_both(&mut case);

    drop(case.app.update(Message::DropSettled(generation)));

    let transfer = case.app.ops.transfer.as_ref().expect("no transfer started");
    assert_eq!(transfer.kind, TransferKind::Copy);
    assert_eq!(
        transfer.sources,
        [case.source_file.clone(), case.source_folder.clone()]
    );
    assert_eq!(transfer.target, case.target.canonicalize().unwrap());
}

#[tokio::test]
async fn a_drop_goes_to_the_left_panel_when_that_is_active() {
    let mut case = dropping();
    let left = case.app.panels.left.path.clone();
    std::fs::create_dir_all(&left).unwrap();
    case.app.panels.active = PanelSide::Left;
    let generation = drop_both(&mut case);

    drop(case.app.update(Message::DropSettled(generation)));

    assert_eq!(
        case.app.ops.transfer.as_ref().unwrap().target,
        left.canonicalize().unwrap()
    );
}

#[tokio::test]
async fn a_stale_settle_timer_starts_nothing() {
    let mut case = dropping();
    let generation = drop_both(&mut case);

    drop(case.app.update(Message::DropSettled(generation - 1)));

    assert!(case.app.ops.transfer.is_none());
}

#[tokio::test]
async fn a_taken_name_in_the_target_asks_what_to_do() {
    let mut case = dropping();
    std::fs::write(case.target.join("note.txt"), "old").unwrap();
    let generation = drop_both(&mut case);

    deliver(&mut case.app, Message::DropSettled(generation)).await;

    assert!(
        case.app.ops.pending_conflict.is_some(),
        "no conflict dialog"
    );
}

#[tokio::test]
async fn a_drop_during_a_running_transfer_says_so_and_changes_nothing() {
    let mut case = dropping();
    let running = Transfer {
        kind: TransferKind::Move,
        sources: vec![PathBuf::from("/busy")],
        target: PathBuf::from("/elsewhere"),
        names: Vec::new(),
    };
    case.app.ops.transfer = Some(running.clone());
    let generation = drop_both(&mut case);

    drop(case.app.update(Message::DropSettled(generation)));

    assert_eq!(case.app.ops.transfer.as_ref(), Some(&running));
    assert!(case.app.notice.is_some());
}

#[tokio::test]
async fn a_drop_into_the_folder_it_comes_from_is_refused_in_words() {
    let mut case = dropping();
    case.app.panels.right.path = case.source_file.parent().unwrap().to_path_buf();
    let generation = drop_both(&mut case);

    drop(case.app.update(Message::DropSettled(generation)));

    assert!(case.app.ops.transfer.is_none());
    assert!(case.app.job_error.is_some());
}

#[test]
fn hovering_marks_the_panel_until_it_is_left_or_dropped() {
    let mut app = App::with_environment(Environment::isolated()).0;
    drop(app.update(Message::FilesHovered));
    assert!(app.ops.drop_hover);
    drop(app.update(Message::FilesHoverLeft));
    assert!(!app.ops.drop_hover);
    drop(app.update(Message::FilesHovered));
    drop(app.update(Message::FileDropped(PathBuf::from("/x"))));
    assert!(!app.ops.drop_hover);
}
