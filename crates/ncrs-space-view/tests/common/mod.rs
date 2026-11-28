//! A scan without a disk: the entries are written down, the messages are fed by hand.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test support: each test file uses some of it; a failing unwrap is the signal"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ncrs_core::FolderSize;
use ncrs_i18n::Language;
use ncrs_space::decide::DecideKind;
use ncrs_space::{
    CatalogId, Entry, EntryKind, Group, Holder, RunLog, Scan, VolumeSpace, WorktreeSpace,
};
use ncrs_space_view::{open, update, Action, Ctx, Message, State};

pub const HOME: &str = "/Users/marc";
pub const GIB: u64 = 1024 * 1024 * 1024;

/// The clock of every test: a fixed moment, so "measured 2 min ago" does not move.
pub fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)
}

pub fn home() -> PathBuf {
    PathBuf::from(HOME)
}

pub fn ctx() -> Ctx {
    let mut ctx = Ctx::new(Language::English, home());
    ctx.now = now();
    ctx
}

fn size(bytes: u64) -> FolderSize {
    FolderSize {
        bytes,
        partial: false,
    }
}

pub fn entry(path: &str, group: Group, kind: EntryKind, bytes: u64) -> Entry {
    Entry {
        path: PathBuf::from(path),
        group,
        kind,
        size: size(bytes),
        modified: Some(now() - Duration::from_secs(5 * 86_400)),
        device: None,
        held_by: Vec::new(),
        facts: Default::default(),
    }
}

pub fn catalog(path: &str, id: CatalogId, bytes: u64) -> Entry {
    entry(path, Group::Safe, EntryKind::Catalog(id), bytes)
}

pub fn worktree(path: &str, branch: &str, bytes: u64) -> Entry {
    let space = WorktreeSpace {
        repo: PathBuf::from("/Users/marc/code/ncrs"),
        path: PathBuf::from(path),
        branch: branch.to_owned(),
        size: size(bytes),
        modified: Some(now() - Duration::from_secs(20 * 86_400)),
        ignored_files: Vec::new(),
    };
    entry(path, Group::Worktrees, EntryKind::Worktree(space), bytes)
}

pub fn decide(path: &str, bytes: u64) -> Entry {
    entry(
        path,
        Group::Decide,
        EntryKind::Decide(DecideKind::BigFolder),
        bytes,
    )
}

pub fn in_use(path: &str, id: CatalogId, bytes: u64, tool: &str) -> Entry {
    let mut held = entry(path, Group::InUse, EntryKind::Catalog(id), bytes);
    held.held_by = vec![Holder {
        pid: 4242,
        name: tool.to_owned(),
    }];
    held
}

pub fn scan_of(entries: Vec<Entry>, processes_known: bool) -> Arc<Scan> {
    Arc::new(Scan {
        volume: VolumeSpace {
            total: 926 * GIB,
            available: 78 * GIB,
        },
        entries,
        partial: false,
        processes_known,
        volume_name: "Macintosh HD".to_owned(),
        worktree_roots: Vec::new(),
        measured_at: now() - Duration::from_secs(120),
        took: Duration::from_millis(4_200),
    })
}

/// A handful of rows, one of each group.
pub fn small_scan() -> Arc<Scan> {
    scan_of(
        vec![
            catalog(
                "/Users/marc/.cargo-target",
                CatalogId::CargoTarget,
                40 * GIB,
            ),
            catalog("/Users/marc/.yarn/cache", CatalogId::Yarn, 6 * GIB),
            worktree("/Users/marc/code/ncrs/.wt/old-a", "feat/old-a", 11 * GIB),
            decide("/Users/marc/Downloads/Setup.dmg", 3 * GIB),
            decide("/Users/marc/Downloads/photos.zip", 2 * GIB),
            decide("/Users/marc/Movies", 30 * GIB),
            in_use("/Users/marc/.gradle", CatalogId::Gradle, 5 * GIB, "gradle"),
        ],
        true,
    )
}

const CACHES: [&str; 12] = [
    "ms-playwright",
    "go-build",
    "typescript",
    "node-gyp",
    "pypoetry",
    "CocoaPods",
    "org.swift.swiftpm",
    "Cypress",
    "puppeteer",
    "pre-commit",
    "uv",
    "mypy",
];

/// 70 rows: what Marc's drive looks like, so a list that is too slow or too long shows.
pub fn crowded_scan(processes_known: bool) -> Arc<Scan> {
    let mut entries = Vec::new();
    for (place, name) in CACHES.iter().enumerate() {
        entries.push(catalog(
            &format!("/Users/marc/Library/Caches/{name}"),
            CatalogId::AppCache(name),
            u64::div_euclid((place as u64 + 1) * GIB, 2),
        ));
    }
    for place in 0..24 {
        entries.push(catalog(
            &format!("/Users/marc/.cargo-target/ncrs-{place}"),
            CatalogId::CargoTarget,
            u64::div_euclid((place + 2) * GIB, 3),
        ));
    }
    for place in 0..22 {
        entries.push(worktree(
            &format!("/Users/marc/code/ncrs/.wt/branch-{place}"),
            &format!("feat/branch-{place}"),
            u64::div_euclid((place + 1) * GIB, 4),
        ));
    }
    for place in 0..7 {
        entries.push(decide(
            &format!("/Users/marc/Documents/archive-{place}"),
            (place + 3) * GIB,
        ));
    }
    entries.push(decide("/Users/marc/Downloads/Setup.dmg", 3 * GIB));
    entries.push(decide("/Users/marc/Downloads/photos.zip", 2 * GIB));
    entries.push(in_use(
        "/Users/marc/.gradle",
        CatalogId::Gradle,
        5 * GIB,
        "gradle",
    ));
    entries.push(in_use("/Users/marc/.npm", CatalogId::Npm, 2 * GIB, "node"));
    entries.push(in_use("/Users/marc/.bun", CatalogId::Bun, GIB, "bun"));
    scan_of(entries, processes_known)
}

/// A tab whose measure has answered with `scan`.
pub fn tab(scan: Arc<Scan>, ctx: &Ctx) -> (State, Vec<Action>) {
    let (mut state, _task) = open(ctx);
    let run = state.scan_run();
    let (_scan_task, actions) = update(
        &mut state,
        Message::Scanned {
            run,
            scan: Some(scan),
        },
        ctx,
    );
    (state, actions)
}

pub fn send(state: &mut State, message: Message, ctx: &Ctx) -> Vec<Action> {
    update(state, message, ctx).1
}

pub fn path(text: &str) -> PathBuf {
    Path::new(text).to_path_buf()
}

pub fn log_of(rows: Vec<ncrs_space::LogRow>) -> Arc<RunLog> {
    Arc::new(RunLog {
        started: now(),
        finished: now() + Duration::from_secs(3),
        rows,
    })
}
