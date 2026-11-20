//! 1.6.0 acceptance, the Git flows: the Git column and status line (#336)
//! and the Git view tab (#337), on real repositories made with the git
//! command line in a temporary folder. No remote is ever contacted: `origin`
//! is only a name for a remote-tracking branch.

use std::path::{Path, PathBuf};
use std::process::Command;

use iced::keyboard::key::Named;
use iced::keyboard::Key;
use ncrs_files::context_menu::ContextAction;
use ncrs_ui_kit::git_mark::GitMark;

use super::acceptance_v160::{go_to, pump_until, shoot, Live, World};
use super::Session;
use crate::app::App;
use crate::messages::{MenuMessage, Message, PanelSide, ShownTab};

const AUTHORS: [(&str, &str); 3] = [
    ("Anna Becker", "anna@example.org"),
    ("Jonas Weber", "jonas@example.org"),
    ("Marc Winter", "marc@example.org"),
];

const FIRST_TICK: u64 = 1_790_000_000;

fn run_git(dir: &Path, tick: u64, author: usize, args: &[&str]) -> String {
    let (name, email) = AUTHORS[author % AUTHORS.len()];
    let date = format!("{} +0000", FIRST_TICK + tick * 5_400);
    let output = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("the git command line");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

const ROOT_FILES: [&str; 18] = [
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "strings.json",
    "rustfmt.toml",
    "clippy.toml",
    "deny.toml",
    "build.rs",
    "justfile",
    ".editorconfig",
    ".gitattributes",
    "Dockerfile",
    "Makefile",
    "todo.txt",
    "SECURITY.md",
];

const SUMMARIES: [&str; 21] = [
    "Read the config file before the first window",
    "Fix the cursor jump after a rename",
    "Show the size column right-aligned",
    "Keep the sort order when a folder is reloaded",
    "Add the favourites bar",
    "Give the status bar a progress line",
    "Move the search into its own crate",
    "Stop the copy when the target is full",
    "Name the trash entries by their origin",
    "Add tags to the file rows",
    "Read archives without unpacking them",
    "Fix the wrong icon of a link to a folder",
    "Speed up the first listing of a big folder",
    "Add the command palette",
    "Handle files that are not UTF-8",
    "Fix the crash on an empty archive",
    "Show the date as written in the system",
    "Open a file with the program of its type",
    "Remember the window size",
    "Put the notes into a tab",
    "Warn before deleting for good",
];

fn engine_source() -> String {
    (1..=80)
        .map(|line| format!("pub fn step_{line:02}() -> u32 {{ {line} }}\n"))
        .collect()
}

fn write(root: &Path, relative: &str, text: &str) {
    super::write(&root.join(relative), text);
}

fn append(root: &Path, relative: &str, line: &str) {
    let path = root.join(relative);
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(line);
    text.push('\n');
    super::write(&path, &text);
}

fn commit_all(root: &Path, tick: u64, author: usize, summary: &str) {
    run_git(root, tick, author, &["add", "-A"]);
    run_git(root, tick, author, &["commit", "-q", "-m", summary]);
}

/// The repository the Git flows look at: 33 commits on `main`, a merged
/// feature branch, a branch ahead by two checked out in a linked worktree
/// with two changes, two tags, and in the main checkout one modified, one
/// added, one untracked and one ignored file and an ignored `target` folder.
/// Returns the main checkout.
fn build_main_repository(parent: &Path) -> PathBuf {
    let root = parent.join("ncrs-demo");
    std::fs::create_dir_all(&root).unwrap();
    run_git(&root, 0, 0, &["init", "-q"]);
    write(&root, ".gitignore", "target/\n*.log\n");
    for name in ROOT_FILES {
        write(&root, name, &format!("# {name}\n"));
    }
    for n in 1..=22 {
        write(
            &root,
            &format!("kapitel-{n:02}.md"),
            &format!("# Kapitel {n}\n\nEntwurf.\n"),
        );
    }
    write(&root, "src/engine.rs", &engine_source());
    write(&root, "src/main.rs", "fn main() {}\n");
    write(&root, "src/theme.rs", "pub const DARK: bool = false;\n");
    write(&root, "docs/guide.md", "# Guide\n");
    write(&root, "assets/logo.svg", "<svg/>\n");
    write(&root, "assets/icon.svg", "<svg/>\n");
    write(&root, "tests/smoke.rs", "#[test]\nfn smoke() {}\n");
    write(&root, "scripts/release.sh", "#!/bin/sh\n");
    write(&root, "packaging/Info.plist", "<plist/>\n");
    commit_all(&root, 1, 0, "Start the project");

    let touched = [
        "src/main.rs",
        "docs/guide.md",
        "CHANGELOG.md",
        "README.md",
        "tests/smoke.rs",
    ];
    for (index, summary) in SUMMARIES.iter().enumerate() {
        let tick = 2 + index as u64;
        let author = index % AUTHORS.len();
        if index == 13 {
            // Two changes far apart: two hunks in one diff.
            let source = engine_source()
                .replace("step_03() -> u32 { 3 }", "step_03() -> u32 { 33 }")
                .replace("step_60() -> u32 { 60 }", "step_60() -> u32 { 600 }");
            write(&root, "src/engine.rs", &source);
        } else {
            append(
                &root,
                touched[index % touched.len()],
                &format!("- {summary}"),
            );
        }
        commit_all(&root, tick, author, summary);
        if index == 8 {
            let sha = run_git(&root, tick, 0, &["rev-parse", "HEAD"]);
            run_git(&root, tick, 0, &["tag", "v1.0", &sha]);
        }
        if index == 18 {
            run_git(&root, tick, 1, &["tag", "-a", "v1.1", "-m", "Version 1.1"]);
        }
    }

    run_git(&root, 30, 0, &["checkout", "-q", "-b", "feature/dark-mode"]);
    for (step, file) in ["src/theme.rs", "assets/icon.svg", "docs/guide.md"]
        .iter()
        .enumerate()
    {
        append(&root, file, &format!("// dark mode {step}"));
        commit_all(
            &root,
            31 + step as u64,
            1,
            &format!("Dark mode, part {}", step + 1),
        );
    }
    run_git(&root, 34, 0, &["checkout", "-q", "main"]);
    for (step, file) in ["CHANGELOG.md", "SECURITY.md", "todo.txt"]
        .iter()
        .enumerate()
    {
        append(&root, file, &format!("main {step}"));
        commit_all(
            &root,
            35 + step as u64,
            2,
            &format!("Prepare the release, step {}", step + 1),
        );
    }
    run_git(
        &root,
        40,
        0,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'feature/dark-mode'",
            "feature/dark-mode",
        ],
    );
    for (step, summary) in [
        "Update the screenshots",
        "Fix a typo in the guide",
        "Bump the version",
        "Write the release notes",
    ]
    .iter()
    .enumerate()
    {
        append(&root, "README.md", &format!("- {summary}"));
        commit_all(&root, 41 + step as u64, step, summary);
    }

    // The remote-tracking branch: three commits behind a commit that is only
    // there, so the checkout is 3 ahead and 1 behind. Nothing is fetched.
    run_git(
        &root,
        50,
        0,
        &["remote", "add", "origin", "/nonexistent/origin.git"],
    );
    let base = run_git(&root, 50, 0, &["rev-parse", "main~3"]);
    let tree = run_git(&root, 50, 0, &["rev-parse", "main~3^{tree}"]);
    let only_there = run_git(
        &root,
        50,
        1,
        &["commit-tree", &tree, "-p", &base, "-m", "Hotfix on origin"],
    );
    run_git(
        &root,
        50,
        0,
        &["update-ref", "refs/remotes/origin/main", &only_there],
    );
    run_git(&root, 50, 0, &["config", "branch.main.remote", "origin"]);
    run_git(
        &root,
        50,
        0,
        &["config", "branch.main.merge", "refs/heads/main"],
    );

    // A second branch, ahead by two, in a linked worktree with two changes.
    // Relative, since git on Windows cannot create the canonical `\\?\` path.
    let linked = parent.join("ncrs-demo-logo");
    run_git(
        &root,
        51,
        0,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fix/logo",
            "../ncrs-demo-logo",
            "main",
        ],
    );
    append(&linked, "assets/logo.svg", "<!-- new logo -->");
    commit_all(&linked, 52, 0, "Redraw the logo");
    append(&linked, "docs/guide.md", "The logo changed.");
    commit_all(&linked, 53, 1, "Mention the logo in the guide");
    append(&linked, "README.md", "- the logo, unfinished");
    write(&linked, "idee.txt", "ein Logo mit Welle\n");

    // The main checkout: one modified, one added, one untracked, one ignored
    // file, and an ignored folder.
    append(&root, "README.md", "- not committed yet");
    write(&root, "Roadmap.md", "# Roadmap\n");
    run_git(&root, 54, 0, &["add", "Roadmap.md"]);
    write(&root, "scratch.txt", "scribbles\n");
    write(&root, "build.log", "compiled\n");
    for n in 0..4 {
        write(&root, &format!("target/debug/out-{n}.o"), "binary");
    }
    root
}

