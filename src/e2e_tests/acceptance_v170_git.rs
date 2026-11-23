//! 1.7.0 acceptance, the Git actions of the Git tab (#338): staging, commits,
//! branches, push and pull, merge and rebase with conflicts, the stash and the
//! command list. The real app drives them with the keys and messages a user
//! produces, on real repositories (a local bare `origin` and a second clone
//! for the other side) made with the git command line in a temporary folder.
//! After each step the repository itself is asked, not only the view.
//!
//! Pictures: `NCRS_ACCEPTANCE_SHOTS=1 cargo test -p ncrs --bin ncrs acceptance_v170_git -- --test-threads=1`
//! writes them to `target/acceptance/v170-git/`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor;
use iced::Rectangle;
use iced_test::selector::Candidate;
use ncrs_git::PullStrategy;
use ncrs_git_view::actions::{ConflictAction, Dialog, MenuTarget, Notice, Question, Side};
use ncrs_git_view::state::SidebarItem;
use ncrs_git_view::Message as View;

use super::acceptance_v160::{shoot_into, Live, World};
use super::acceptance_v160_git::{append, commit_all, run_git, write};
use super::Session;
use crate::messages::{CommandPaletteMessage, GitTabId, Message, ShownTab};

const SHOT_FOLDER: &str = "target/acceptance/v170-git";

static ISOLATE: Once = Once::new();

