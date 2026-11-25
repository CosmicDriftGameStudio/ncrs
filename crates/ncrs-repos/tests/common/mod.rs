//! A tab with repositories that need no folder on disk: the list is written to
//! a `repos.toml`, and the states are injected as reads.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test support: each test file uses some of it; a failing unwrap is the signal"
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use iced::Task;
use ncrs_git::{RepoSummary, WorktreeSummary};
use ncrs_i18n::Language;
use ncrs_repos::{open, update, Action, Ctx, Message, Origin, Reading, State, Store};

pub const ROOT: &str = "/Users/marc/code";
pub const HOME: &str = "/Users/marc";
/// The clock of every test: a fixed moment, so "fetched 3 hours ago" does not move.
pub fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)
}

pub fn repo(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

pub fn summary(behind: u32, ahead: u32, changed: usize) -> RepoSummary {
    RepoSummary {
        branch: Some("main".to_owned()),
        default_branch: Some("main".to_owned()),
        behind,
        ahead,
        changed,
        fetched_at: Some(now() - Duration::from_secs(3 * 3_600)),
        ..RepoSummary::default()
    }
}

pub fn with_worktree(mut base: RepoSummary, at: &str, branch: &str) -> RepoSummary {
    base.worktrees.push(WorktreeSummary {
        path: repo(at),
        branch: Some(branch.to_owned()),
    });
    base
}

pub fn read(summary: RepoSummary) -> Reading {
    Reading::Read(Box::new(summary))
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub ctx: Ctx,
    pub state: State,
}

/// `names` as found repos under [`ROOT`]; the first `favourites` are favourites.
pub fn fixture(names: &[&str], favourites: usize) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("repos.toml");
    let mut store = Store::default();
    for (index, name) in names.iter().enumerate() {
        store.add(repo(name), Origin::Found, Some(PathBuf::from(ROOT)));
        if index < favourites {
            store.toggle_favourite(&repo(name));
        }
    }
    store.write(&file).unwrap();
    let mut ctx = Ctx::new(Language::English);
    ctx.file = Some(file);
    ctx.now = now();
    ctx.home = Some(PathBuf::from(HOME));
    ctx.settings.learn_from_panel = true;
    ctx.settings.concurrency = 2;
    let (mut state, _task) = open(&ctx);
    for name in names {
        let _ = update(
            &mut state,
            Message::SummaryRead(repo(name), Box::new(read(summary(0, 0, 0)))),
            &ctx,
        );
    }
    Fixture { dir, ctx, state }
}

impl Fixture {
    pub fn send(&mut self, message: Message) -> Vec<Action> {
        let (_task, actions): (Task<Message>, Vec<Action>) =
            update(&mut self.state, message, &self.ctx);
        actions
    }

    pub fn reading(&mut self, name: &str, reading: Reading) {
        self.send(Message::SummaryRead(repo(name), Box::new(reading)));
    }
}
