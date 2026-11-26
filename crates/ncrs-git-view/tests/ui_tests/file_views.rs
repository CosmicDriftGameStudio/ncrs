//! The history and the blame of a file, its diff and its menu.

use super::*;
use ncrs_git::{Blame, BlameGroup, BlameLimit, BlameLine, FileHistory, FileHistoryEntry};
use ncrs_git_view::file_views::{BlameState, HistoryState};
use ncrs_git_view::state::DiffState;

const PATH: &str = "src/kassenbuch/abschluss.rs";
const OLD_PATH: &str = "src/buchhaltung/monatsabschluss.rs";
const COMMITS: usize = 60;

/// 60 commits; the file was moved by the one at `renamed_at`.
fn history(renamed_at: Option<usize>) -> HistoryState {
    let commits = (0..COMMITS).map(|number| {
        let moved = renamed_at == Some(number);
        let before = renamed_at.is_some_and(|row| number > row);
        FileHistoryEntry {
            commit: commit(number),
            path_at_commit: if before { OLD_PATH } else { PATH }.to_owned(),
            old_path: moved.then(|| OLD_PATH.to_owned()),
        }
    });
    HistoryState {
        path: PATH.to_owned(),
        log: Some(FileHistory {
            commits: commits.collect(),
            has_more: true,
        }),
        limit: COMMITS,
        cursor: renamed_at.unwrap_or(2),
        diff: Some(staged_diff()),
        scroll: 0.0,
    }
}

fn blame(at: Option<String>) -> BlameState {
    let source = [
        "//! Der Monatsabschluss des Kassenbuchs.",
        "",
        "use crate::buchung::{Buchung, Monat};",
        "",
        "pub fn monatsabschluss(monat: Monat) -> Abschluss {",
        "    let buchungen = buchungen_im(monat);",
        "    let summe = buchungen.iter().map(|buchung| buchung.betrag).sum();",
        "    Abschluss::neu(monat, summe)",
        "}",
        "",
    ];
    let group_lines = [1, 4, 2, 7, 3, 12, 1, 5];
    let mut found = Blame {
        lines: Vec::new(),
        groups: Vec::new(),
        refused: BlameLimit::None,
    };
    for number in 0..16 {
        let of = commit(number % 7);
        let first_line = found.lines.len() as u32 + 1;
        let len = group_lines.get(number % 8).copied().expect("a length");
        for line_no in first_line..first_line + len {
            found.lines.push(BlameLine {
                line_no,
                text: source
                    .get((line_no as usize - 1) % source.len())
                    .expect("a line")
                    .to_string(),
                group: number,
            });
        }
        found.groups.push(BlameGroup {
            first_line,
            len,
            sha: of.id,
            short: of.short,
            author: of.author,
            time: of.time,
            summary: of.summary,
            path_then: None,
        });
    }
    BlameState {
        path: PATH.to_owned(),
        at,
        blame: Some(found),
        cursor: 11,
        scroll: 0.0,
    }
}

fn file_diff() -> State {
    let mut state = repository();
    on_commit(&mut state);
    state.area = Area::Detail;
    state.file_cursor = 1;
    state.diff = Some(DiffState {
        subject: Subject::Commit(commit(0).id),
        files: state.files_in_detail().to_vec(),
        file: 1,
        diff: Some(staged_diff()),
        unified: false,
        ignore_whitespace: false,
        hunk: 0,
    });
    state
}

#[test]
fn diff_toolbar_picture() {
    picture(&file_diff(), "tests/snapshots/git-diff-toolbar-row2-cosmic");
}

#[test]
fn file_history_picture() {
    let mut state = repository();
    state.history = Some(history(None));
    picture(&state, "tests/snapshots/git-file-history-cosmic");
}

#[test]
fn renamed_file_history_picture() {
    let mut state = repository();
    state.history = Some(history(Some(4)));
    picture(&state, "tests/snapshots/git-file-history-renamed-cosmic");
}

#[test]
fn blame_picture() {
    let mut state = repository();
    on_commit(&mut state);
    state.commit_cursor = 1;
    state.blame = Some(blame(Some(commit(0).id)));
    picture(&state, "tests/snapshots/git-blame-cosmic");
}

#[test]
fn blame_in_the_sandbox_picture() {
    let mut state = repository();
    on_commit(&mut state);
    state.commit_cursor = 1;
    state.actions.sync = None;
    state.actions.local = None;
    state.actions.unavailable = Some(Unavailable::Sandbox);
    state.blame = Some(blame(None));
    picture(&state, "tests/snapshots/git-blame-sandbox-cosmic");
}

#[test]
fn file_menu_of_a_commit_picture() {
    let mut state = repository();
    on_commit(&mut state);
    state.commit_cursor = 1;
    state.area = Area::Detail;
    state.file_cursor = 1;
    let path = "src/export/mod.rs".to_owned();
    let at = Some(commit(0).id);
    state.actions.menu = Some(OpenMenu::new(MenuTarget::File { path, at }));
    picture(&state, "tests/snapshots/git-file-menu-commit-cosmic");
}

#[test]
fn file_menu_of_a_local_file_picture() {
    let mut state = repository();
    state.commit_cursor = 0;
    state.area = Area::Detail;
    let local = state.actions.local.as_mut().expect("local changes");
    let file = local.unstaged.get_mut(2).expect("a third file");
    file.untracked = true;
    let path = file.path.clone();
    local.selected = Some((ChangeSide::Unstaged, 2));
    state.actions.menu = Some(OpenMenu::new(MenuTarget::File { path, at: None }));
    picture(&state, "tests/snapshots/git-file-menu-local-cosmic");
}
