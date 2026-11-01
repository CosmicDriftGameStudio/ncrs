use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

/// Real directories, because the conflict only exists if the target file is
/// really there. The scratch directory removes itself.
fn with_conflict() -> (App, tempfile::TempDir) {
    let scratch = tempfile::Builder::new()
        .prefix("ncrs-conflict-")
        .tempdir()
        .expect("a scratch directory");
    let left = scratch.path().join("left");
    let right = scratch.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();

    for i in 0..3 {
        std::fs::write(left.join(format!("f{i}.txt")), b"new").unwrap();
    }
    // The name the transfer will run into.
    std::fs::write(right.join("f0.txt"), b"existing").unwrap();

    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = left.clone();
    app.panels.right.path = right.clone();
    app.panels.left.entries = (0..3)
        .map(|i| FileEntry {
            name: OsString::from(format!("f{i}.txt")),
            path: left.join(format!("f{i}.txt")),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 3,
            modified: None,
            folder_size: None,
        })
        .collect();
    app.panels.left.selected = 0;
    (app, scratch)
}

/// A name that is taken opens the dialog rather than failing the row. The
/// dialog is the feature: the user gets asked instead of the copy stopping.
///
/// The failure is built the way the operating system builds it and wrapped
/// the way the app wraps it, so this exercises the classification and not a
/// string that happens to look right. `e2e_tests.rs` covers the same ground
/// with a real copy against a real file; this one is here because it also
/// covers the "for all" rule, which needs several conflicts in a row.
fn deliver_conflict(app: &mut App) {
    let _task = app.update(Message::TransferRowDone {
        result: Err(RowFailure::from_io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "alpha.txt already exists",
        ))),
        index: 0,
        generation: app.ops.generation,
    });
}

#[tokio::test]
async fn a_taken_name_asks_instead_of_failing() {
    let (mut app, _scratch) = with_conflict();

    let _started = app.start_transfer(TransferKind::Copy);
    deliver_conflict(&mut app);

    let pending = app
        .ops
        .pending_conflict
        .as_ref()
        .expect("a taken name should open the conflict dialog");
    assert_eq!(pending.index, 0, "it asked about the wrong file");
}

/// Answering "overwrite for all" remembers the rule, so the next conflict is
/// resolved without a second dialog. This is the whole point of the "for all"
/// button: fifty files, one question.
#[tokio::test]
async fn an_all_answer_is_remembered() {
    let (mut app, _scratch) = with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    deliver_conflict(&mut app);
    assert!(
        app.ops.pending_conflict.is_some(),
        "the dialog did not open"
    );

    let _resumed =
        app.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, ConflictChoice::AllOverwrite, ctx));

    assert_eq!(
        app.ops.conflict_rule,
        Some(fs::transfer::OnConflict::Overwrite),
        "the answer was not remembered for the remaining files"
    );
}

/// A single-file answer does not become a standing rule — the next conflict
/// asks again.
#[tokio::test]
async fn a_single_answer_is_not_remembered() {
    let (mut app, _scratch) = with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    deliver_conflict(&mut app);

    let _resumed =
        app.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, ConflictChoice::ThisOverwrite, ctx));

    assert_eq!(
        app.ops.conflict_rule, None,
        "one file's answer was applied to the rest"
    );
}

/// Cancel ends the transfer: no dialog, nothing left to resume, and the
/// "for all" rule of that transfer does not leak into the next one.
#[tokio::test]
async fn cancel_ends_the_transfer() {
    let (mut app, _scratch) = with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    deliver_conflict(&mut app);
    app.ops.conflict_rule = Some(fs::transfer::OnConflict::Skip);

    let _task =
        app.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, ConflictChoice::Cancel, ctx));

    assert!(app.ops.pending_conflict.is_none(), "the dialog is still up");
    assert!(app.ops.transfer.is_none(), "the transfer is still running");
    assert_eq!(
        app.ops.conflict_rule, None,
        "the rule outlived the transfer"
    );
}

/// "Keep all" is a standing rule to skip, not to ask again or to fail.
#[tokio::test]
async fn keep_all_skips_instead_of_asking_again() {
    let (mut app, _scratch) = with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    deliver_conflict(&mut app);

    let _task =
        app.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, ConflictChoice::AllKeep, ctx));

    assert!(app.ops.pending_conflict.is_none());
    assert_eq!(app.ops.conflict_rule, Some(fs::transfer::OnConflict::Skip));
}

