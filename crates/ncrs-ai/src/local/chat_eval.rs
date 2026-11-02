//! The chat against the real model, through the same path the app uses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::tests::models_dir;
use super::LazyLocalModel;
use crate::chat::eval::{rows, HOLDOUT_ROWS, MAIN_ROWS};
use crate::chat::scoring::score;
use crate::chat::{ChatIntent, ChatModel, ChatTurn};
use crate::model_file::LOCAL_MODEL;
use crate::status::LocalModelStatus;

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

fn lazy() -> &'static Arc<LazyLocalModel> {
    static MODEL: OnceLock<Arc<LazyLocalModel>> = OnceLock::new();
    MODEL.get_or_init(|| {
        LazyLocalModel::shared(
            model_path(),
            LOCAL_MODEL,
            nonzero(THREADS),
            nonzero(CONTEXT_TOKENS),
        )
    })
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_chat_eval_meets_the_measured_rates() {
    let model = lazy();
    let all: Vec<_> = rows(MAIN_ROWS)
        .into_iter()
        .chain(rows(HOLDOUT_ROWS))
        .collect();
    // The first query warms up the compute graphs; it is not counted.
    model.understand(&all[0].turn()).unwrap();

    let (mut intent_ok, mut full_ok) = (0_usize, 0_usize);
    let (mut total_ms, mut max_ms) = (0_u128, 0_u128);
    for row in &all {
        let started = Instant::now();
        let answer = model.understand(&row.turn());
        let millis = started.elapsed().as_millis();
        total_ms += millis;
        max_ms = max_ms.max(millis);
        match &answer {
            Ok(intent) => {
                let (same_intent, same_fields) = score(row, intent);
                intent_ok += usize::from(same_intent);
                full_ok += usize::from(same_fields);
                if !same_fields {
                    println!("miss {} expected {} got {intent:?}", row.id, row.intent);
                }
            }
            Err(error) => println!("miss {} expected {} error {error}", row.id, row.intent),
        }
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
