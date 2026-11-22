//! Flow 3, the exact copies and the places that are taken by other content.
//! A copy of an archive file is the question "Liegt schon in der Ablage":
//! delete one, ignore one, delete the rest with the group button. Then the
//! places taken by other content: overwrite, keep both, file where the place
//! became free, ignore, and the group buttons. A taken place whose file has
//! other content never offers "Doppelte löschen".

use std::path::{Path, PathBuf};
use std::time::Duration;

use ncrs_hermes_ui::reviews_page::{Answer, Question, TakenBy};

use super::archive;
use super::drive;
use super::probe::{wait_until, World};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;

const TAKEN: Question = Question::TargetTaken;
const REPEATS: Question = Question::AlreadyFiled;

/// The identical scans that arrived while the folder filed by itself.
fn identical_scans(world: &World) -> Vec<PathBuf> {
    let mut scans: Vec<PathBuf> = drive::files_below(&world.inbox_dir)
        .into_iter()
        .filter(|path| path.to_string_lossy().contains("Scan nochmal"))
        .collect();
    scans.sort();
    scans
}

fn already_filed<P: Live>(world: &World, session: &mut Session<P>) {
    let scans = identical_scans(world);
    assert!(scans.len() >= 3, "the identical scans are gone: {scans:?}");
    let mut copies: Vec<PathBuf> = world
        .inbox
        .duplicates
        .iter()
        .map(|(copy, _)| copy.clone())
        .chain(scans.iter().cloned())
        .collect();
    copies.sort();

    let mut asked = drive::already_filed(session);
    asked.sort();
    let asked_paths: Vec<PathBuf> = asked.iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(asked_paths, copies, "the group does not hold the copies");
    for (copy, keeper) in &asked {
        assert!(keeper.starts_with(&world.archive_dir), "{keeper:?}");
        assert_eq!(drive::text_of(copy), drive::text_of(keeper));
    }
    let count = copies.len();
    assert_eq!(
        drive::page(session).group_answers(REPEATS),
        [(Answer::DeleteDuplicate, count), (Answer::Exclude, count)]
    );
    drive::unfold(session, REPEATS);
    drive::shot(session, "reviews-already-filed");

    // One row: the copy goes to the trash through the journaled run.
    let (deleted, keeper) = asked
        .iter()
        .find(|(path, _)| *path == world.inbox.duplicates[0].0)
        .cloned()
        .expect("the first copy is asked about");
    let text = drive::text_of(&deleted);
    drive::answer_repeat(session, &deleted, Answer::DeleteDuplicate);
    assert!(!deleted.exists(), "{deleted:?} is still in the inbox");
    assert_eq!(drive::text_of(&keeper), text, "the archive file changed");
    assert!(
        drive::trash_holds(world, &text),
        "the copy is not in the trash"
    );
    assert!(
        drive::batch_names(world)
            .iter()
            .flatten()
            .any(|name| name == "Remove duplicates"),
        "no run in the activity: {:?}",
        drive::batch_names(world)
    );
    assert!(drive::journal_moved(world, &deleted).is_some());
    assert!(!is_asked(session, &deleted));

    // One copy that arrived by itself is left alone: it stays, the question
    // is gone and a new reading does not ask again.
    let ignored = scans[0].clone();
    let kept_text = drive::text_of(&ignored);
    drive::answer_repeat(session, &ignored, Answer::Exclude);
    assert_eq!(drive::text_of(&ignored), kept_text);
    assert!(world.index().is_excluded(&ignored).unwrap());
    assert!(!is_asked(session, &ignored));
    drive::reload(session);
    assert!(
        !is_asked(session, &ignored),
        "an ignored copy is asked again"
    );

    // The rest with one button.
    let rest: Vec<PathBuf> = copies
        .iter()
        .filter(|copy| **copy != deleted && **copy != ignored)
        .cloned()
        .collect();
    let handled = drive::answer_group(session, REPEATS, Answer::DeleteDuplicate);
    assert_eq!(handled, rest.len());
    for copy in &rest {
        assert!(!copy.exists(), "{copy:?} is still in the inbox");
    }
    assert!(ignored.is_file(), "the ignored copy was touched");
    for (_, original) in &world.inbox.duplicates {
        assert!(original.is_file(), "{original:?} was touched");
    }
    assert!(
        !drive::page(session)
            .questions()
            .any(|question| question == REPEATS),
        "the group is still there"
    );
    drive::config_intact(world);
}

fn is_asked<P: Live>(session: &Session<P>, path: &Path) -> bool {
    drive::already_filed(session)
        .iter()
        .any(|(asked, _)| asked == path)
}