fn plain_repository(path: &Path, commits: u64) {
    std::fs::create_dir_all(path).unwrap();
    run_git(path, 0, 0, &["init", "-q"]);
    for n in 0..commits {
        append(path, "README.md", &format!("line {n}"));
        commit_all(path, 1 + n, n as usize, &format!("Change {n}"));
    }
}

/// `Entwicklung` with the main repository, its linked worktree, three more
/// repositories, plain folders and loose files: 56 rows.
fn build_development_folder(root: &Path) -> PathBuf {
    let parent = root.join("Entwicklung");
    std::fs::create_dir_all(&parent).unwrap();
    build_main_repository(&parent);
    for (name, commits) in [("kleine-tools", 3), ("skripte", 2), ("webseite", 5)] {
        plain_repository(&parent.join(name), commits);
    }
    for folder in ["Archiv", "Vorlagen"] {
        write(&parent, &format!("{folder}/info.txt"), "x");
    }
    for n in 0..45 {
        write(&parent, &format!("Notiz-{n:02}.txt"), "x");
    }
    parent
}

fn git_ready(app: &App) -> bool {
    app.panel(PanelSide::Left).git.summary.is_some()
}

fn git_scanned(app: &App) -> bool {
    app.panel(PanelSide::Left)
        .git
        .marks
        .values()
        .any(|mark| *mark == GitMark::Repository)
}

