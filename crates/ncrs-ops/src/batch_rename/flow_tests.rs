#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_ai::{NameBlock, NameSpec};
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::KnownHostsFiles;
use ncrs_index::{Amount, Currency};

use super::flow::{amount_text, pattern_of, run_steps};
use super::pattern::{plan, NameRules, Step};
use super::state::ApplyFailure;

const LOCAL: NameRules = NameRules {
    windows: false,
    fold_case: false,
};

fn never() -> bool {
    false
}

fn pool() -> Arc<Pool> {
    Arc::new(Pool::over_tcp(KnownHostsFiles::default()))
}

fn pairs(names: &[(&str, &str)]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|(old, new)| ((*old).to_owned(), (*new).to_owned()))
        .collect()
}

fn names_in(folder: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn touch(folder: &std::path::Path, names: &[&str]) {
    for name in names {
        std::fs::write(folder.join(name), name).unwrap();
    }
}

#[tokio::test]
async fn a_swap_goes_through_a_temporary_name() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["a.txt", "b.txt"]);
    let steps = plan(
        &pairs(&[("a.txt", "b.txt"), ("b.txt", "a.txt")]),
        &["a.txt".to_owned(), "b.txt".to_owned()],
        LOCAL,
    );

    run_steps(pool(), folder.path().to_path_buf(), steps, None, never)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(folder.path().join("a.txt")).unwrap(),
        "b.txt"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("b.txt")).unwrap(),
        "a.txt"
    );
    assert_eq!(names_in(folder.path()), ["a.txt", "b.txt"]);
}

#[tokio::test]
async fn a_failure_puts_every_earlier_rename_back() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["a.txt", "b.txt"]);
    let steps = vec![
        Step {
            from: "a.txt".to_owned(),
            to: "x.txt".to_owned(),
        },
        Step {
            from: "missing.txt".to_owned(),
            to: "y.txt".to_owned(),
        },
    ];

    let failure = run_steps(pool(), folder.path().to_path_buf(), steps, None, never)
        .await
        .unwrap_err();

    assert!(matches!(
        &failure,
        ApplyFailure::Failed {
            name,
            rollback_failed: false,
            ..
        } if name.as_str() == "missing.txt"
    ));
    assert_eq!(names_in(folder.path()), ["a.txt", "b.txt"]);
}

#[tokio::test]
async fn taking_a_batch_back_restores_the_old_names() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["one.txt", "two.txt"]);
    let forward = plan(
        &pairs(&[("one.txt", "1.txt"), ("two.txt", "2.txt")]),
        &[],
        LOCAL,
    );
    run_steps(pool(), folder.path().to_path_buf(), forward, None, never)
        .await
        .unwrap();
    let batch: Vec<(PathBuf, PathBuf)> = vec![
        (folder.path().join("one.txt"), folder.path().join("1.txt")),
        (folder.path().join("two.txt"), folder.path().join("2.txt")),
    ];
    let back = plan(
        &pairs(&[("1.txt", "one.txt"), ("2.txt", "two.txt")]),
        &[],
        LOCAL,
    );

    run_steps(
        pool(),
        folder.path().to_path_buf(),
        back,
        Some(batch),
        never,
    )
    .await
    .unwrap();

    assert_eq!(names_in(folder.path()), ["one.txt", "two.txt"]);
}

#[tokio::test]
async fn taking_back_changes_nothing_when_an_entry_is_gone() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["1.txt"]);
    let batch = vec![
        (folder.path().join("one.txt"), folder.path().join("1.txt")),
        (folder.path().join("two.txt"), folder.path().join("2.txt")),
    ];
    let back = plan(
        &pairs(&[("1.txt", "one.txt"), ("2.txt", "two.txt")]),
        &[],
        LOCAL,
    );

    let failure = run_steps(
        pool(),
        folder.path().to_path_buf(),
        back,
        Some(batch),
        never,
    )
    .await
    .unwrap_err();

    assert_eq!(failure, ApplyFailure::Gone("2.txt".to_owned()));
    assert_eq!(names_in(folder.path()), ["1.txt"]);
}