pub fn run<P: Live>(world: &World, session: &mut Session<P>) {
    drive::show_reviews(session);
    already_filed(world, session);
    let clashes = world.inbox.clashes.clone();
    assert!(clashes.len() >= 6);

    for (incoming, target) in &clashes {
        let (_, item) = drive::item(session, incoming);
        let taken = drive::page(session).taken_by(item.document_id);
        let places: Vec<_> = drive::page(session)
            .taken_to_compare()
            .into_iter()
            .filter(|(document, _, _)| *document == item.document_id)
            .collect();
        assert_eq!(
            taken,
            TakenBy::OtherContent,
            "{incoming:?} vs {target:?}, places {places:?}, target exists {}",
            target.exists()
        );
    }
    let offered = drive::page(session).group_answers(TAKEN);
    let count_of = |answer| {
        offered
            .iter()
            .find(|(known, _)| *known == answer)
            .map(|(_, n)| *n)
    };
    assert_eq!(
        count_of(Answer::DeleteDuplicate),
        None,
        "no review holds the same content"
    );
    assert_eq!(count_of(Answer::Overwrite), Some(clashes.len()));
    assert_eq!(count_of(Answer::KeepBoth), Some(clashes.len()));
    assert_eq!(count_of(Answer::Accept), None, "nothing is free yet");
    assert_eq!(
        count_of(Answer::Exclude),
        Some(clashes.len()),
        "ignoring is always offered"
    );

    overwrite(world, session, &clashes[0]);
    keep_both(world, session, &clashes[1]);
    ignore(world, session, &clashes[2]);
    file_into_free_place(world, session, &clashes[3]);

    // The rest of the clashes with one button: nothing but the ignored file
    // is left of the question.
    let rest: Vec<&(PathBuf, PathBuf)> = clashes[4..].iter().collect();
    drive::unfold(session, TAKEN);
    drive::shot(session, "reviews-taken-before-group");
    let handled = drive::answer_group(session, TAKEN, Answer::KeepBoth);
    assert_eq!(handled, rest.len());
    for (incoming, target) in rest {
        assert!(!incoming.exists(), "{incoming:?} was not filed");
        assert!(target.is_file(), "the old file at {target:?} was touched");
    }
    drive::shot(session, "reviews-after-group");
    drive::config_intact(world);
}

fn overwrite<P: Live>(
    world: &World,
    session: &mut Session<P>,
    (incoming, target): &(PathBuf, PathBuf),
) {
    let (old, new) = (drive::text_of(target), drive::text_of(incoming));
    assert_ne!(old, new);
    drive::answer_file(session, incoming, Answer::Overwrite);
    assert!(!incoming.exists());
    assert_eq!(
        drive::text_of(target),
        new,
        "the new file did not take the place"
    );
    assert!(
        drive::trash_holds(world, &old),
        "the old file is not in the trash"
    );
    assert!(
        drive::batch_names(world)
            .iter()
            .flatten()
            .any(|name| name == "Replace files"),
        "no journal run for the replaced file: {:?}",
        drive::batch_names(world)
    );
    assert!(!drive::has_open_review(world, incoming));
}

fn keep_both<P: Live>(
    world: &World,
    session: &mut Session<P>,
    (incoming, target): &(PathBuf, PathBuf),
) {
    let (old, new) = (drive::text_of(target), drive::text_of(incoming));
    let folder = target.parent().unwrap();
    let before = drive::files_below(folder).len();
    drive::answer_file(session, incoming, Answer::KeepBoth);
    assert!(!incoming.exists());
    assert_eq!(drive::text_of(target), old, "the old file changed");
    let files = drive::files_below(folder);
    assert_eq!(
        files.len(),
        before + 1,
        "no second file next to the old one"
    );
    let other: Vec<&PathBuf> = files
        .iter()
        .filter(|path| path.as_path() != target && drive::text_of(path) == new)
        .collect();
    assert_eq!(other.len(), 1, "the new file is not next to the old one");
    assert!(
        other[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(2)"),
        "{other:?} has no number"
    );
    assert!(!drive::has_open_review(world, incoming));
}

fn ignore<P: Live>(
    world: &World,
    session: &mut Session<P>,
    (incoming, target): &(PathBuf, PathBuf),
) {
    let (text, old) = (drive::text_of(incoming), drive::text_of(target));
    drive::answer_file(session, incoming, Answer::Exclude);
    assert_eq!(
        drive::text_of(incoming),
        text,
        "the ignored file was touched"
    );
    assert_eq!(drive::text_of(target), old);
    assert!(
        !drive::has_open_review(world, incoming),
        "an ignored file keeps its review"
    );
    assert!(
        world.index().is_excluded(incoming).unwrap(),
        "the file is not left alone for good"
    );

    // Another run of the continuous filing must not pick it up.
    let (arrival, landed) = archive::write_rule_arrival(&world.inbox_dir, &world.archive_dir, 8900);
    assert!(
        wait_until(session, Duration::from_secs(120), |_| !arrival.exists()
            && landed.is_file()),
        "the continuous filing did not file {arrival:?}"
    );
    assert_eq!(
        drive::text_of(incoming),
        text,
        "a later run moved the ignored file"
    );
    assert!(!drive::has_open_review(world, incoming));
    drive::config_intact(world);
}

/// The place becomes free while the page is open; the page reads again and
/// offers "Ablegen".
fn file_into_free_place<P: Live>(
    world: &World,
    session: &mut Session<P>,
    (incoming, target): &(PathBuf, PathBuf),
) {
    let text = drive::text_of(incoming);
    std::fs::remove_file(target).unwrap();
    drive::reload(session);
    let (_, item) = drive::item(session, incoming);
    assert_eq!(
        drive::page(session).taken_by(item.document_id),
        TakenBy::Free
    );
    drive::answer_file(session, incoming, Answer::Accept);
    assert!(!incoming.exists());
    assert_eq!(
        drive::text_of(target),
        text,
        "the file did not land in the free place"
    );
    assert!(!drive::has_open_review(world, incoming));
}