fn mark(app: &App, name: &str) -> Option<GitMark> {
    app.panel(PanelSide::Left)
        .git
        .mark_of(&std::ffi::OsString::from(name))
}

#[test]
fn git_column_and_status_line_in_a_repository() {
    let world = World::new("git-column", "");
    let parent = build_development_folder(&world.root);
    let repository = parent.join("ncrs-demo");
    // More changes among the visible rows: a file changed inside `src`, more
    // modified files, and a file taken out of the index but left on disk, a new staged and an untracked file near the top of the list.
    append(&repository, "src/main.rs", "// edited");
    append(&repository, "Cargo.toml", "# edited");
    append(&repository, "kapitel-03.md", "Neuer Absatz.");
    run_git(
        &repository,
        55,
        0,
        &["rm", "-q", "--cached", "CHANGELOG.md"],
    );
    write(&repository, "Anhang.md", "# Anhang\n");
    run_git(&repository, 55, 0, &["add", "Anhang.md"]);
    write(&repository, "Aufgaben.txt", "offen\n");
    let mut session = world.boot_in(&repository);
    pump_until(&mut session, 20, git_ready);

    let panel = session.app.panel(PanelSide::Left);
    assert!(panel.git.in_repository());
    assert!(
        panel.entries.len() >= 50,
        "a dense repository folder: {}",
        panel.entries.len()
    );
    let summary = panel.git.summary.clone().unwrap();
    assert_eq!(summary.branch.as_deref(), Some("main"));
    assert_eq!((summary.ahead, summary.behind), (3, 1));
    assert_eq!(
        summary.changed, 9,
        "modified, added and untracked count, the ignored ones do not"
    );
    assert_eq!(mark(&session.app, "CHANGELOG.md"), Some(GitMark::Deleted));
    assert_eq!(mark(&session.app, "Anhang.md"), Some(GitMark::Added));
    assert_eq!(mark(&session.app, "Aufgaben.txt"), Some(GitMark::Untracked));
    assert_eq!(mark(&session.app, "src"), Some(GitMark::Modified));
    assert_eq!(mark(&session.app, "Cargo.toml"), Some(GitMark::Modified));
    assert_eq!(mark(&session.app, "kapitel-03.md"), Some(GitMark::Modified));
    assert_eq!(mark(&session.app, "README.md"), Some(GitMark::Modified));
    assert_eq!(mark(&session.app, "Roadmap.md"), Some(GitMark::Added));
    assert_eq!(mark(&session.app, "scratch.txt"), Some(GitMark::Untracked));
    assert_eq!(mark(&session.app, "build.log"), Some(GitMark::Ignored));
    assert_eq!(mark(&session.app, "target"), Some(GitMark::Ignored));
    assert_eq!(
        mark(&session.app, "kapitel-01.md"),
        None,
        "clean rows have no mark"
    );
    shoot(&session.app, "17-git-spalte-im-repository");
}

