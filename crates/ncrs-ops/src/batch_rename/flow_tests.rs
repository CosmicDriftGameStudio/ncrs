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

    run_steps(pool(), folder.path().to_path_buf(), steps, None)
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

    let failure = run_steps(pool(), folder.path().to_path_buf(), steps, None)
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
    run_steps(pool(), folder.path().to_path_buf(), forward, None)
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

    run_steps(pool(), folder.path().to_path_buf(), back, Some(batch))
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

    let failure = run_steps(pool(), folder.path().to_path_buf(), back, Some(batch))
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

    let failure = run_steps(pool(), folder.path().to_path_buf(), back, Some(batch))
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
