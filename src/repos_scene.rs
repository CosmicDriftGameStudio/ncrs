//! The Repos tab for the screenshot generators: a list of made-up repositories
//! whose states are fed in as reads, so nothing on this computer is shown.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ncrs_git::{RepoSummary, WorktreeSummary};
use ncrs_i18n::Language;
use ncrs_repos::{Ctx, Message, Origin, Reading, State, Store};

const ROOT: &str = "/Users/alex/code";
const FAVOURITES: usize = 3;
const FEATURE_BRANCH: &str = "feature/rebindable-keys";

/// Name, commits behind, commits ahead, changed files, hours since the last fetch.
const REPOS: [(&str, u32, u32, usize, u64); 22] = [
    ("ncrs", 0, 3, 4, 2),
    ("ncrs-site", 2, 0, 0, 3),
    ("weather-api", 0, 0, 0, 5),
    ("design-tokens", 5, 0, 0, 7),
    ("chess-engine", 0, 0, 2, 4),
    ("cloud-setup", 1, 0, 0, 9),
    ("ops-notes", 0, 0, 0, 26),
    ("pixel-game", 0, 2, 0, 30),
    ("studio-site", 3, 0, 1, 48),
    ("shell-config", 0, 0, 0, 12),
    ("dotfiles", 0, 1, 0, 52),
    ("homebrew-tap", 0, 0, 0, 72),
    ("photo-sorter", 0, 0, 7, 6),
    ("invoice-parser", 4, 0, 0, 14),
    ("iced-playground", 0, 0, 0, 96),
    ("terraform-modules", 0, 0, 0, 20),
    ("k8s-manifests", 2, 0, 0, 22),
    ("runbooks", 0, 0, 0, 80),
    ("blog", 0, 0, 3, 120),
    ("fonts", 0, 0, 0, 200),
    ("scan-archive", 0, 0, 0, 150),
    ("recipes", 0, 4, 0, 170),
];

pub fn repo(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

fn summary(
    &(name, behind, ahead, changed, hours): &(&str, u32, u32, usize, u64),
    index: usize,
) -> RepoSummary {
    let mut summary = RepoSummary {
        branch: Some("main".to_owned()),
        default_branch: Some("main".to_owned()),
        behind,
        ahead,
        has_upstream: true,
        changed,
        fetched_at: SystemTime::now().checked_sub(Duration::from_secs(hours * 3_600)),
        ..RepoSummary::default()
    };
    if index.is_multiple_of(6) && name != "ncrs" {
        summary.branch = Some(FEATURE_BRANCH.to_owned());
    }
    if name == "ncrs" {
        summary.worktrees.push(WorktreeSummary {
            path: repo("ncrs-1-8-0"),
            branch: Some("feat/1-8-0".to_owned()),
        });
    }
    summary
}

/// The tab as it looks after the list was read: the first three are favourites.
pub fn repos_tab(lang: Language) -> State {
    let mut store = Store::default();
    for (index, (name, ..)) in REPOS.iter().enumerate() {
        store.add(repo(name), Origin::Found, Some(PathBuf::from(ROOT)));
        if index < FAVOURITES {
            store.toggle_favourite(&repo(name));
        }
    }
    let mut ctx = Ctx::new(lang);
    ctx.now = SystemTime::now();
    ctx.home = Some(PathBuf::from("/Users/alex"));
    let mut state = State::new(store, None);
    for (index, row) in REPOS.iter().enumerate() {
        let read = Reading::Read(Box::new(summary(row, index)));
        drop(ncrs_repos::update(
            &mut state,
            Message::SummaryRead(repo(row.0), Box::new(read)),
            &ctx,
        ));
    }
    state
}
