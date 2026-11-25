//! The reading half of the filing eval: the real program with the local model
//! reads a copy of an archive and of a source folder, and the folder it worked
//! in is kept. `brother_eval_trial` in `ncrs-filing` then plans the source into
//! the archive from that folder and counts the hits.
//!
//! ```text
//! NCRS_EVAL_ARCHIVE=<archive, only read> NCRS_EVAL_SOURCE=<scans, only read>
//! NCRS_AI_MODELS_DIR=<models> NCRS_OCR_MODELS_DIR=<models> NCRS_PDFIUM_DIR=<pdfium>
//! cargo test --release -p ncrs brother_eval_read -- --ignored --nocapture
//! ```
//!
//! The folder is `target/eval/<stamp>/`: `master` and `run/archive` (the archive),
//! `run/downloads` (the source) and `index.db` (the index with the fields the
//! model read). `NCRS_PROTOCOL_AI_WAIT_SECS` limits the wait for the model;
//! `NCRS_EVAL_RESUME=<that folder>` continues a run the wait cut short, and
//! `NCRS_EVAL_TIDY=1` then tidies the source in the chat, with screenshots in `out`.
//! `NCRS_EVAL_FILE=copy|move` then files the plan in the tab and undoes it.
//! `NCRS_EVAL_CONTINUOUS=<folder with new files>` (with `NCRS_EVAL_TIDY=1` and
//! `NCRS_EVAL_FILE=copy` or `move`) answers "Always" in the chat instead of "Once", so the
//! source files by itself afterwards, copies the folder into the scratch copy
//! of the source, waits (`NCRS_PROTOCOL_AI_WAIT_SECS`) for the background run,
//! prints what it filed and the review entries before and after, takes the
//! pictures for the acceptance (1740x1130) and undoes the run; after the undo
//! the index must not call the put-back files filed.
//! A run that ended while it was still reading is continued the same way, with
//! everything it had read.
//!
//! To start from a fixed base instead of reading everything: write the hash list
//! (`eval_base::seed_hash_list` in `ncrs-pipeline`), extract the documents once
//! (`eval_seed::seed_extract` in `ncrs-index`, needs the key), build the folder
//! (`eval_base::seed_base`, anew for each run, it takes seconds) and resume from
//! it with `NCRS_EVAL_RESUME=<that folder>`. The index knows the files by their
//! path: do not move the folder.
//!
//! `NCRS_EVAL_QUIT_AFTER_SECS=<n>` quits n seconds into the reading instead and
//! prints how long the quit and the end of the app took.

use std::path::Path;
use std::time::{Duration, Instant};

use ncrs_i18n::Language;
use ncrs_index::{ContentHash, DocumentSource, InMemoryKeyStore, Index, NewDocument};

use crate::messages::Message;

use super::scratch::{copy_tree, count_files, report_removal};
use super::{env_path, start_protocol};

/// Lies in the kept folder while a run is reading. Afterwards the scratch index
/// also holds what the flows moved, so only a marked one is rescued.
const READING: &str = "reading";

/// Keeps what a run that ended while reading had read. Nothing is removed unless
/// the rescued index is whole.
fn rescue_unfinished_reading(kept: &Path) -> bool {
    let marker = kept.join(READING);
    let scratch_index = kept.join("scratch").join("data").join("index.db");
    if !marker.is_file() || !scratch_index.is_file() {
        return false;
    }
    // Opened where it lies: a journal a kill left beside it belongs to it.
    let index = Index::open(&scratch_index, &InMemoryKeyStore::empty()).unwrap();
    assert_eq!(
        index.damage().unwrap(),
        None,
        "the index to rescue is damaged"
    );
    let rescued = kept.join("index.db.rescued");
    report_removal(&rescued, std::fs::remove_file(&rescued));
    index.save_plain_copy(&rescued).unwrap();
    drop(index);
    std::fs::rename(&rescued, kept.join("index.db")).unwrap();
    std::fs::remove_file(&marker).unwrap();
    true
}

