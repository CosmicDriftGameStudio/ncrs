//! The Git column: what a scan marks, when the column exists, which answers
//! are dropped, and how a panel inside a repository looks.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::Size;
use iced_test::Simulator;
use ncrs_core::{EntryDetails, FileEntry, PanelSide};
use ncrs_i18n::Language;
use ncrs_ui_kit::git_mark::GitMark;
use ncrs_ui_kit::palette::ThemePreset;
use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

use crate::ctx::{ColumnSettings, ViewCtx};
use crate::git_status::{scan, GitScan, GitSummary, PanelGit, ScanRow};
use crate::panel::PanelState;
use crate::state::Panels;
use crate::view::view;

fn entry(folder: &str, name: &str, is_dir: bool, size: u64) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: PathBuf::from(folder).join(name),
        is_dir,
        is_symlink: false,
        is_parent: name == "..",
        size,
        modified: None,
        folder_size: None,
        details: EntryDetails::default(),
    }
}

fn panels_in(left: PathBuf) -> Panels {
    Panels::new(
        PanelState::new(left),
        PanelState::new(PathBuf::from("/")),
        PanelSide::Left,
    )
}

fn summary(changed: usize) -> GitSummary {
    GitSummary {
        branch: Some("main".to_owned()),
        head_short: None,
        ahead: 2,
        behind: 0,
        changed,
    }
}

#[test]
fn a_scan_marks_a_subfolder_that_is_a_repository_and_nothing_else() {
    let folder = tempfile::tempdir().expect("temp folder");
    fs::create_dir_all(folder.path().join("project/.git")).expect("repository");
    fs::create_dir_all(folder.path().join("plain")).expect("folder");
    let rows = ["project", "plain"].map(|name| ScanRow {
        name: OsString::from(name),
        path: folder.path().join(name),
        is_dir: true,
    });
    let found = scan(None, &rows, &Arc::new(AtomicBool::new(false))).expect("scan");
    assert_eq!(
        found.marks,
        HashMap::from([(OsString::from("project"), GitMark::Repository)])
    );
    assert_eq!(found.summary, None);
}

#[test]
fn a_stopped_scan_gives_no_answer() {
    let rows = [ScanRow {
        name: OsString::from("a"),
        path: PathBuf::from("/a"),
        is_dir: false,
    }];
    assert_eq!(scan(None, &rows, &Arc::new(AtomicBool::new(true))), None);
}

#[test]
fn the_column_exists_only_inside_a_repository() {
    let outside = tempfile::tempdir().expect("temp folder");
    let mut panels = panels_in(outside.path().to_path_buf());
    let _ = panels.start_git_scan(PanelSide::Left);
    assert!(!panels.left.git.in_repository());

    let inside = tempfile::tempdir().expect("temp folder");
    fs::create_dir_all(inside.path().join(".git")).expect("repository");
    let mut inside_panels = panels_in(inside.path().to_path_buf());
    let _ = inside_panels.start_git_scan(PanelSide::Left);
    assert!(inside_panels.left.git.in_repository());
}

#[test]
fn an_answer_is_shown_and_one_from_before_a_folder_change_is_dropped() {
    let first = tempfile::tempdir().expect("temp folder");
    let second = tempfile::tempdir().expect("temp folder");
    let mut panels = panels_in(first.path().to_path_buf());
    let _ = panels.start_git_scan(PanelSide::Left);
    let first_id = panels.left.git.request_id;
    let first_stop = Arc::clone(&panels.left.git.stop);

    panels.left.path = second.path().to_path_buf();
    let _ = panels.start_git_scan(PanelSide::Left);
    assert!(
        first_stop.load(Ordering::Relaxed),
        "the running read is stopped"
    );

    let old_answer = GitScan {
        summary: Some(summary(9)),
        marks: HashMap::from([(OsString::from("old.txt"), GitMark::Modified)]),
    };
    panels.apply_git_scan(PanelSide::Left, first_id, Some(old_answer));
    assert_eq!(panels.left.git.summary, None);
    assert_eq!(panels.left.git.mark_of(&OsString::from("old.txt")), None);

    let answer = GitScan {
        summary: Some(summary(3)),
        marks: HashMap::from([(OsString::from("new.txt"), GitMark::Untracked)]),
    };
    let current = panels.left.git.request_id;
    panels.apply_git_scan(PanelSide::Left, current, Some(answer));
    assert_eq!(panels.left.git.summary, Some(summary(3)));
    assert_eq!(
        panels.left.git.mark_of(&OsString::from("new.txt")),
        Some(GitMark::Untracked)
    );
}