#[test]
fn the_parent_of_several_repositories_marks_them() {
    let world = World::new("git-parent", "");
    let parent = build_development_folder(&world.root);
    let mut session = world.boot_in(&parent);
    pump_until(&mut session, 20, git_scanned);

    let panel = session.app.panel(PanelSide::Left);
    assert!(
        !panel.git.in_repository(),
        "the parent is no repository itself"
    );
    assert!(panel.git.summary.is_none());
    for name in ["ncrs-demo", "kleine-tools", "skripte", "webseite"] {
        assert_eq!(
            mark(&session.app, name),
            Some(GitMark::Repository),
            "{name}"
        );
    }
    assert_eq!(mark(&session.app, "Archiv"), None);
    assert!(panel.entries.len() >= 50);
    shoot(&session.app, "18-git-markierung-repositories");
}

#[test]
fn outside_a_repository_there_is_no_git_column() {
    let world = World::new("git-outside", "");
    let _parent = build_development_folder(&world.root);
    let mut session = world.boot();
    pump_until(&mut session, 5, |app| !app.panel(PanelSide::Left).loading);
    session.settle();
    let panel = session.app.panel(PanelSide::Left);
    assert!(!panel.git.in_repository());
    assert!(panel.git.summary.is_none());
    assert!(panel.git.marks.is_empty());
    shoot(&session.app, "19-ohne-git-spalte");
}

fn open_git_view_from_the_context_menu<P: Live>(session: &mut Session<P>) {
    go_to(session, "ncrs-demo");
    session.send(Message::Files(MenuMessage::ContextMenuKey));
    assert!(session.app.context_menu_for_test().is_some());
    for _ in 0..40 {
        if session
            .app
            .context_menu_for_test()
            .and_then(|menu| menu.selected_action())
            == Some(ContextAction::GitView)
        {
            break;
        }
        session.press(Key::Named(Named::ArrowDown));
    }
    assert_eq!(
        session
            .app
            .context_menu_for_test()
            .and_then(|menu| menu.selected_action()),
        Some(ContextAction::GitView)
    );
    session.press(Key::Named(Named::Enter));
    session.settle();
}

