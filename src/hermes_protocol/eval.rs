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
//! `NCRS_EVAL_QUIT_AFTER_SECS=<n>` quits n seconds into the reading instead and
//! prints how long the quit and the end of the app took.

use std::path::Path;
use std::time::{Duration, Instant};

use ncrs_i18n::Language;

use crate::messages::Message;

use super::scratch::{copy_tree, count_files};
use super::{env_path, start_protocol};

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
        // `run` and `scratch` are rebuilt from `master` and the source at the
        // same paths, so the kept index knows the documents read before.
        assert!(
            kept.join("index.db").is_file() && kept.join("master").is_dir(),
            "NCRS_EVAL_RESUME is not a folder brother_eval_read kept: {}",
            kept.display()
        );
        std::fs::remove_dir_all(kept.join("run")).unwrap();
        std::fs::remove_dir_all(kept.join("scratch")).unwrap();
        kept
    } else {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let kept = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("eval")
            .join(stamp);
        std::fs::create_dir_all(&kept).unwrap();
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
        println!(
            "the app ended after another {} s",
            drop_started.elapsed().as_secs_f32()
        );
        return;
    }
    run.wait_for_indexing();
    if env_path("NCRS_EVAL_TIDY").is_some() {
        run.flow_5_tidy();
    }
    drop(run);
    // The background folder picks may still hold the model; a process that
    // ends with it loaded makes the GPU backend abort.
    ncrs_ai::LazyLocalModel::shutdown_all();
    println!("NCRS_EVAL_SCRATCH={}", kept.display());
}