#[tokio::test]
async fn taking_back_changes_nothing_when_the_old_name_is_taken() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["1.txt", "one.txt"]);
    let batch = vec![(folder.path().join("one.txt"), folder.path().join("1.txt"))];
    let back = plan(&pairs(&[("1.txt", "one.txt")]), &[], LOCAL);

    let failure = run_steps(
        pool(),
        folder.path().to_path_buf(),
        back,
        Some(batch),
        never,
    )
    .await
    .unwrap_err();

    assert_eq!(failure, ApplyFailure::Taken("one.txt".to_owned()));
    assert_eq!(names_in(folder.path()), ["1.txt", "one.txt"]);
}

#[test]
fn hermes_blocks_become_a_pattern_with_the_extension() {
    let spec = NameSpec {
        blocks: vec![NameBlock::Year, NameBlock::Sender, NameBlock::Kind],
        separator: '-',
    };

    assert_eq!(
        pattern_of(&spec).as_deref(),
        Some("{year}-{sender}-{kind}{extension}")
    );
    assert_eq!(
        pattern_of(&NameSpec {
            blocks: Vec::new(),
            separator: '-',
        }),
        None
    );
}

#[test]
fn an_amount_reads_with_two_decimals_and_its_currency() {
    let amount = Amount {
        minor_units: -12_305,
        currency: Currency::new("EUR").unwrap(),
    };

    assert_eq!(amount_text(&amount), "-123.05 EUR");
}

#[tokio::test]
async fn a_stop_after_the_temporary_name_puts_the_swap_back() {
    let folder = tempfile::tempdir().unwrap();
    touch(folder.path(), &["a.txt", "b.txt"]);
    let steps = plan(
        &pairs(&[("a.txt", "b.txt"), ("b.txt", "a.txt")]),
        &["a.txt".to_owned(), "b.txt".to_owned()],
        LOCAL,
    );
    let asked = std::cell::Cell::new(0);
    let stop_after_the_first_rename = || {
        asked.set(asked.get() + 1);
        asked.get() > 1
    };

    let failure = run_steps(
        pool(),
        folder.path().to_path_buf(),
        steps,
        None,
        stop_after_the_first_rename,
    )
    .await
    .unwrap_err();

    assert_eq!(failure, ApplyFailure::Stopped);
    assert_eq!(names_in(folder.path()), ["a.txt", "b.txt"]);
    assert_eq!(
        std::fs::read_to_string(folder.path().join("a.txt")).unwrap(),
        "a.txt"
    );
}

mod abort {
    use std::cell::{Cell, RefCell};

    use iced_test::futures::futures::StreamExt as _;
    use iced_test::runtime::{task, Action as RuntimeAction};
    use ncrs_core::PanelSide;
    use ncrs_i18n::Language;

    use super::*;
    use crate::action::Action;
    use crate::batch_rename::flow::{abort_batch_rename, update_batch_rename};
    use crate::batch_rename::state::BatchRenameMessage;
    use crate::ctx::FlowCtx;
    use crate::message::OpsMessage;
    use crate::state::{BatchRun, Ops, RunOutcome, StopFlag};

    fn running(folder: &std::path::Path, ops: &mut Ops) -> (StopFlag, RunOutcome) {
        let stop = StopFlag::default();
        let outcome = RunOutcome::default();
        ops.batch_running = Some(BatchRun {
            pairs: vec![(folder.join("a.txt"), folder.join("x.txt"))],
            undo: false,
            side: PanelSide::Left,
            stop: stop.clone(),
            outcome: outcome.clone(),
        });
        (stop, outcome)
    }

