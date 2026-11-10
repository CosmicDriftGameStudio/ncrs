//! The chat against the real model, through the same path the app uses.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::use_debug,
    clippy::integer_division
)]

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::eval::{rows, HOLDOUT_ROWS, MAIN_ROWS};
use super::scoring::score;
use super::shard::{parse_shard, select, Shard, SHARD_VARIABLE};
use super::{ChatIntent, ChatModel as _, ChatTurn};
use crate::local::models_dir;
use crate::model_file::LOCAL_MODEL;
use crate::status::LocalModelStatus;
use crate::LazyLocalModel;

const THREADS: u32 = 4;
const CONTEXT_TOKENS: u32 = 4096;

fn nonzero(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn model_path() -> std::path::PathBuf {
    let path = models_dir().join(LOCAL_MODEL.name);
    assert!(
        path.is_file(),
        "{} is missing; set NCRS_AI_MODELS_DIR to the folder with the model",
        path.display()
    );
    path
}

/// A model of the test itself: dropped with the test, before the process ends.
fn lazy() -> Arc<LazyLocalModel> {
    LazyLocalModel::shared(
        model_path(),
        LOCAL_MODEL,
        nonzero(THREADS),
        nonzero(CONTEXT_TOKENS),
    )
}

/// `None` when the variable is not set: the whole run, with its thresholds.
fn shard_from_env() -> Option<Shard> {
    let value = std::env::var(SHARD_VARIABLE).ok()?;
    Some(parse_shard(&value).unwrap_or_else(|message| panic!("{message}")))
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_chat_eval_meets_the_measured_rates() {
    let model = lazy();
    let every_row: Vec<_> = rows(MAIN_ROWS)
        .into_iter()
        .chain(rows(HOLDOUT_ROWS))
        .collect();
    let total = every_row.len();
    let shard_setting = shard_from_env();
    let all = match shard_setting {
        Some(chosen) => select(every_row, chosen),
        None => every_row,
    };
    assert!(!all.is_empty(), "the shard has no sentences");
    // The first query warms up the compute graphs; it is not counted.
    model.understand(&all[0].turn()).unwrap();

    let (mut intent_ok, mut full_ok): (usize, usize) = (0, 0);
    let (mut total_ms, mut max_ms): (u128, u128) = (0, 0);
    for row in &all {
        let started = Instant::now();
        let answer = model.understand(&row.turn());
        let millis = started.elapsed().as_millis();
        total_ms += millis;
        max_ms = max_ms.max(millis);
        let mut hit = false;
        match &answer {
            Ok(intent) => {
                let (same_intent, same_fields) = score(row, intent);
                intent_ok += usize::from(same_intent);
                full_ok += usize::from(same_fields);
                hit = same_fields;
                if !same_fields {
                    println!("miss {} expected {} got {intent:?}", row.id, row.intent);
                }
            }
            Err(error) => println!("miss {} expected {} error {error}", row.id, row.intent),
        }
        println!(
            "sentence {} {millis} ms {}",
            row.id,
            if hit { "ok" } else { "miss" }
        );
    }
    let count = all.len();
    let compute = match model.status() {
        LocalModelStatus::Loaded { compute, .. } => format!("{compute:?}"),
        other => format!("{other:?}"),
    };
    println!(
        "chat eval: sentences {count} intent {intent_ok} full {full_ok} mean_ms {} max_ms {max_ms} compute {compute}",
        total_ms / u128::try_from(count).unwrap()
    );

    model.shutdown();
    if let Some(chosen) = shard_setting {
        // Read by the aggregation job of the workflow; the thresholds apply to
        // the sum of the shards, not to one of them.
        println!(
            "chat eval shard {chosen}: total {total} sentences {count} intent {intent_ok} full {full_ok}"
        );
        return;
    }
    assert!(intent_ok * 100 >= count * 92, "intent {intent_ok}/{count}");
    assert!(full_ok * 100 >= count * 85, "full {full_ok}/{count}");
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_model_unloads_after_idle_and_wakes_again() {
    let model = LazyLocalModel::with_idle_unload(
        model_path(),
        LOCAL_MODEL,
        nonzero(THREADS),
        nonzero(CONTEXT_TOKENS),
        Duration::from_secs(2),
    );
    let turn = ChatTurn {
        message: "mach das rückgängig".to_owned(),
        question: None,
        previous_search: None,
        previous_request: None,
        previous_message: None,
    };
    let is_loaded = || matches!(model.status(), LocalModelStatus::Loaded { .. });
    let wait_for = |wanted: bool| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while is_loaded() != wanted {
            assert!(
                Instant::now() < deadline,
                "the status never became {wanted}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    assert_eq!(model.status(), LocalModelStatus::NotLoadedYet);
    assert_eq!(model.understand(&turn), Ok(ChatIntent::Undo));
    assert!(is_loaded());

    wait_for(false);
    assert_eq!(model.status(), LocalModelStatus::NotLoadedYet);

    assert_eq!(model.understand(&turn), Ok(ChatIntent::Undo));
    assert!(is_loaded());
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_protocol_sentences_print_their_intent() {
    let model = lazy();
    let sentences = [
        "Downloads aufräumen",
        "Bank nach Monat",
        "Versicherung nach Anbieter",
        "Mach mal einen Vorschlag, im Root sollten die Themen stehen",
        "Was kannst du?",
        "Danke",
        "Tidy up Downloads",
        "Bank by month",
        "Insurance by sender",
        "Make a suggestion, the topics should be at the root",
        "What can you do?",
        "Thanks",
    ];
    for sentence in sentences {
        let turn = ChatTurn {
            message: sentence.to_owned(),
            question: None,
            previous_search: None,
            previous_request: None,
            previous_message: None,
        };
        let intent = model.understand(&turn);
        println!("intent {sentence:?} => {intent:?}");
        if matches!(sentence, "Was kannst du?" | "What can you do?") {
            assert_eq!(intent, Ok(ChatIntent::SmallTalk), "{sentence:?}");
        }
    }
    model.shutdown();
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn folder_sentences_print_their_intent_with_the_model() {
    let model = lazy();
    let sentences = [
        "öffne mal /Bank",
        "zeig mir /Archiv/Versicherung",
        "was liegt in /Scans",
        "open ~/Documents/Taxes",
        "Kontoauszüge der Sparkasse kommen nach Bank",
        "Rechnungen von der Telekom nach Telefon, Jahr und Monat",
        "räum /Downloads auf",
        "sortier /Downloads nach Jahr",
    ];
    for sentence in sentences {
        let turn = ChatTurn {
            message: sentence.to_owned(),
            question: None,
            previous_search: None,
            previous_request: None,
            previous_message: None,
        };
        println!("intent {sentence:?} => {:?}", model.understand(&turn));
    }
    model.shutdown();
}