#[test]
fn a_panel_inside_a_repository_has_a_git_column() {
    let palette = ThemePreset::Dracula.palette();
    let mut state = PanelState::new(PathBuf::from("/work"));
    state.entries = vec![
        entry("/work", "..", true, 0),
        entry("/work", "src", true, 0),
        entry("/work", "target", true, 0),
        entry("/work", "vendor", true, 0),
        entry("/work", "Cargo.toml", false, 1_024),
        entry("/work", "new.rs", false, 512),
        entry("/work", "old.rs", false, 256),
        entry("/work", "notes.txt", false, 2_048),
    ];
    state.git = PanelGit {
        folder: PathBuf::from("/work"),
        root: Some(PathBuf::from("/work")),
        summary: Some(summary(4)),
        marks: HashMap::from([
            (OsString::from("src"), GitMark::Modified),
            (OsString::from("target"), GitMark::Ignored),
            (OsString::from("vendor"), GitMark::Repository),
            (OsString::from("Cargo.toml"), GitMark::Modified),
            (OsString::from("new.rs"), GitMark::Added),
            (OsString::from("old.rs"), GitMark::Deleted),
        ]),
        ..PanelGit::default()
    };
    let noted = std::collections::HashSet::new();
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        Size::new(720.0, 260.0),
        view(
            &state,
            ViewCtx {
                side: PanelSide::Left,
                lang: Language::English,
                palette,
                is_active: true,
                visible_rows: 8,
                compare_marks: None,
                hermes_cell: None,
                columns: ColumnSettings::STANDARD,
                special_folders: &[],
                noted: &noted,
                header_drag: None,
                size_run: None,
            },
        ),
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/git-column-in-repository.png")
        .unwrap();
    assert!(matches, "the Git column differs from its image");
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("the git command line");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn scan_row(folder: &std::path::Path, name: &str) -> ScanRow {
    let path = folder.join(name);
    ScanRow {
        name: OsString::from(name),
        is_dir: path.is_dir(),
        path,
    }
}

fn repository_with_every_state() -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("temp folder");
    let root = folder.path();
    git(root, &["init", "-q"]);
    fs::write(root.join(".gitignore"), "ignored.log\n").expect("write");
    fs::write(root.join("changed.txt"), "one\n").expect("write");
    fs::write(root.join("gone.txt"), "one\n").expect("write");
    fs::create_dir_all(root.join("sub")).expect("folder");
    fs::write(root.join("sub/inner.txt"), "one\n").expect("write");
    fs::create_dir_all(root.join("clean")).expect("folder");
    fs::write(root.join("clean/file.txt"), "one\n").expect("write");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);
    fs::write(root.join("changed.txt"), "two\n").expect("write");
    fs::remove_file(root.join("gone.txt")).expect("remove");
    fs::write(root.join("sub/inner.txt"), "two\n").expect("write");
    fs::write(root.join("added.txt"), "new\n").expect("write");
    git(root, &["add", "added.txt"]);
    fs::write(root.join("untracked.txt"), "x\n").expect("write");
    fs::write(root.join("ignored.log"), "x\n").expect("write");
    folder
}

#[test]
fn the_rows_of_a_repository_get_the_letter_of_their_state() {
    let folder = repository_with_every_state();
    let root = folder.path();
    let names = [
        "changed.txt",
        "gone.txt",
        "added.txt",
        "untracked.txt",
        "ignored.log",
        "clean",
        "sub",
    ];
    let rows: Vec<ScanRow> = names.iter().map(|name| scan_row(root, name)).collect();
    let found = scan(Some(root), &rows, &Arc::new(AtomicBool::new(false))).expect("scan");
    let mark = |name: &str| found.marks.get(&OsString::from(name)).copied();
    assert_eq!(mark("changed.txt"), Some(GitMark::Modified));
    assert_eq!(mark("gone.txt"), Some(GitMark::Deleted));
    assert_eq!(mark("added.txt"), Some(GitMark::Added));
    assert_eq!(mark("untracked.txt"), Some(GitMark::Untracked));
    assert_eq!(mark("ignored.log"), Some(GitMark::Ignored));
    assert_eq!(mark("clean"), None);
    assert_eq!(
        mark("sub"),
        Some(GitMark::Modified),
        "a folder shows the change inside it"
    );
    assert_eq!(found.summary.expect("summary").changed, 5);
}

#[test]
fn a_folder_of_repositories_marks_each_repository_and_shows_the_column() {
    let folder = tempfile::tempdir().expect("temp folder");
    fs::create_dir_all(folder.path().join("one/.git")).expect("repository");
    fs::create_dir_all(folder.path().join("two/.git")).expect("repository");
    fs::create_dir_all(folder.path().join("plain")).expect("folder");
    let rows: Vec<ScanRow> = ["one", "two", "plain"]
        .iter()
        .map(|name| scan_row(folder.path(), name))
        .collect();
    let found = scan(None, &rows, &Arc::new(AtomicBool::new(false))).expect("scan");
    let mut git = PanelGit::default();
    assert!(!git.shows_column());
    git.marks = found.marks;
    assert!(git.shows_column() && !git.in_repository());
    assert_eq!(
        git.mark_of(&OsString::from("one")),
        Some(GitMark::Repository)
    );
    assert_eq!(git.mark_of(&OsString::from("plain")), None);
}