    fn with_ctx<R>(step: impl FnOnce(&FlowCtx<'_>) -> R) -> R {
        let trash = ncrs_fs::system_trash(None);
        let sandbox = ncrs_fs::SandboxAccess::inactive();
        let remote = pool();
        step(&FlowCtx {
            lang: Language::default(),
            trash: &trash,
            jobs_busy: false,
            start_blocked: false,
            transfer_dialog_open: false,
            has_job_error: false,
            sandbox: &sandbox,
            source_side: PanelSide::Left,
            left_archive: None,
            right_archive: None,
            left_server: None,
            right_server: None,
            remote: &remote,
            permanent_delete_key: None,
            restore_last_key: None,
            index: None,
            model: None,
        })
    }

    /// The renames of `a` and `b`, aborted through `abort_batch_rename` when
    /// the second is about to start; `before_rollback` is whatever happens
    /// to the folder meanwhile. Gives the task the abort asked the app to run.
    async fn abort_between_two_renames(
        folder: &std::path::Path,
        ops: &RefCell<Ops>,
        outcome: &RunOutcome,
        before_rollback: impl Fn(),
    ) -> iced::Task<OpsMessage> {
        touch(folder, &["a.txt", "b.txt"]);
        let steps = plan(
            &pairs(&[("a.txt", "x.txt"), ("b.txt", "y.txt")]),
            &["a.txt".to_owned(), "b.txt".to_owned()],
            LOCAL,
        );
        let asked = Cell::new(0);
        let waiting = RefCell::new(None);
        let stop = ops.borrow().batch_running.as_ref().unwrap().stop.clone();
        let result = run_steps(pool(), folder.to_path_buf(), steps, None, || {
            asked.set(asked.get() + 1);
            if asked.get() == 2 {
                let (wait, immediately) = abort_batch_rename(&mut ops.borrow_mut()).unwrap();
                assert!(
                    immediately.is_empty(),
                    "nothing is reloaded while names are put back"
                );
                *waiting.borrow_mut() = Some(wait);
                before_rollback();
            }
            stop.is_raised()
        })
        .await;
        outcome.report(result);
        waiting.into_inner().expect("the abort came")
    }

    async fn applied_after_abort(ops: &RefCell<Ops>, wait: iced::Task<OpsMessage>) -> Vec<Action> {
        let generation = ops.borrow().generation;
        let mut stream = task::into_stream(wait).expect("the abort waits for the end");
        let Some(RuntimeAction::Output(OpsMessage::BatchRename(message))) = stream.next().await
        else {
            panic!("the end of the rename arrives as a message");
        };
        assert!(
            matches!(&message, BatchRenameMessage::Applied { generation: seen, .. } if *seen == generation)
        );
        with_ctx(|ctx| update_batch_rename(&mut ops.borrow_mut(), message, ctx).1)
    }

    #[tokio::test]
    async fn an_abort_between_two_renames_reads_the_panels_once_the_names_are_back() {
        let folder = tempfile::tempdir().unwrap();
        let ops = RefCell::new(Ops::default());
        let (_stop, outcome) = running(folder.path(), &mut ops.borrow_mut());

        let wait = abort_between_two_renames(folder.path(), &ops, &outcome, || {
            assert_eq!(names_in(folder.path()), ["b.txt", "x.txt"]);
        })
        .await;
        assert_eq!(names_in(folder.path()), ["a.txt", "b.txt"]);

        let asked = applied_after_abort(&ops, wait).await;

        assert!(asked
            .iter()
            .any(|action| matches!(action, Action::ReloadPanels)));
        assert!(!asked
            .iter()
            .any(|action| matches!(action, Action::ShowError(_))));
        assert!(ops.borrow().batch_running.is_none());
    }

    #[tokio::test]
    async fn names_that_cannot_be_put_back_are_reported_with_a_reason() {
        let folder = tempfile::tempdir().unwrap();
        let ops = RefCell::new(Ops::default());
        let (_stop, outcome) = running(folder.path(), &mut ops.borrow_mut());

        let wait = abort_between_two_renames(folder.path(), &ops, &outcome, || {
            std::fs::remove_file(folder.path().join("x.txt")).unwrap();
        })
        .await;

        let asked = applied_after_abort(&ops, wait).await;

        let shown: Vec<&str> = asked
            .iter()
            .filter_map(|action| match action {
                Action::ShowError(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(shown.len(), 1);
        assert!(shown[0].contains("b.txt"), "{}", shown[0]);
        assert!(asked
            .iter()
            .any(|action| matches!(action, Action::ReloadPanels)));
    }

    #[tokio::test]
    async fn a_second_abort_does_not_stop_it_again() {
        let folder = tempfile::tempdir().unwrap();
        let mut ops = Ops::default();
        let (_stop, _outcome) = running(folder.path(), &mut ops);

        assert!(abort_batch_rename(&mut ops).is_some());
        assert!(abort_batch_rename(&mut ops).is_none());
    }
}
