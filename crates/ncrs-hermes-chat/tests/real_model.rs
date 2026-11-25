//! Recognising the order of a folder with the real local model on the test
//! archive. Needs the model: `NCRS_AI_MODELS_DIR` names the folder with it.

#![cfg(feature = "llama")]

mod common;

use std::num::NonZeroU32;
use std::path::PathBuf;
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
        .with_real_model(Arc::clone(model) as Arc<dyn ChatModel>);
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
    model.shutdown();
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
    model.shutdown();
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_a_sentence_puts_the_date_in_front_of_the_file_name_level() {
    let model = real_model();
    let mut talk = archive_talk(&model);
    talk.send(Message::Open);
    talk.send(Message::FolderPicked(Some(talk.path("archive/Arbeit/Gehalt"))));
    let shown = levels_card(&talk);
    println!("recognized: {:?}", shown.rows);
    talk.press(&ChipReply::LevelsChange);

    talk.type_text("kannst du bei 2 das datum vorne hin machen?");

    let changed = levels_card(&talk);
    let file_row = changed.rows.last().expect("a file name row");
    println!("changed: {:?}", changed.rows);
    let names: Vec<&str> = file_row.text.split(", ").collect();
    assert!(!names.is_empty(), "{file_row:?}");
    for name in names {
        let stem = name.strip_suffix(".pdf").unwrap_or(name);
        let (date, word) = stem.split_once('_').unwrap_or_default();
        assert!(
            date.len() == 10 && date.chars().all(|ch| ch.is_ascii_digit() || ch == '-'),
            "the date comes first: {file_row:?}"
        );
        assert_eq!(word, "Gehaltsabrechnung", "{file_row:?}");
    }
    assert_eq!(talk.rule_count(), 0, "changing the levels saves nothing");
    drop(talk);
    model.shutdown();
}

fn levels_card(talk: &Talk) -> &ncrs_hermes_chat::LevelsCard {
    talk.last_parts()
        .iter()
        .find_map(|part| match part {
            ncrs_hermes_chat::Part::Levels(card) => Some(card),
            _ => None,
        })
        .expect("a card of levels")
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
    model.shutdown();
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_hermes_suggests_rules_for_the_order_of_a_folder() {
    let model = real_model();
    let mut talk = archive_talk(&model);

    let area_folder = talk.path(&format!("archive/{AREA}"));
    talk.send(Message::Open);
    talk.send(Message::FolderPicked(Some(area_folder.clone())));
    let deviations_shown = levels_card(&talk).footer.is_some();
    println!("recognized: {:?}", levels_card(&talk).rows);
    talk.press(&ChipReply::LevelsKeep);
    talk.press(&ChipReply::SuggestRules);

    let suggested = talk.suggestions();
    let [(folder, Some(pattern))] = suggested.as_slice() else {
        panic!("one suggestion with a pattern: {suggested:?}");
    };
    assert_eq!(folder, &area_folder);

    let ChatResult::Read(Ok(reading)) = talk.access.run(ChatWork::Read {
        folder: area_folder,
        progress: Arc::new(ReadProgress::default()),
    }) else {
        panic!("the folder could not be read");
    };
    let files: Vec<_> = reading
        .files
        .iter()
        .map(|file| {
            (
                file.relative_path.to_string_lossy().replace('\\', "/"),
                file.fields.clone(),
            )
        })
        .collect();
    let kinds = Kinds::new(Arc::new(KindCatalog::built_in()), KindLanguage::German);
    let mismatches = pattern_mismatches(pattern, &files, &kinds);
    // What the model recognized may not fit every file; then the card said
    // so before the click, and when it fits, nothing deviates.
    assert_eq!(
        deviations_shown,
        !mismatches.is_empty(),
        "{pattern}: {mismatches:?}"
    );
    assert!(mismatches.is_empty(), "{pattern}: {mismatches:?}");
    talk.assert_saves_no_rule();
    drop(talk);
    model.shutdown();
}