/// The app's own git calls read no config of the developer's machine.
fn isolate_git() {
    ISOLATE.call_once(|| {
        let empty = std::env::temp_dir().join("ncrs-e2e-git-empty-config");
        std::fs::write(&empty, "").unwrap();
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
}

fn lines(count: usize, line: impl Fn(usize) -> String) -> String {
    (1..=count).map(|n| format!("{}\n", line(n))).collect()
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} has no {from:?}");
    write(root, relative, &text.replacen(from, to, 1));
}

fn theme_source() -> String {
    lines(40, |n| {
        format!("pub const COLOR_{n:02}: u32 = 0x{:06x};", n * 7)
    })
}

fn guide_source() -> String {
    lines(12, |n| {
        format!("Schritt {n}: Der Ablauf, wie er im Alltag aussieht.")
    })
}

const PLAIN_FILES: [&str; 22] = [
    "Cargo.toml",
    "LICENSE",
    "CHANGELOG.md",
    "strings.json",
    "justfile",
    "todo.txt",
    "src/main.rs",
    "src/config.rs",
    "src/search.rs",
    "src/index.rs",
    "src/files.rs",
    "docs/install.md",
    "docs/shortcuts.md",
    "docs/faq.md",
    "assets/logo.svg",
    "assets/icon.svg",
    "assets/banner.svg",
    "tests/smoke.rs",
    "tests/config_test.rs",
    "scripts/release.sh",
    "scripts/build.sh",
    "packaging/Info.plist",
];

/// What the history says and the file each commit touched.
const HISTORY: [(&str, &str); 14] = [
    (
        "Read the config file before the first window",
        "src/config.rs",
    ),
    ("Fix the cursor jump after a rename", "src/files.rs"),
    ("Show the size column right-aligned", "src/files.rs"),
    (
        "Keep the sort order when a folder is reloaded",
        "src/files.rs",
    ),
    ("Add the favourites bar", "src/main.rs"),
    ("Give the status bar a progress line", "src/main.rs"),
    ("Move the search into its own crate", "src/search.rs"),
    ("Name the trash entries by their origin", "docs/faq.md"),
    ("Add tags to the file rows", "src/index.rs"),
    ("Read archives without unpacking them", "src/files.rs"),
    ("Explain the shortcuts", "docs/shortcuts.md"),
    ("Add the command palette", "src/main.rs"),
    ("Handle files that are not UTF-8", "src/config.rs"),
    ("Write the install notes", "docs/install.md"),
];

struct Repositories {
    repo: PathBuf,
    origin: PathBuf,
    clone: PathBuf,
}

/// 30 files in five folders, 15 commits, a bare `origin` with everything
/// pushed and a second clone of it.
fn build_repositories(parent: &Path) -> Repositories {
    std::fs::create_dir_all(parent).unwrap();
    let repo = parent.join("ncrs-demo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, 0, 0, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "Marc Winter"),
        ("user.email", "marc@example.org"),
        ("commit.gpgsign", "false"),
    ] {
        run_git(&repo, 0, 0, &["config", key, value]);
    }
    write(&repo, ".gitignore", "target/\n*.log\n");
    write(
        &repo,
        "README.md",
        "# ncrs-demo\n\nEin Dateimanager.\n\n- Zwei Spalten\n- Tastatur zuerst\n",
    );
    for name in PLAIN_FILES {
        write(&repo, name, &format!("// {name}\n"));
    }
    write(
        &repo,
        "src/engine.rs",
        &lines(80, |n| format!("pub fn step_{n:02}() -> u32 {{ {n} }}")),
    );
    write(&repo, "src/theme.rs", &theme_source());
    write(&repo, "docs/guide.md", &guide_source());
    write(&repo, "tests/engine_test.rs", "#[test]\nfn engine() {}\n");
    write(&repo, "assets/colors.svg", "<svg/>\n");
    write(&repo, "scripts/check.sh", "#!/bin/sh\n");
    write(&repo, "docs/changes.md", "# Änderungen\n");
    write(&repo, "src/lib.rs", "pub mod engine;\n");
    commit_all(&repo, 1, 0, "Start the project");
    for (index, (summary, file)) in HISTORY.iter().enumerate() {
        append(&repo, file, &format!("// {summary}"));
        commit_all(&repo, 2 + index as u64, index % 3, summary);
    }

    let origin = ncrs_intake::without_verbatim_prefix(parent.join("origin.git"));
    run_git(parent, 20, 0, &["init", "--bare", "-q", "origin.git"]);
    run_git(
        &repo,
        20,
        0,
        &["remote", "add", "origin", &origin.to_string_lossy()],
    );
    run_git(&repo, 20, 0, &["push", "-q", "-u", "origin", "main"]);
    let clone = parent.join("zweiter-klon");
    run_git(
        parent,
        21,
        1,
        &["clone", "-q", &origin.to_string_lossy(), "zweiter-klon"],
    );
    Repositories {
        repo,
        origin,
        clone,
    }
}

fn shown_git_tab<P: Live>(session: &Session<P>) -> GitTabId {
    match session.app.shown_tab_for_test() {
        ShownTab::Git(id) => id,
        other => panic!("a Git tab is shown, not {other:?}"),
    }
}

fn shifted_command() -> Modifiers {
    Modifiers::COMMAND | Modifiers::SHIFT
}

/// One flow: the app showing the Git tab of the repository, and the other
/// repositories around it.
struct Flow<P: Live> {
    _world: World,
    session: Session<P>,
    id: GitTabId,
    repo: PathBuf,
    origin: PathBuf,
    clone: PathBuf,
}

fn start(label: &str) -> Flow<impl Live> {
    isolate_git();
    let world = World::new(label, "");
    let Repositories {
        repo,
        origin,
        clone,
    } = build_repositories(&world.root.join("Entwicklung"));
    let mut session = world.boot_in(&repo);
    session.send(Message::OpenGitView);
    let id = shown_git_tab(&session);
    let flow = Flow {
        _world: world,
        session,
        id,
        repo,
        origin,
        clone,
    };
    assert!(
        flow.view().actions.local.is_some(),
        "the actions read the repository"
    );
    flow
}

impl<P: Live> Flow<P> {
    fn view(&self) -> &ncrs_git_view::State {
        self.session.app.git_view_for_test()
    }

    fn send(&mut self, message: View) {
        self.session.send(Message::GitView(self.id, message));
        assert!(
            self.view().actions.busy.is_none(),
            "every call to git has ended"
        );
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.session.press_with(key, modifiers);
    }

    fn press_shifted(&mut self, letter: &str) {
        self.press(Key::Character(letter.into()), shifted_command());
    }

    fn git(&self, args: &[&str]) -> String {
        run_git(&self.repo, 90, 0, args)
    }

    fn shoot(&self, name: &str) {
        shoot_into(SHOT_FOLDER, &self.session.app, name, |_| {});
    }

    /// The row of the left list of the local branch `name`.
    fn branch_row(&self, name: &str) -> usize {
        let state = self.view();
        let overview = state.overview.as_ref().expect("the refs");
        state
            .sidebar_items()
            .iter()
            .position(|item| {
                matches!(item, SidebarItem::Local(index) if overview.local[*index].name == name)
            })
            .unwrap_or_else(|| panic!("no branch {name}"))
    }

    fn pick_branch(&mut self, name: &str) {
        let row = self.branch_row(name);
        self.send(View::SidebarPressed(row));
    }

    fn pick_stash(&mut self) {
        let row = self
            .view()
            .sidebar_items()
            .iter()
            .position(|item| matches!(item, SidebarItem::Stash(0)))
            .expect("a stash row");
        self.send(View::SidebarPressed(row));
    }

    fn unstaged_row(&self, path: &str) -> usize {
        let local = self.view().actions.local.as_ref().unwrap();
        local
            .unstaged
            .iter()
            .position(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} is not among the changes"))
    }

    fn current_branch(&self) -> String {
        self.git(&["branch", "--show-current"])
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.repo.join(relative)).unwrap()
    }

    /// Takes what happened outside the app into the tab.
    fn refresh(&mut self) {
        self.send(View::Refresh);
    }

    fn dialog(&self) -> Option<&Dialog> {
        self.view().actions.dialog.as_ref()
    }

    fn notice(&self) -> Option<&Notice> {
        self.view().actions.notice.as_ref()
    }

    /// The picture with the mouse over the button of "undo".
    fn shoot_undo_tooltip(&self, name: &str) {
        let mut found: Vec<Rectangle> = Vec::new();
        shoot_into(SHOT_FOLDER, &self.session.app, name, |simulator| {
            let _ignored = simulator.find(|candidate: Candidate<'_>| {
                let bounds = candidate.bounds();
                // The icon buttons of the header, left of the filter field.
                if bounds.y < 70.0
                    && bounds.x < 500.0
                    && bounds.width < 80.0
                    && bounds.height < 60.0
                {
                    found.push(bounds);
                }
                None::<()>
            });
            let undo = found
                .iter()
                .filter(|bounds| bounds.width > 20.0 && bounds.height > 20.0)
                .max_by(|left, right| left.x.total_cmp(&right.x))
                .expect("the buttons of the toolbar");
            simulator.point_at(undo.center());
        });
    }
}