/// A rule from an earlier transfer must not answer the next transfer's
/// conflicts without asking.
#[tokio::test]
async fn a_new_transfer_forgets_the_old_rule() {
    let (mut app, _scratch) = with_conflict();
    app.ops.conflict_rule = Some(fs::transfer::OnConflict::Overwrite);

    let _started = app.start_transfer(TransferKind::Copy);

    assert_eq!(app.ops.conflict_rule, None);
}

#[tokio::test]
async fn a_refused_7z_copy_shows_the_text_of_the_language() {
    use crate::fs::archive::fixtures::{DICTIONARY_512M_7Z, ENCRYPTED_7Z};
    let dir = tempfile::tempdir().unwrap();
    let (file, bytes) = ENCRYPTED_7Z[0];
    let encrypted = dir.path().join(file);
    std::fs::write(&encrypted, bytes).unwrap();
    let (huge_file, huge_bytes) = DICTIONARY_512M_7Z[1];
    let huge = dir.path().join(huge_file);
    std::fs::write(&huge, huge_bytes).unwrap();
    let cases = [
        (encrypted.join("secret.txt"), Msg::ErrorArchiveEncrypted),
        (huge.join("a.txt"), Msg::ErrorArchiveDictionaryTooLarge),
    ];
    for lang in [Language::English, Language::German] {
        for (source, expected) in &cases {
            let error = fs::archive::copy_out(
                source,
                dir.path(),
                &mut fs::transfer::Counting,
                fs::transfer::OnConflict::Fail,
            )
            .unwrap_err();
            let (mut app, _scratch) = with_conflict();
            app.lang = lang;
            let _started = app.start_transfer(TransferKind::Copy);

            let _task = app.update(Message::TransferRowDone {
                result: Err(RowFailure::from_io(error)),
                index: 0,
                generation: app.ops.generation,
            });

            assert_eq!(app.job_error.as_deref(), Some(lang.text(*expected)));
        }
    }
}

/// A late result from a stopped transfer must not steer the next one: here
/// it would otherwise end the new transfer with an error.
#[tokio::test]
async fn a_late_result_of_a_stopped_transfer_is_ignored() {
    let (mut app, _scratch) = with_conflict();
    let _first = app.start_transfer(TransferKind::Copy);
    let stale = app.ops.generation;
    let _stopped = app.update(Message::AbortJob);
    let _second = app.start_transfer(TransferKind::Copy);
    assert!(app.jobs.is_busy());

    let _task = app.update(Message::TransferRowDone {
        result: Err(RowFailure::Other("the app is gone".to_string())),
        index: 0,
        generation: stale,
    });

    assert!(
        app.ops.transfer.is_some(),
        "the old result ended the new transfer"
    );
    assert!(
        app.jobs.is_busy(),
        "the old result freed the new job's slot"
    );
}

/// And the rule goes when the last row is done.
#[tokio::test]
async fn a_finished_transfer_forgets_its_rule() {
    let (mut app, _scratch) = with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    app.ops.conflict_rule = Some(fs::transfer::OnConflict::Overwrite);
    let transfer = app.ops.transfer.clone().expect("a running transfer");

    let _task = app.update(Message::TransferRowDone {
        result: Ok(transfer),
        index: 0,
        generation: app.ops.generation,
    });

    assert!(app.ops.transfer.is_none());
    assert_eq!(app.ops.conflict_rule, None);
}

/// The conflict is recognised from the error kind, not from the wording.
/// A wording change in the filesystem layer would otherwise turn every
/// conflict into a silent failure.
#[test]
fn a_conflict_is_recognised_by_its_kind() {
    // Built the way the operating system builds them, and matched the way
    // the app matches them: on the kind, not on the wording. The old test
    // fed these strings to a `starts_with` that could never be true, which
    // is why it passed while the dialog never appeared.
    let conflict = std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "/tmp/dst/alpha.txt already exists",
    );
    assert_eq!(
        RowFailure::from_io(conflict),
        RowFailure::Conflict("/tmp/dst/alpha.txt already exists".to_string())
    );
    let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
    assert!(matches!(RowFailure::from_io(denied), RowFailure::Other(_)));
}
