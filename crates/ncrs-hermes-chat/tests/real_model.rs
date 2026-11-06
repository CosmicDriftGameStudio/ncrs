//! The rule flow against the real local model on the test archive. Needs the
//! model: `NCRS_AI_MODELS_DIR` names the folder with it.

#![cfg(feature = "llama")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::Talk;
use ncrs_ai::{pattern_mismatches, ChatModel, LazyLocalModel, LOCAL_MODEL};
use ncrs_filing::chat_work::{ChatResult, ChatWork, ReadProgress};
use ncrs_hermes_chat::{ChipReply, Message};
use ncrs_index::{DocumentFields, KindCatalog, KindLanguage, Kinds};

const MODELS_DIR_VARIABLE: &str = "NCRS_AI_MODELS_DIR";

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_hermes_rule_flow() {
    let folder = PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = folder.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    // Local to the test: dropped before the process ends.
    let model: Arc<LazyLocalModel> = LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    );

    let dir = tempfile::tempdir().unwrap();
    let archive = ncrs_intake::test_archive::test_archive();
    archive.write_to(dir.path()).unwrap();
    let mut talk = Talk::in_dir(dir, &["archive"], &["inbox"])
        .with_real_model(Arc::clone(&model) as Arc<dyn ChatModel>)
        .with_rules();
    for file in &archive.archive {
        if let Some(text) = file.text() {
            let relative = format!("archive/{}", file.relative_path.display());
            let id = talk.index_document(&relative, text);
            let sparkasse = relative.contains("Sparkasse Chemnitz/Kontoauszüge");
            talk.set_fields(
                id,
                &DocumentFields {
                    sender: sparkasse.then(|| "Sparkasse Chemnitz".to_owned()),
                    kind: sparkasse.then(|| "Kontoauszug".to_owned()),
                    date: file.date,
                    ..DocumentFields::default()
                },
            );
        }
    }

    let area_folder = talk.path("archive/Bank/Sparkasse Chemnitz/Kontoauszüge");
    talk.send(Message::Open);
    talk.send(Message::ChangeArea {
        folder: talk.path("archive"),
        area: "Bank/Sparkasse Chemnitz/Kontoauszüge".to_owned(),
    });
    let deviations_shown = talk
        .last_parts()
        .iter()
        .any(|part| matches!(part, ncrs_hermes_chat::Part::Levels(card) if card.footer.is_some()));
    let recognized = talk
        .state
        .chips()
        .iter()
        .any(|chip| chip.reply == ChipReply::LevelsKeep);
    println!("levels recognized and kept: {recognized}");
    talk.press(&if recognized {
        ChipReply::LevelsKeep
    } else {
        ChipReply::LevelsDefault
    });
    talk.type_text("Alle Kontoauszüge der Sparkasse Chemnitz");
    talk.press(&ChipReply::RuleCreate);

    let rules = talk.rules();
    assert_eq!(rules.len(), 1, "{rules:?}");
    let rule = &rules[0];
    assert!(!rule.spec.conditions.list.is_empty(), "{rule:?}");
    assert!(
        rule.spec
            .filing_pattern
            .starts_with("Bank/Sparkasse Chemnitz/Kontoauszüge/"),
        "{rule:?}"
    );

    let progress = Arc::new(ReadProgress::default());
    let ChatResult::Read(Ok(reading)) = talk.access.run(ChatWork::Read {
        folder: area_folder,
        progress: Arc::clone(&progress),
    }) else {
        panic!("the folder could not be read");
    };
    assert!(progress.done.load(Ordering::Relaxed) > 0);
    let files: Vec<_> = reading
        .files
        .iter()
        .map(|file| {
            let relative = file.relative_path.to_string_lossy();
            (
                format!("Bank/Sparkasse Chemnitz/Kontoauszüge/{relative}"),
                file.fields.clone(),
            )
        })
        .collect();
    let kinds = Kinds::new(Arc::new(KindCatalog::built_in()), KindLanguage::German);
    let mismatches = pattern_mismatches(&rule.spec.filing_pattern, &files, &kinds);
    // What the model recognized may not fit every file; then the card said
    // so before the click, and when it fits, nothing deviates.
    assert_eq!(
        deviations_shown,
        !mismatches.is_empty(),
        "{}: {mismatches:?}",
        rule.spec.filing_pattern
    );
    assert!(
        mismatches.is_empty(),
        "{}: {mismatches:?}",
        rule.spec.filing_pattern
    );
    drop(talk);
    drop(model);
}