/// The clone makes a commit and pushes it, as a colleague would.
fn colleague_pushes(clone: &Path, file: &str, line: &str, summary: &str) {
    append(clone, file, line);
    commit_all(clone, 30, 1, summary);
    run_git(clone, 30, 1, &["push", "-q"]);
}

fn local_commit(repo: &Path, file: &str, line: &str, summary: &str) {
    append(repo, file, line);
    commit_all(repo, 31, 0, summary);
}

#[test]
fn one_hunk_is_staged_a_file_is_discarded_and_undo_brings_it_back() {
    let mut flow = start("git-stage");
    edit(
        &flow.repo,
        "src/engine.rs",
        "step_03() -> u32 { 3 }",
        "step_03() -> u32 { 33 }",
    );
    edit(
        &flow.repo,
        "src/engine.rs",
        "step_60() -> u32 { 60 }",
        "step_60() -> u32 { 600 }",
    );
    append(&flow.repo, "README.md", "- Die Hilfeseite fehlt noch");
    write(&flow.repo, "docs/idee.md", "# Idee\n");
    flow.refresh();
    flow.send(View::SidebarPressed(0));
    assert!(flow.view().actions.changes_open);
    let engine = flow.unstaged_row("src/engine.rs");
    flow.send(View::SelectChange(
        ncrs_git_view::actions::ChangeSide::Unstaged,
        engine,
    ));
    let local = flow.view().actions.local.as_ref().unwrap();
    assert_eq!(local.diff.as_ref().unwrap().hunks.len(), 2);
    flow.shoot("01-lokale-aenderungen-mit-hunk");

    flow.send(View::StageHunk(0));
    let staged = flow.git(&["diff", "--cached"]);
    assert!(staged.contains("step_03() -> u32 { 33 }"), "{staged}");
    assert!(!staged.contains("{ 600 }"), "only the first hunk: {staged}");
    assert_eq!(
        flow.git(&["diff", "--cached", "--name-only"]),
        "src/engine.rs"
    );
    assert!(flow.git(&["diff"]).contains("{ 600 }"));

    let readme = flow.unstaged_row("README.md");
    flow.send(View::DiscardFile(readme));
    assert_eq!(
        flow.dialog(),
        Some(&Dialog::Ask(Question::DiscardFile {
            path: "README.md".to_owned()
        }))
    );
    flow.shoot("02-verwerfen-nachfrage");

    flow.press(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(flow.dialog(), None, "Escape closes the question");
    assert!(
        matches!(flow.session.app.shown_tab_for_test(), ShownTab::Git(_)),
        "and leaves the tab open"
    );
    assert!(flow.read("README.md").contains("Hilfeseite"));

    flow.send(View::DiscardFile(readme));
    flow.press(Key::Named(Named::Enter), Modifiers::empty());
    assert_eq!(flow.dialog(), None);
    assert_eq!(flow.git(&["diff", "--", "README.md"]), "", "restored");
    assert!(!flow.read("README.md").contains("Hilfeseite"));
    assert!(flow.view().actions.sync.as_ref().unwrap().undo.is_some());
    flow.shoot_undo_tooltip("03-rueckgaengig-tooltip");

    flow.press_shifted("y");
    assert!(flow.read("README.md").contains("Hilfeseite"));
    assert_eq!(flow.view().actions.sync.as_ref().unwrap().undo, None);
}

#[test]
fn a_commit_is_made_undone_made_again_and_amended() {
    let mut flow = start("git-commit");
    let before = flow.git(&["rev-list", "--count", "HEAD"]);
    assert_eq!(before, "15");
    append(&flow.repo, "docs/install.md", "Zuerst das Paket laden.");
    append(&flow.repo, "src/config.rs", "// Pfad aus der Umgebung");
    flow.refresh();
    flow.send(View::SidebarPressed(0));
    flow.press_shifted("i");
    assert_eq!(
        flow.git(&["diff", "--cached", "--name-only"]),
        "docs/install.md\nsrc/config.rs"
    );

    flow.send(View::CommitTitleChanged(
        "Describe the install steps".to_owned(),
    ));
    flow.send(View::CommitBodyEdited(text_editor::Action::Edit(
        text_editor::Edit::Paste(Arc::new("The package first.\nThen the path.".to_owned())),
    )));
    flow.shoot("04-commit-mit-titel-und-text");
    flow.press(Key::Named(Named::Enter), Modifiers::COMMAND);
    assert_eq!(
        flow.git(&["log", "-1", "--format=%s"]),
        "Describe the install steps"
    );
    let body = flow.git(&["log", "-1", "--format=%b"]);
    assert!(
        body.contains("The package first.") && body.contains("Then the path."),
        "{body}"
    );
    assert_eq!(flow.git(&["rev-list", "--count", "HEAD"]), "16");
    assert_eq!(flow.git(&["status", "--porcelain"]), "");

    flow.press_shifted("y");
    assert_eq!(
        flow.git(&["rev-list", "--count", "HEAD"]),
        "15",
        "the commit is gone"
    );
    assert_eq!(
        flow.git(&["diff", "--cached", "--name-only"]),
        "docs/install.md\nsrc/config.rs",
        "the changes are staged again"
    );

    flow.send(View::CommitTitleChanged("Describe the install".to_owned()));
    flow.press(Key::Named(Named::Enter), Modifiers::COMMAND);
    assert_eq!(flow.git(&["rev-list", "--count", "HEAD"]), "16");
    flow.send(View::ToggleAmend);
    flow.send(View::CommitTitleChanged(
        "Describe the install steps in order".to_owned(),
    ));
    flow.send(View::Commit);
    assert_eq!(
        flow.git(&["log", "-1", "--format=%s"]),
        "Describe the install steps in order"
    );
    assert_eq!(
        flow.git(&["rev-list", "--count", "HEAD"]),
        "16",
        "amended, not added"
    );
}

#[test]
fn a_branch_is_made_renamed_checked_out_and_deleted_after_a_warning() {
    let mut flow = start("git-branch");
    flow.pick_branch("main");
    flow.press_shifted("t");
    assert!(flow.view().actions.new_branch.is_some());
    flow.session.type_into_widgets("feature/hilfe");
    assert_eq!(
        flow.view().actions.new_branch.as_ref().unwrap().name,
        "feature/hilfe",
        "the name field has the focus when the dialog opens"
    );
    flow.shoot("05-neuer-branch");
    flow.session
        .press_in_widgets(Key::Named(Named::Enter), Modifiers::empty());
    assert!(flow.view().actions.new_branch.is_none());
    assert_eq!(flow.current_branch(), "feature/hilfe");

    local_commit(
        &flow.repo,
        "docs/faq.md",
        "Wo ist die Hilfe?",
        "Answer the help question",
    );
    flow.refresh();
    flow.pick_branch("feature/hilfe");
    flow.press(Key::Named(Named::F6), Modifiers::SHIFT);
    assert!(
        matches!(flow.dialog(), Some(Dialog::Rename(rename)) if rename.name == "feature/hilfe")
    );
    flow.session.type_into_widgets("feature/hilfe-seite");
    assert!(
        matches!(flow.dialog(), Some(Dialog::Rename(rename)) if rename.name == "feature/hilfe-seite"),
        "the name field has the focus and its text is selected"
    );
    flow.shoot("06-branch-umbenennen");
    flow.session
        .press_in_widgets(Key::Named(Named::Enter), Modifiers::empty());
    assert_eq!(flow.dialog(), None);
    let branches = flow.git(&["branch", "--format=%(refname:short)"]);
    assert!(
        branches.contains("feature/hilfe-seite") && !branches.lines().any(|b| b == "feature/hilfe"),
        "{branches}"
    );

    flow.pick_branch("main");
    flow.press_shifted("o");
    assert_eq!(flow.current_branch(), "main");

    flow.pick_branch("feature/hilfe-seite");
    flow.press(Key::Named(Named::Backspace), Modifiers::COMMAND);
    assert_eq!(
        flow.dialog(),
        Some(&Dialog::Ask(Question::DeleteBranch {
            name: "feature/hilfe-seite".to_owned()
        }))
    );
    flow.press(Key::Named(Named::Enter), Modifiers::empty());
    assert_eq!(
        flow.notice(),
        Some(&Notice::BranchNotMerged {
            branch: "feature/hilfe-seite".to_owned()
        })
    );
    assert!(flow
        .git(&["branch", "--list", "feature/hilfe-seite"])
        .contains("hilfe-seite"));
    flow.shoot("07-branch-nicht-zusammengefuehrt");

    flow.send(View::DeleteBranchAnyway);
    assert_eq!(flow.git(&["branch", "--list", "feature/hilfe-seite"]), "");
    assert_eq!(flow.notice(), None);
}

#[test]
fn a_rejected_push_is_fixed_by_pulling_first() {
    let mut flow = start("git-push");
    run_git(&flow.repo, 0, 0, &["config", "pull.rebase", "false"]);
    colleague_pushes(
        &flow.clone,
        "docs/faq.md",
        "Antwort von der Kollegin",
        "Answer from the colleague",
    );
    local_commit(
        &flow.repo,
        "src/search.rs",
        "// Treffer zählen",
        "Count the hits",
    );
    flow.refresh();
    flow.press_shifted("e");
    assert_eq!(flow.git(&["rev-list", "--count", "main..origin/main"]), "1");

    flow.press_shifted("u");
    assert!(
        matches!(flow.notice(), Some(Notice::PushRejected { behind: 1, .. })),
        "{:?}",
        flow.notice()
    );
    assert_eq!(
        run_git(&flow.origin, 91, 0, &["log", "-1", "--format=%s", "main"]),
        "Answer from the colleague",
        "origin is as it was"
    );
    flow.shoot("08-push-abgelehnt");

    flow.send(View::PullFirst);
    assert_eq!(flow.notice(), None);
    assert_eq!(flow.git(&["rev-list", "--count", "main..origin/main"]), "0");
    flow.press_shifted("u");
    assert_eq!(flow.notice(), None);
    assert_eq!(
        flow.git(&["rev-parse", "HEAD"]),
        run_git(&flow.origin, 91, 0, &["rev-parse", "main"])
    );
    let origin_log = run_git(&flow.origin, 91, 0, &["log", "--format=%s", "main"]);
    assert!(
        origin_log.contains("Count the hits") && origin_log.contains("Answer from the colleague"),
        "{origin_log}"
    );
}

#[test]
fn a_pull_on_diverged_branches_asks_and_rebases() {
    let mut flow = start("git-pull");
    colleague_pushes(
        &flow.clone,
        "docs/faq.md",
        "Antwort von der Kollegin",
        "Answer from the colleague",
    );
    local_commit(
        &flow.repo,
        "src/search.rs",
        "// Treffer zählen",
        "Count the hits",
    );
    flow.refresh();
    flow.press_shifted("e");
    flow.press_shifted("l");
    assert_eq!(flow.notice(), Some(&Notice::NeedsPullStrategy));
    flow.shoot("09-pull-strategie");

    flow.send(View::PullWith(PullStrategy::Rebase));
    assert_eq!(flow.notice(), None);
    assert_eq!(
        flow.git(&["rev-list", "--merges", "--count", "HEAD"]),
        "0",
        "linear history"
    );
    assert_eq!(
        flow.git(&["log", "-2", "--format=%s"]),
        "Count the hits\nAnswer from the colleague"
    );
}

/// Two files that both sides changed: `src/theme.rs` in two places far apart,
/// `docs/guide.md` in one. `main` is checked out afterwards.
fn make_branch_that_conflicts(repo: &Path) {
    run_git(repo, 40, 0, &["checkout", "-q", "-b", "feature/farben"]);
    edit(
        repo,
        "src/theme.rs",
        "COLOR_05: u32 = 0x000023",
        "COLOR_05: u32 = 0xff0000",
    );
    edit(
        repo,
        "src/theme.rs",
        "COLOR_30: u32 = 0x0000d2",
        "COLOR_30: u32 = 0x00ff00",
    );
    edit(
        repo,
        "docs/guide.md",
        "Schritt 3: Der Ablauf, wie er im Alltag aussieht.",
        "Schritt 3: Der neue Ablauf der Farben.",
    );
    commit_all(repo, 41, 1, "Pick the colours");
    run_git(repo, 42, 0, &["checkout", "-q", "main"]);
    edit(
        repo,
        "src/theme.rs",
        "COLOR_05: u32 = 0x000023",
        "COLOR_05: u32 = 0x112233",
    );
    edit(
        repo,
        "src/theme.rs",
        "COLOR_30: u32 = 0x0000d2",
        "COLOR_30: u32 = 0x445566",
    );
    edit(
        repo,
        "docs/guide.md",
        "Schritt 3: Der Ablauf, wie er im Alltag aussieht.",
        "Schritt 3: Der Ablauf mit dem neuen Menü.",
    );
    commit_all(repo, 43, 2, "Tune the theme");
}

#[test]
fn a_merge_with_conflicts_is_resolved_block_by_block_and_by_file() {
    let mut flow = start("git-merge");
    make_branch_that_conflicts(&flow.repo);
    flow.refresh();
    flow.pick_branch("feature/farben");
    flow.press_shifted("m");
    let merge = flow
        .view()
        .actions
        .merge
        .as_ref()
        .expect("the merge stopped at conflicts");
    let paths: Vec<&str> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["docs/guide.md", "src/theme.rs"]);
    assert_eq!(
        flow.git(&["diff", "--name-only", "--diff-filter=U"]),
        "docs/guide.md\nsrc/theme.rs"
    );

    flow.send(View::SelectConflict(1));
    flow.send(View::TakeBlock(0, Side::Mine));
    let halfway = flow.read("src/theme.rs");
    assert!(halfway.contains("COLOR_05: u32 = 0x112233"), "mine is main");
    assert!(
        halfway.contains("<<<<<<<"),
        "the second place is still open"
    );
    flow.shoot("10-konflikte-mitten-in-der-loesung");

    flow.send(View::TakeBlock(0, Side::Theirs));
    let theme = flow.read("src/theme.rs");
    assert!(
        theme.contains("COLOR_05: u32 = 0x112233") && theme.contains("COLOR_30: u32 = 0x00ff00")
    );
    assert!(!theme.contains("<<<<<<<"));
    assert_eq!(
        flow.git(&["diff", "--name-only", "--diff-filter=U"]),
        "docs/guide.md"
    );

    flow.send(View::OpenMenu(MenuTarget::Conflict(0)));
    assert!(flow.view().actions.menu.is_some());
    flow.shoot("11-konflikt-datei-menue");
    flow.send(View::ConflictPressed(0, ConflictAction::Take(Side::Both)));
    assert!(flow.view().actions.menu.is_none(), "the menu closes");
    let guide = flow.read("docs/guide.md");
    assert!(
        guide.contains("Der Ablauf mit dem neuen Menü.\nSchritt 3: Der neue Ablauf der Farben."),
        "both lines, mine first: {guide}"
    );
    assert_eq!(flow.git(&["diff", "--name-only", "--diff-filter=U"]), "");

    flow.send(View::MergeContinue);
    assert_eq!(
        flow.git(&["log", "-1", "--format=%P"])
            .split_whitespace()
            .count(),
        2,
        "a merge commit"
    );
    assert_eq!(flow.git(&["status", "--porcelain"]), "");
    assert!(flow
        .git(&["show", "HEAD:src/theme.rs"])
        .contains("COLOR_30: u32 = 0x00ff00"));
    assert!(flow.view().actions.merge.is_none());
}