#[test]
#[ignore = "reads NCRS_EVAL_ARCHIVE and NCRS_EVAL_SOURCE with the real local model; see the module doc"]
fn brother_eval_read() {
    let (Some(archive), Some(source)) =
        (env_path("NCRS_EVAL_ARCHIVE"), env_path("NCRS_EVAL_SOURCE"))
    else {
        println!(
            "brother_eval_read: NCRS_EVAL_ARCHIVE or NCRS_EVAL_SOURCE is not set, nothing to run"
        );
        return;
    };
    let resumed = env_path("NCRS_EVAL_RESUME");
    let kept = if let Some(kept) = resumed.clone() {
        if rescue_unfinished_reading(&kept) {
            println!("continuing a run that ended while it was reading");
        }
        // `run` and `scratch` are rebuilt from `master` and the source at the
        // same paths, so the kept index knows the documents read before.
        assert!(
            kept.join("index.db").is_file() && kept.join("master").is_dir(),
            "NCRS_EVAL_RESUME is not a folder brother_eval_read kept: {}",
            kept.display()
        );
        for folder in ["run", "scratch"] {
            let path = kept.join(folder);
            if path.exists() {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
        kept
    } else {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let kept = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("eval")
            .join(stamp);
        std::fs::create_dir_all(&kept).unwrap();
        // Texts and files lie here unencrypted.
        #[cfg(unix)]
        std::fs::set_permissions(&kept, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        copy_tree(&archive, &kept.join("master")).unwrap();
        kept
    };
    println!(
        "copied the archive: {} files",
        count_files(&kept.join("master"))
    );
    let mut run = start_protocol(
        &archive,
        &kept,
        &kept.join("scratch"),
        &kept.join("out"),
        Language::German,
        resumed.is_some(),
        Some(&source),
    );
    let marker = kept.join(READING);
    std::fs::write(&marker, b"").unwrap();
    run.flow_1_greeting_while_starting();
    if let Some(seconds) = std::env::var("NCRS_EVAL_QUIT_AFTER_SECS")
        .ok()
        .and_then(|text| text.parse().ok())
    {
        let reading_until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < reading_until {
            run.pump(Duration::from_millis(200));
        }
        let load = std::process::Command::new("ps")
            .args(["-Ao", "pcpu,comm", "-r"])
            .output()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .take(8)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        println!("load while reading:\n{load}");
        let started = Instant::now();
        run.deliver(Message::Quit);
        println!("quit returned after {} s", started.elapsed().as_secs_f32());
        let drop_started = Instant::now();
        drop(run);
        #[cfg(feature = "local-ai")]
        ncrs_ai::LazyLocalModel::shutdown_all();
        println!(
            "the app ended after another {} s",
            drop_started.elapsed().as_secs_f32()
        );
        return;
    }
    run.wait_for_indexing();
    assert!(kept.join("index.db").is_file(), "the index was not kept");
    std::fs::remove_file(&marker).unwrap();
    if env_path("NCRS_EVAL_TIDY").is_some() {
        run.flow_5_tidy();
    }
    drop(run);
    // The background folder picks may still hold the model, even after the
    // run is dropped; a process that ends with it loaded makes the GPU backend
    // abort.
    #[cfg(feature = "local-ai")]
    ncrs_ai::LazyLocalModel::shutdown_all();
    println!("NCRS_EVAL_SCRATCH={}", kept.display());
}

/// A kept folder with one document in the kept index and two in the scratch
/// index, as a run leaves it.
fn kept_folder() -> tempfile::TempDir {
    let kept = tempfile::TempDir::new().unwrap();
    let data = kept.path().join("scratch").join("data");
    std::fs::create_dir_all(&data).unwrap();
    for (folder, documents) in [(kept.path(), 1), (data.as_path(), 2)] {
        let index = Index::create_plain(&folder.join("index.db")).unwrap();
        for seed in 1..=documents {
            index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: format!("{seed}.pdf").into(),
                    source: DocumentSource::Archive,
                    received_at: chrono::Utc::now(),
                    stamp: None,
                })
                .unwrap();
        }
    }
    kept
}

fn kept_documents(kept: &Path) -> usize {
    let index = Index::open(&kept.join("index.db"), &InMemoryKeyStore::empty()).unwrap();
    (1..=2)
        .filter(|seed| {
            index
                .document_by_hash(&ContentHash([*seed; 32]))
                .unwrap()
                .is_some()
        })
        .count()
}

#[test]
fn a_run_that_ended_while_reading_keeps_what_it_read() {
    let kept = kept_folder();
    std::fs::write(kept.path().join(READING), b"").unwrap();

    assert!(rescue_unfinished_reading(kept.path()));

    assert_eq!(kept_documents(kept.path()), 2);
    assert!(!kept.path().join(READING).exists());
    assert!(!kept.path().join("index.db.rescued").exists());
}

#[test]
fn a_fresh_run_that_ended_while_reading_had_no_kept_index_yet() {
    let kept = kept_folder();
    std::fs::remove_file(kept.path().join("index.db")).unwrap();
    std::fs::write(kept.path().join(READING), b"").unwrap();

    assert!(rescue_unfinished_reading(kept.path()));

    assert_eq!(kept_documents(kept.path()), 2);
}

#[test]
fn a_finished_reading_is_not_replaced_by_what_the_flows_moved() {
    let kept = kept_folder();

    assert!(!rescue_unfinished_reading(kept.path()));

    assert_eq!(kept_documents(kept.path()), 1);
}
