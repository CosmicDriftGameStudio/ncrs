use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_index::{Field, Index, ReviewReason};

use super::archive::{self, Inbox};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for, Store};
use crate::messages::{FilingStatusMessage, Message};
use crate::session_state::{SessionState, WindowSize};

pub struct World {
    pub root: PathBuf,
    pub dir: tempfile::TempDir,
    pub inbox_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub store: Store,
    pub inbox: Inbox,
}

pub const CONFIG_HEAD: &str = "version = 1\nlang = \"de\"\n[modules.filing]\nenabled = true\n\
[modules.filing.ai]\nenabled = true\nprovider = \"local\"\n";

impl World {
    pub fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("a170{label}-"))
            .tempdir_in(if cfg!(unix) {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            })
            .unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let inbox_dir = root.join("Eingang");
        let archive_dir = root.join("Ablage");
        std::fs::create_dir_all(&inbox_dir).unwrap();
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archive = archive::write_archive(&archive_dir);
        let inbox = archive::write_inbox(&inbox_dir, &archive_dir, &archive);
        let store = empty_store();
        let (mut index, _key) =
            Index::create_encrypted(&root.join("index.db"), store.as_ref()).unwrap();
        for spec in archive::rules() {
            index.create_rule(&spec).unwrap();
        }
        drop(index);
        std::fs::write(
            root.join("config.toml"),
            Self::archive_only_config(&archive_dir),
        )
        .unwrap();
        Self {
            root,
            dir,
            inbox_dir,
            archive_dir,
            store,
            inbox,
        }
    }

    /// The first start reads the archive alone, so that every inbox file meets
    /// an archive that is already indexed; [`World::add_inbox_to_config`] follows.
    fn archive_only_config(archive: &Path) -> String {
        format!(
            "{CONFIG_HEAD}[[modules.filing.locations]]\npath = {archive:?}\nmode = \"archive\"\ntarget = true\n"
        )
    }

    pub fn add_inbox_to_config(&self) {
        std::fs::write(
            self.root.join("config.toml"),
            Self::config(&self.inbox_dir, &self.archive_dir, false),
        )
        .unwrap();
    }

    pub fn config(inbox: &Path, archive: &Path, continuous: bool) -> String {
        format!(
            "{CONFIG_HEAD}[[modules.filing.locations]]\npath = {inbox:?}\nmode = \"inbox\"\n{}\
             [[modules.filing.locations]]\npath = {archive:?}\nmode = \"archive\"\ntarget = true\n",
            if continuous {
                "continuous = true\n"
            } else {
                ""
            }
        )
    }

    pub fn index(&self) -> Index {
        Index::open(&self.dir.path().join("index.db"), self.store.as_ref()).unwrap()
    }

    pub fn boot(&self, size: iced::Size) -> Session<impl Live> {
        let mut saved = SessionState::default();
        saved.left.path = Some(self.inbox_dir.to_string_lossy().into_owned());
        saved.right.path = Some(self.archive_dir.to_string_lossy().into_owned());
        saved.window = Some(WindowSize {
            width: size.width,
            height: size.height,
        });
        boot_with(&self.root, Arc::clone(&self.store), saved)
    }
}

/// Every file of the archive is in the index.
pub fn archive_indexed(world: &World) -> bool {
    let indexed = world
        .index()
        .documents_beneath(&world.archive_dir)
        .unwrap()
        .len();
    indexed >= super::drive::files_below(&world.archive_dir).len()
}

/// The archive document an inbox file is noted to repeat, while the file is
/// unchanged: what the intake records for an exact copy of a filed document.
pub fn copy_of(index: &Index, copy: &Path) -> Option<ncrs_index::DocumentId> {
    let meta = std::fs::metadata(copy).ok()?;
    let stamp = ncrs_index::FileStamp {
        size: meta.len(),
        mtime: meta.modified().ok()?,
    };
    index.filed_document_for_inbox_path(copy, stamp).unwrap()
}

/// The inbox copies of archive files that the index has noted as copies.
pub fn copies_recorded(world: &World) -> usize {
    let index = world.index();
    world
        .inbox
        .duplicates
        .iter()
        .filter(|(copy, _)| copy_of(&index, copy).is_some())
        .count()
}

pub fn files_in(folder: &Path) -> usize {
    std::fs::read_dir(folder).map_or(0, |entries| entries.count())
}

/// Pumps, with the once-a-second tick of the app, until `done` holds; `limit`
/// only ends a wait that would never end.
pub fn wait_until<P: Live>(
    session: &mut Session<P>,
    limit: Duration,
    mut done: impl FnMut(&mut Session<P>) -> bool,
) -> bool {
    let started = Instant::now();
    let mut last_tick = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    while started.elapsed() < limit {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            session.send(Message::FilingStatus(FilingStatusMessage::Tick));
        }
        if done(session) {
            return true;
        }
        pump_for(session, Duration::from_millis(100), |_| false);
    }
    done(session)
}

/// Waits until the pipeline has nothing left for two looks in a row and
/// `reached` holds as well: the quiet alone can come before it has started.
pub fn wait_quiet_and<P: Live>(
    session: &mut Session<P>,
    limit: Duration,
    mut reached: impl FnMut(&mut Session<P>) -> bool,
) -> bool {
    let mut quiet_looks = 0;
    wait_until(session, limit, |live| {
        let quiet = live.app.pipeline_is_quiet_for_test() && live.in_flight == 0;
        quiet_looks = if quiet { quiet_looks + 1 } else { 0 };
        quiet_looks >= 2 && reached(live)
    })
}