#[test]
fn the_git_view_shows_the_graph_details_diff_and_filter() {
    let world = World::new("git-view", "");
    let parent = build_development_folder(&world.root);
    let mut session = world.boot_in(&parent);
    pump_until(&mut session, 20, git_scanned);
    open_git_view_from_the_context_menu(&mut session);

    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Git);
    let view = session.app.git_view_for_test();
    let overview = view.overview.clone().expect("the refs were read");
    assert_eq!(overview.tags.len(), 2);
    assert_eq!(overview.local.len(), 3, "main, feature/dark-mode, fix/logo");
    assert_eq!(overview.worktrees.len(), 2);
    let linked = overview
        .worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some("fix/logo"))
        .expect("the linked worktree");
    assert_eq!(linked.wip, 2, "the WIP chip of the linked worktree");
    assert_eq!((linked.ahead, linked.behind), (2, 0), "against main");
    let current = view.current_worktree().expect("the opened worktree");
    assert_eq!(current.wip, 3);
    assert!(view.has_working_row());
    let log = view.log.clone().expect("the log was read");
    assert!(log.commits.len() >= 33, "{} commits", log.commits.len());
    shoot(&session.app, "20-git-ansicht");

    // The merge commit, then a normal one.
    let working_row = 1;
    let merge_row = working_row
        + log
            .commits
            .iter()
            .position(|commit| commit.is_merge)
            .expect("the merge commit");
    session.send(Message::GitView(ncrs_git_view::Message::CommitPressed(
        merge_row,
    )));
    let detail = session
        .app
        .git_view_for_test()
        .detail
        .clone()
        .expect("details");
    let info = detail.info.expect("a commit");
    assert_eq!(info.parents.len(), 2, "the merge has two parents");
    assert!(info.message.contains("Merge branch 'feature/dark-mode'"));
    shoot(&session.app, "21-git-details-merge");

    let two_hunks = log
        .commits
        .iter()
        .position(|commit| commit.summary == "Add the command palette")
        .expect("the commit with two hunks");
    session.send(Message::GitView(ncrs_git_view::Message::CommitPressed(
        working_row + two_hunks,
    )));
    let file_detail = session
        .app
        .git_view_for_test()
        .detail
        .clone()
        .expect("details");
    let file = file_detail.files.first().expect("a changed file").clone();
    assert_eq!(file.path, "src/engine.rs");
    assert!(
        file.added > 0 && file.removed > 0,
        "+{} -{}",
        file.added,
        file.removed
    );
    shoot(&session.app, "22-git-details-plus-minus");

    // Enter goes to the file list, Enter again opens the diff.
    session.press(Key::Named(Named::Enter));
    session.press(Key::Named(Named::Enter));
    let shown = session
        .app
        .git_view_for_test()
        .diff
        .clone()
        .expect("the diff");
    assert!(!shown.unified, "side by side first");
    assert!(shown
        .diff
        .as_ref()
        .is_some_and(|diff| diff.hunks.len() >= 2));
    shoot(&session.app, "23-git-diff-nebeneinander");

    session.send(Message::GitView(ncrs_git_view::Message::SetUnified(true)));
    assert!(
        session
            .app
            .git_view_for_test()
            .diff
            .as_ref()
            .unwrap()
            .unified
    );
    shoot(&session.app, "24-git-diff-vereint");

    assert_eq!(
        session.app.git_view_for_test().diff.as_ref().unwrap().hunk,
        0
    );
    session.press(Key::Character("n".into()));
    assert_eq!(
        session.app.git_view_for_test().diff.as_ref().unwrap().hunk,
        1,
        "n jumps to the next change"
    );
    shoot(&session.app, "25-git-diff-naechste-aenderung");

    session.press(Key::Named(Named::Escape));
    assert!(
        session.app.git_view_for_test().diff.is_none(),
        "Esc leaves the diff"
    );
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Git);

    // The filter by author.
    let everyone = session
        .app
        .git_view_for_test()
        .log
        .as_ref()
        .unwrap()
        .commits
        .len();
    session.send(Message::GitView(ncrs_git_view::Message::FilterChanged(
        "Anna".to_owned(),
    )));
    let filtered = session.app.git_view_for_test().log.clone().unwrap();
    assert!(!filtered.commits.is_empty() && filtered.commits.len() < everyone);
    assert!(filtered
        .commits
        .iter()
        .all(|commit| commit.author.contains("Anna")));
    shoot(&session.app, "26-git-filter-autor");
}
