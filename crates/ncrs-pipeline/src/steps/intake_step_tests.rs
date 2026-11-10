#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use ncrs_index::Index;
use ncrs_intake::{Location, LocationMode, TextChain, STABLE_AFTER};
use tempfile::TempDir;

use super::{intake_steps, OwnDetails};
use crate::step::{IndexAccess, Item, StepOutcome};

#[derive(Debug)]
struct CountedIndex {
    index: Mutex<Index>,
    reached: AtomicUsize,
}

impl IndexAccess for CountedIndex {
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
        self.reached.fetch_add(1, Ordering::SeqCst);
        work(&mut self.index.lock().unwrap());
        true
    }
}

#[test]
fn the_index_is_free_for_others_while_the_intake_step_hashes_a_file() {
    let dir = TempDir::new().unwrap();
    let inbox = dir.path().join("inbox");
    fs::create_dir(&inbox).unwrap();
    let location = Location::new(&inbox, LocationMode::Inbox, false).unwrap();
    let big = location.path.join("big.pdf");
    fs::write(&big, vec![7; 96 * 1024 * 1024]).unwrap();
    let access = Arc::new(CountedIndex {
        index: Mutex::new(Index::create_plain(&dir.path().join("index.db")).unwrap()),
        reached: AtomicUsize::new(0),
    });
    let step = intake_steps(
        Arc::clone(&access) as Arc<dyn IndexAccess>,
        Arc::new(TextChain::new()),
        None,
        OwnDetails::default(),
    )
    .remove(0);
    let item = || Item {
        location: location.clone(),
        path: big.clone(),
        document: None,
        text: None,
        review: None,
        plan: None,
        force: false,
        stored_text_is_current: false,
    };
    let first_seen = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    assert_eq!(
        step.run(&mut item(), first_seen).unwrap(),
        StepOutcome::NotReady
    );
    let before = access.reached.load(Ordering::SeqCst);

    let listed_while_hashing = thread::scope(|scope| {
        let hashing = scope.spawn(|| step.run(&mut item(), first_seen + STABLE_AFTER * 2));
        while access.reached.load(Ordering::SeqCst) == before {
            thread::yield_now();
        }
        let mut listed = None;
        access.with_index(&mut |index| {
            listed = Some(index.document_by_path(&big).unwrap());
        });
        assert_eq!(hashing.join().unwrap().unwrap(), StepOutcome::Continue);
        listed.unwrap()
    });

    assert!(
        listed_while_hashing.is_none(),
        "the step held the index until the file was taken in"
    );
}
