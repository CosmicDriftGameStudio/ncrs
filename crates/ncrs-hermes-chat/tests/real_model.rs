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

const AREA: &str = "Bank/Sparkasse Chemnitz/Kontoauszüge";

fn real_model() -> Arc<LazyLocalModel> {
    let folder = PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = folder.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    )
}

/// The test archive in a temp dir and in the index; the account statements of
/// the Sparkasse carry sender and kind.
fn archive_talk(model: &Arc<LazyLocalModel>) -> Talk {
    let dir = tempfile::tempdir().unwrap();
    let archive = ncrs_intake::test_archive::test_archive();
    archive.write_to(dir.path()).unwrap();
    let talk = Talk::in_dir(dir, &["archive"], &["inbox"])
        .with_real_model(Arc::clone(model) as Arc<dyn ChatModel>)
        .with_rules();
    for file in &archive.archive {
        if let Some(text) = file.text() {
            let relative = format!("archive/{}", file.relative_path.display());
            let id = talk.index_document(&relative, text);
            let sparkasse = relative.contains(AREA);
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
    talk
}

fn has_levels_card(talk: &Talk) -> bool {
    talk.last_parts()
        .iter()
        .any(|part| matches!(part, ncrs_hermes_chat::Part::Levels(_)))
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_a_typed_open_reaches_the_levels_through_the_router() {
    let model = real_model();
    let mut talk = archive_talk(&model);
    talk.send(Message::Open);
    talk.type_text(&format!("öffne mal /{AREA}"));
    assert!(
        has_levels_card(&talk),
        "{:?} {:?}",
        talk.last_texts(),
        talk.labels()
    );
    assert_eq!(talk.rule_count(), 0, "opening saves nothing");
    drop(talk);
    drop(model);
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_a_nested_folder_is_opened_by_its_name_alone() {
    let model = real_model();
    let mut talk = archive_talk(&model);
    talk.send(Message::Open);
    talk.type_text("öffne mal die Kontoauszüge");
    let wanted = talk.path(&format!("archive/{AREA}"));
    if !has_levels_card(&talk) {
        talk.press(&ChipReply::NamedFolder(wanted));
    }
    assert!(
        has_levels_card(&talk),
        "{:?} {:?}",
        talk.last_texts(),
        talk.labels()
    );
    drop(talk);
    drop(model);
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_an_empty_folder_gets_the_default_levels_and_one_rule() {
    let model = real_model();
    let mut talk = archive_talk(&model);
    std::fs::create_dir_all(talk.path("archive/Telefon")).unwrap();
    talk.send(Message::Open);
    talk.send(Message::ChangeArea {
        folder: talk.path("archive"),
        area: "Telefon".to_owned(),
    });
    talk.press(&ChipReply::LevelsKeep);
    talk.type_text("Alle Kontoauszüge der Sparkasse Chemnitz");
    talk.press(&ChipReply::RuleCreate);
    let rules = talk.rules();
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(
        rules[0].spec.filing_pattern,
        "Telefon/{year}/{date}_{sender}_{kind}"
    );
    drop(talk);
    drop(model);
}

#[test]
#[ignore = "measures the router on a warm model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_router_latency_per_sentence() {
    let model = real_model();
    let mut talk = archive_talk(&model);
    talk.send(Message::Open);
    let sentences = [
        "Suche Rechnungen von der Telekom",
        "Räum den Posteingang auf",
        "mach das rückgängig",
        "Zeig mir die Kontoauszüge",
        "Wo liegt der Mietvertrag?",
        "Zeig mir die Rechnungen von letztem Jahr",
        "Was kannst du?",
        "Finde alles von der Sparkasse",
    ];
    talk.type_text("hallo");
    let mut millis = Vec::new();
    for sentence in sentences {
        let started = std::time::Instant::now();
        talk.type_text(sentence);
        millis.push(started.elapsed().as_millis());
        talk.send(Message::Open);
    }
    millis.sort_unstable();
    println!("router ms per sentence (sorted): {millis:?}");
    println!("router median ms: {}", millis[millis.len() / 2]);
    println!(
        "router mean ms: {}",
        millis.iter().sum::<u128>() / millis.len() as u128
    );
    drop(talk);
    drop(model);
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_hermes_rule_flow() {
    let model = real_model();
    let mut talk = archive_talk(&model);

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