#[test]
fn a_rebase_with_a_conflict_is_aborted_and_the_repository_is_as_before() {
    let mut flow = start("git-rebase");
    run_git(
        &flow.repo,
        40,
        0,
        &["checkout", "-q", "-b", "fix/startseite"],
    );
    edit(
        &flow.repo,
        "README.md",
        "Ein Dateimanager.",
        "Ein Dateimanager mit zwei Spalten.",
    );
    commit_all(&flow.repo, 41, 1, "Describe the columns");
    run_git(&flow.repo, 42, 0, &["checkout", "-q", "main"]);
    edit(
        &flow.repo,
        "README.md",
        "Ein Dateimanager.",
        "Ein schneller Dateimanager.",
    );
    commit_all(&flow.repo, 43, 2, "Describe the speed");
    run_git(&flow.repo, 44, 0, &["checkout", "-q", "fix/startseite"]);
    let before = flow.git(&["rev-parse", "HEAD"]);
    flow.refresh();

    flow.pick_branch("main");
    flow.press_shifted("x");
    assert!(flow.view().actions.rebasing);
    assert!(flow.view().actions.merge.is_some());
    assert!(flow.repo.join(".git/rebase-merge").exists());
    flow.shoot("12-rebase-mit-konflikt");

    flow.send(View::MergeAbort);
    assert_eq!(flow.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(flow.current_branch(), "fix/startseite");
    assert_eq!(flow.git(&["status", "--porcelain"]), "");
    assert!(!flow.repo.join(".git/rebase-merge").exists());
    assert!(flow.view().actions.merge.is_none());
}

#[test]
fn the_stash_is_pushed_applied_and_dropped_after_a_question() {
    let mut flow = start("git-stash");
    append(&flow.repo, "README.md", "- Noch nicht fertig");
    write(&flow.repo, "docs/idee.md", "# Idee\n");
    flow.refresh();
    flow.press_shifted("s");
    let list = flow.git(&["stash", "list"]);
    assert_eq!(list.lines().count(), 1, "{list}");
    assert!(
        list.contains("main"),
        "the message names the branch: {list}"
    );
    assert_eq!(
        flow.git(&["status", "--porcelain"]),
        "",
        "the stash took the changes, new files too"
    );
    flow.pick_stash();
    flow.shoot("13-stash-liste");

    flow.press(Key::Named(Named::Enter), shifted_command());
    assert!(
        flow.read("README.md").contains("Noch nicht fertig"),
        "applied"
    );
    assert!(flow.repo.join("docs/idee.md").exists());
    assert_eq!(
        flow.git(&["stash", "list"]).lines().count(),
        1,
        "apply keeps the stash"
    );

    flow.pick_stash();
    flow.press(Key::Named(Named::Backspace), Modifiers::COMMAND);
    assert!(matches!(
        flow.dialog(),
        Some(Dialog::Ask(Question::DropStash { .. }))
    ));
    flow.shoot("14-stash-verwerfen-nachfrage");
    flow.press(Key::Named(Named::Enter), Modifiers::empty());
    assert_eq!(flow.git(&["stash", "list"]), "");
    assert_eq!(flow.dialog(), None);
}

#[test]
fn the_command_list_runs_a_git_action_of_the_tab() {
    let mut flow = start("git-palette");
    colleague_pushes(&flow.clone, "docs/faq.md", "Neue Frage", "Add a question");
    let before = flow.git(&["rev-parse", "origin/main"]);
    let pushed = run_git(&flow.clone, 92, 0, &["rev-parse", "HEAD"]);
    assert_ne!(before, pushed);

    flow.press(Key::Character("p".into()), shifted_command());
    assert!(
        flow.session.app.command_palette_for_test().is_some(),
        "the command list is open"
    );

    flow.session.send(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged("Holen".to_owned()),
    ));
    flow.shoot("15-befehlsliste-mit-git");
    flow.session.press(Key::Named(Named::Enter));
    assert!(
        flow.session.app.command_palette_for_test().is_none(),
        "the list closed"
    );
    assert!(flow.view().actions.busy.is_none());
    assert_eq!(flow.git(&["rev-parse", "origin/main"]), pushed, "fetch ran");
    assert!(matches!(
        flow.session.app.shown_tab_for_test(),
        ShownTab::Git(_)
    ));
}
