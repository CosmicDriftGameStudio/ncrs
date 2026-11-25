use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_index::Index;

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
            Self::target_config(&archive_dir),
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
    /// an archive that is already indexed; [`World::add_source_to_config`] follows.
    fn target_config(archive: &Path) -> String {
        format!("{CONFIG_HEAD}[[modules.filing.targets]]\nid = \"main\"\npath = {archive:?}\n")
    }

    pub fn add_source_to_config(&self) {
        let config = format!(
            "{}[[modules.filing.sources]]\npath = {:?}\ntargets = [\"main\"]\n\
             when = \"watch\"\nfiles = \"move\"\n",
            Self::target_config(&self.archive_dir),
            self.inbox_dir
        );
        std::fs::write(self.root.join("config.toml"), config).unwrap();
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

/// Every inbox file is a document or noted as a copy of one.
pub fn inbox_read(world: &World) -> bool {
    let documents = world
        .index()
        .documents_beneath(&world.inbox_dir)
        .unwrap()
        .len();
    documents + copies_recorded(world) >= files_in(&world.inbox_dir)
        && copies_recorded(world) == world.inbox.duplicates.len()
}

pub fn backlog_recorded(world: &World) -> bool {
    world
        .index()
        .has_source_backlog(&world.inbox_dir)
        .unwrap()
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

/// How far the reading of the inbox got.
pub fn describe(world: &World) {
    println!(
        "inbox: {} files, {} documents, {} copies noted, backlog recorded: {}",
        files_in(&world.inbox_dir),
        world
            .index()
            .documents_beneath(&world.inbox_dir)
            .unwrap()
            .len(),
        copies_recorded(world),
        backlog_recorded(world)
    );
}

/// What lay in the inbox when the source was first watched stays there, without
/// a question, and an exact copy of an indexed archive file is no document: the
/// index notes it as a repeat of the archive document.
pub fn check_phase_a(world: &World) {
    let index = world.index();
    let inbox = &world.inbox;
    let pairs = inbox
        .sure
        .iter()
        .chain(&inbox.duplicates)
        .chain(&inbox.clashes)
        .map(|(file, _)| file);
    let singles = inbox
        .no_date
        .iter()
        .chain(&inbox.known_sender)
        .chain(&inbox.no_rule)
        .chain(&inbox.unconfirmed);
    for file in pairs.chain(singles) {
        assert!(file.is_file(), "{file:?} left the inbox");
    }
    for (_, target) in &inbox.sure {
        assert!(!target.exists(), "{target:?} was filed from the backlog");
    }
    assert!(index.recent_batches(5).unwrap().is_empty());
    assert!(index.open_reviews().unwrap().is_empty());

    assert!(inbox.duplicates.len() >= 3);
    for (copy, original) in &inbox.duplicates {
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
    }
}