/// What happened to each group of the inbox: how many are still there and why.
pub fn describe(world: &World) {
    let index = world.index();
    let line = |label: &str, files: &[PathBuf]| {
        let mut left = 0;
        let mut why: Vec<(String, usize)> = Vec::new();
        for file in files {
            if file.exists() {
                left += 1;
                let reason = index
                    .document_by_path(file)
                    .unwrap()
                    .and_then(|document| index.open_review_of(document.id).unwrap())
                    .map_or_else(
                        || "no review".to_owned(),
                        |entry| format!("{:?}", entry.reason),
                    );
                match why.iter_mut().find(|(known, _)| *known == reason) {
                    Some((_, count)) => *count += 1,
                    None => why.push((reason, 1)),
                }
            }
        }
        let described: Vec<String> = why
            .iter()
            .map(|(reason, count)| format!("{reason} x{count}"))
            .collect();
        println!(
            "{label}: {} files, {left} left {}",
            files.len(),
            described.join(", ")
        );
    };
    let pairs =
        |pairs: &[(PathBuf, PathBuf)]| pairs.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>();
    line("sure", &pairs(&world.inbox.sure));
    line("duplicates", &pairs(&world.inbox.duplicates));
    line("clashes", &pairs(&world.inbox.clashes));
    line("no date", &world.inbox.no_date);
    line("no amount", &world.inbox.no_amount);
    line("unsure sender (miete)", &world.inbox.unsure_sender);
    line("no rule", &world.inbox.no_rule);
    line("unconfirmed", &world.inbox.unconfirmed);
}

/// The pipeline cannot find out by itself that it is unsure of a sender, so a
/// few documents of a known sender are given that question in the index.
pub fn mark_unsure_senders(world: &World) {
    let mut index = world.index();
    for path in &world.inbox.unsure_sender_marked {
        let document = index
            .document_by_path(path)
            .unwrap()
            .expect("a known document");
        let entry = index
            .open_review_of(document.id)
            .unwrap()
            .expect("a review to replace");
        index.resolve_review(entry.id).unwrap();
        index
            .add_review(document.id, ReviewReason::LowConfidence(Field::Sender))
            .unwrap();
    }
}

fn reason_of(index: &Index, path: &Path) -> Option<ReviewReason> {
    let document = index.document_by_path(path).unwrap()?;
    Some(index.open_review_of(document.id).unwrap()?.reason)
}

/// What the normal pipeline made of the inbox: the sure ones are filed, and
/// every kind of question has at least three documents.
pub fn check_phase_a(world: &World) {
    let index = world.index();
    for (from, target) in &world.inbox.sure {
        assert!(
            !from.exists() && target.is_file(),
            "sure but not filed: {from:?}"
        );
    }
    let expect = |label: &str, files: &[PathBuf], reason: &dyn Fn(&ReviewReason) -> bool| {
        assert!(files.len() >= 3, "{label}: fewer than three documents");
        for file in files {
            assert!(file.is_file(), "{label}: {file:?} is gone");
            let found = reason_of(&index, file);
            assert!(
                found.as_ref().is_some_and(reason),
                "{label}: {file:?} has review {found:?}"
            );
        }
    };
    let taken = |reason: &ReviewReason| *reason == ReviewReason::TargetExists;
    let paths = |pairs: &[(PathBuf, PathBuf)]| {
        pairs
            .iter()
            .map(|(from, _)| from.clone())
            .collect::<Vec<_>>()
    };
    // An exact copy of an indexed archive file is no question: it stays in the
    // inbox, with no review, and the index only notes it as a repeat of the
    // archive document.
    assert!(world.inbox.duplicates.len() >= 3);
    for (copy, original) in &world.inbox.duplicates {
        assert!(
            copy.is_file() && original.is_file(),
            "{copy:?}: a copy or its original is gone"
        );
        assert!(
            index.document_by_path(copy).unwrap().is_none(),
            "{copy:?} became a document"
        );
        let holder =
            copy_of(&index, copy).unwrap_or_else(|| panic!("{copy:?} is not noted as a copy"));
        let original_document = index
            .document_by_path(original)
            .unwrap()
            .expect("the archive file is known");
        assert_eq!(
            holder, original_document.id,
            "{copy:?} is a copy of another document"
        );
        assert!(index.open_review_of(holder).unwrap().is_none());
    }
    expect("clashes", &paths(&world.inbox.clashes), &taken);
    expect("no date", &world.inbox.no_date, &|reason| {
        *reason == ReviewReason::MissingField(Field::Date)
    });
    expect("no amount", &world.inbox.no_amount, &|reason| {
        *reason == ReviewReason::MissingField(Field::Amount)
    });
    expect("no rule", &world.inbox.no_rule, &|reason| {
        *reason == ReviewReason::NoMatchingRule
    });
    expect(
        "no rule, known neighbours",
        &world.inbox.unsure_sender,
        &|reason| *reason == ReviewReason::NoMatchingRule,
    );
    expect("unconfirmed", &world.inbox.unconfirmed, &|reason| {
        *reason == ReviewReason::UnconfirmedRule
    });
    expect(
        "unsure sender",
        &world.inbox.unsure_sender_marked,
        &|reason| *reason == ReviewReason::LowConfidence(Field::Sender),
    );
}
