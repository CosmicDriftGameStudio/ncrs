//! Screenshots of the change of the levels with the real local model: for
//! each way of changing the pay slip folder, the card with the change marked
//! and the dry run after "Apply". The flows themselves are asserted in
//! `crates/ncrs-hermes-chat/tests/flow_matrix.rs`. Needs the model:
//! `NCRS_AI_MODELS_DIR` names the folder with it. Screenshots go to
//! `target/acceptance/flow-matrix/`.

#![cfg(feature = "local-ai")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroU32;
use std::sync::Mutex;

use iced_test::Simulator;
use ncrs_ai::{ChatModel, LazyLocalModel, LOCAL_MODEL};
use ncrs_filing::chat_work::ChatAccess;
use ncrs_filing::rules_work::RulesAccess;
use ncrs_hermes_chat::{Availability, ChipReply, Ctx};
use ncrs_index::{DocumentFields, Index, KindLanguage};
use ncrs_intake::TextChain;
use sha2::Digest as _;

use super::hermes_real_model_tests::{
    canonical_folder, has_chip, levels_card_shown, press, type_sentence, Utf8Text,
};
use crate::app::*;
use crate::i18n::Language;

type IndexSlot = std::sync::Arc<Mutex<Option<Index>>>;

const MODELS_DIR_VARIABLE: &str = "NCRS_AI_MODELS_DIR";
const SENDER: &str = "Cosmicdrift GmbH";
const SHOT_DIR: &str = "target/acceptance/flow-matrix";
const SHOT_SIZE: iced::Size = iced::Size::new(1740.0, 1130.0);

/// Name, the sentence in German and in English.
const CASES: [(&str, &str, &str); 8] = [
    (
        "1-rename",
        "nenne Ebene 1 Lohn statt Gehalt",
        "rename level 1 to Lohn",
    ),
    (
        "2-year-month",
        "Jahr und Monat zusammen in Ebene 2, mit Bindestrich",
        "put year and month together in level 2, with a dash",
    ),
    (
        "3-date-first",
        "Datum vorne bei Ebene 3",
        "date first at level 3",
    ),
    (
        "1+2",
        "nenne Ebene 1 Lohn und mach Jahr und Monat zusammen in Ebene 2, mit Bindestrich",
        "rename level 1 to Lohn and put year and month together in level 2, with a dash",
    ),
    (
        "1+3",
        "nenne Ebene 1 Lohn und setz das Datum vorne bei Ebene 3",
        "rename level 1 to Lohn and put the date first in level 3",
    ),
    (
        "2+3",
        "Jahr und Monat zusammen in Ebene 2, mit Bindestrich, und das Datum vorne bei Ebene 3",
        "put year and month together in level 2, with a dash, and the date first in level 3",
    ),
    (
        "1+2+3",
        "nenne Ebene 1 Lohn, mach Jahr und Monat zusammen in Ebene 2, mit Bindestrich, und setz das Datum vorne bei Ebene 3",
        "rename level 1 to Lohn, put year and month together in level 2, with a dash, and the date first in level 3",
    ),
    (
        "report-bare-3-then-date-first",
        "mach bei 3 das datum vorne hin",
        "put the date first in level 3",
    ),
];

fn shoot(app: &App, name: &str) {
    std::fs::create_dir_all(SHOT_DIR).unwrap();
    let shot = std::path::PathBuf::from(SHOT_DIR);
    drop(std::fs::remove_file(
        shot.join(format!("{name}-tiny-skia.png")),
    ));
    // A missing file is written instead of compared.
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        SHOT_SIZE,
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_image(shot.join(format!("{name}.png")))
    .unwrap();
    assert!(shot.join(format!("{name}-tiny-skia.png")).is_file());
}

/// The pay slips of the test archive in the index, five more in the inbox.
fn pay_slip_archive(dir: &std::path::Path, slot: &Mutex<Option<Index>>) {
    let root = std::fs::canonicalize(dir).unwrap();
    std::fs::create_dir_all(root.join("archive")).unwrap();
    std::fs::create_dir_all(root.join("inbox")).unwrap();
    let archive = ncrs_intake::test_archive::test_archive();
    let mut documents: Vec<(std::path::PathBuf, String, ncrs_index::DocumentSource, _)> =
        Vec::new();
    for file in &archive.archive {
        let Some(text) = file.text() else { continue };
        if file.relative_path.starts_with("Arbeit/Gehalt") {
            documents.push((
                root.join("archive").join(&file.relative_path),
                text.to_owned(),
                ncrs_index::DocumentSource::Archive,
                file.date,
            ));
        }
    }
    for month in 1..=5 {
        let date = chrono::NaiveDate::from_ymd_opt(2025, month, 25).unwrap();
        documents.push((
            root.join("inbox").join(format!("Eingang_{month}.pdf")),
            format!(
                "{SENDER}\nMoritzhof 1, 09111 Chemnitz\n\nChemnitz, {}\n\nGehaltsabrechnung\n",
                date.format("%d.%m.%Y")
            ),
            ncrs_index::DocumentSource::Inbox,
            Some(date),
        ));
    }
    for (path, text, source, date) in documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        let mut guard = slot.lock().unwrap();
        let index = guard.as_mut().unwrap();
        let hash: [u8; 32] = sha2::Sha256::digest(text.as_bytes()).into();
        let id = index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash(hash),
                path,
                source,
                received_at: chrono::Utc::now(),
                stamp: None,
            })
            .unwrap();
        index.set_document_text(id, &text).unwrap();
        index
            .set_document_fields(
                id,
                &DocumentFields {
                    sender: Some(SENDER.to_owned()),
                    date,
                    ..DocumentFields::default()
                },
                |_| ncrs_index::FieldOrigin::Manual,
            )
            .unwrap();
    }
}

/// The app with the pay slip archive, the chat opened on the folder Arbeit
/// and its levels card on screen.
/// Says "open Arbeit" and takes the archive folder when Hermes asks which.
fn open_arbeit(app: &mut App, ctx: &Ctx, access: &ChatAccess, answering: &dyn ChatModel) {
    type_sentence(app, ctx, access, answering, "öffne mal Arbeit");
    let named: Vec<ChipReply> = app
        .hermes_chat
        .chips()
        .iter()
        .filter_map(|chip| match &chip.reply {
            ChipReply::NamedFolder(path) if path.ends_with("archive/Arbeit") => {
                Some(chip.reply.clone())
            }
            _ => None,
        })
        .collect();
    if let Some(reply) = named.first() {
        press(app, ctx, access, answering, reply);
    }
}

fn opened_pay_slips(
    model: &std::sync::Arc<LazyLocalModel>,
    lang: Language,
    answering: &dyn ChatModel,
    name: &str,
) -> (tempfile::TempDir, App, Ctx, ChatAccess, IndexSlot) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let slot = std::sync::Arc::new(Mutex::new(Some(
        Index::create_plain(&dir.path().join("index.db")).unwrap(),
    )));
    pay_slip_archive(dir.path(), &slot);
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let trash =
        ncrs_fs::JournaledTrash::new(dir.path().join("trash.toml"), ncrs_fs::FolderBackend(bin));
    let archive_location = ncrs_intake::Location::new(
        &root.join("archive"),
        ncrs_intake::LocationMode::Archive,
        false,
    )
    .unwrap();
    let inbox_location =
        ncrs_intake::Location::new(&root.join("inbox"), ncrs_intake::LocationMode::Inbox, false)
            .unwrap();
    let rules = RulesAccess::new(
        std::sync::Arc::clone(&slot),
        vec![archive_location, inbox_location],
    )
    .with_kind_language(KindLanguage::German);
    let access = ChatAccess::new(
        std::sync::Arc::clone(&slot),
        std::sync::Arc::new(TextChain::new().with_source(Utf8Text)),
        std::sync::Arc::new(trash),
    )
    .with_kind_language(KindLanguage::German)
    .with_rules(Some(rules));
    let ctx = Ctx {
        availability: Availability::Ready(Box::new(access.clone())),
        targets: vec![root.join("archive")],
        sources: vec![root.join("inbox")],
        resolve_folder: canonical_folder,
        home: root.clone(),
        model: Some(std::sync::Arc::clone(model) as std::sync::Arc<dyn ChatModel>),
        ..super::hermes_chat_ui_tests::ready(lang)
    };

    let mut app = App::with_fixed_panels().in_language(lang);
    super::hermes_chat_ui_tests::send(&mut app, &ctx, HermesChatMessage::Open);
    open_arbeit(&mut app, &ctx, &access, answering);
    assert!(
        levels_card_shown(&app),
        "{name}: {:?}",
        app.hermes_chat.thread.last()
    );
    (dir, app, ctx, access, slot)
}

fn local_model() -> std::sync::Arc<LazyLocalModel> {
    let models =
        std::path::PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = models.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    )
}

/// The proposal with the reason of its condition, the sentence that changes
/// the condition, the saved rule with the question about the existing files,
/// and the answer "No, only new ones".
#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_rule_flow_from_the_proposal_to_the_question_is_shot() {
    let model = local_model();
    let answering: &dyn ChatModel = model.as_ref();
    for (lang, suffix, sentence) in [
        (
            Language::German,
            "de",
            "nur wenn im Text Gehaltsabrechnung steht",
        ),
        (
            Language::English,
            "en",
            "only if the text says Gehaltsabrechnung",
        ),
    ] {
        let name = &format!("rule-{suffix}");
        let (_dir, mut app, ctx, access, _slot) = opened_pay_slips(&model, lang, answering, name);
        press(&mut app, &ctx, &access, answering, &ChipReply::LevelsKeep);
        assert!(has_chip(&app, &ChipReply::RuleCondition), "{name}");
        assert!(has_chip(&app, &ChipReply::LevelsChange), "{name}");
        shoot(&app, &format!("{name}-1-proposal"));
        press(
            &mut app,
            &ctx,
            &access,
            answering,
            &ChipReply::RuleCondition,
        );
        shoot(&app, &format!("{name}-2-ask-condition"));
        type_sentence(&mut app, &ctx, &access, answering, sentence);
        assert!(has_chip(&app, &ChipReply::RuleCreate), "{name}");
        shoot(&app, &format!("{name}-3-new-condition"));

        // The question about the existing files, from the sender proposal.
        let (second_dir, mut second_app, second_ctx, second_access, slot) =
            opened_pay_slips(&model, lang, answering, name);
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::LevelsKeep,
        );
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::RuleCreate,
        );
        assert!(has_chip(&second_app, &ChipReply::RuleNow), "{name}");
        shoot(&second_app, &format!("{name}-4-saved-question"));
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::RuleOnlyNew,
        );
        shoot(&second_app, &format!("{name}-5-only-new"));

        // The folder again: the rule exists, saving changes it.
        open_arbeit(&mut second_app, &second_ctx, &second_access, answering);
        shoot(&second_app, &format!("{name}-6-second-open"));
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::LevelsKeep,
        );
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::RuleCreate,
        );
        shoot(&second_app, &format!("{name}-7-rule-adjusted"));

        let root = std::fs::canonicalize(second_dir.path()).unwrap();
        let before = files_on_disk(&root);
        let found_before = searched(&slot, &root);
        assert!(has_chip(&second_app, &ChipReply::RuleNow), "{name}");
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::RuleNow,
        );
        shoot(&second_app, &format!("{name}-8-rearranged"));
        let after = files_on_disk(&root);
        let mut removed: Vec<(&String, &String)> = before
            .iter()
            .filter(|(path, _)| !after.contains_key(*path))
            .collect();
        let mut added: Vec<(&String, &String)> = after
            .iter()
            .filter(|(path, _)| !before.contains_key(*path))
            .collect();
        assert!(!removed.is_empty(), "{name}: nothing moved");
        assert_eq!(removed.len(), added.len(), "{name}");
        removed.sort_by_key(|(path, content)| (*content, *path));
        added.sort_by_key(|(path, content)| (*content, *path));
        let found_after = searched(&slot, &root);
        let mut searched_pairs = 0;
        for ((old, old_content), (new, new_content)) in removed.iter().zip(&added) {
            assert_eq!(old_content, new_content, "{name}: {old} -> {new}");
            assert!(root.join(new).is_file(), "{name}: {new} missing");
            assert!(!root.join(old).exists(), "{name}: {old} remains");
            assert!(
                index_knows(&slot, &root.join(new)),
                "{name}: index lacks {new}"
            );
            assert!(
                !index_knows(&slot, &root.join(old)),
                "{name}: index keeps {old}"
            );
            if found_before.contains(*old) {
                searched_pairs += 1;
                assert!(found_after.contains(*new), "{name}: search lacks {new}");
                assert!(!found_after.contains(*old), "{name}: search finds {old}");
            }
        }
        assert!(searched_pairs > 0, "{name}: no moved file was searchable");

        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::Undo,
        );
        press(
            &mut second_app,
            &second_ctx,
            &second_access,
            answering,
            &ChipReply::PutBack,
        );
        shoot(&second_app, &format!("{name}-9-undone"));
        assert_eq!(
            files_on_disk(&root),
            before,
            "{name}: undo puts every file back"
        );
        assert_eq!(
            searched(&slot, &root),
            found_before,
            "{name}: search after undo"
        );
        for (old, _) in &removed {
            assert!(
                index_knows(&slot, &root.join(old)),
                "{name}: index lacks {old}"
            );
        }
    }
    model.shutdown();
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_changed_levels_and_the_dry_run_are_shot_for_each_change() {
    let model = local_model();
    let answering: &dyn ChatModel = model.as_ref();

    for (case, german_sentence, english_sentence) in CASES {
        for (lang, suffix, sentence) in [
            (Language::German, "de", german_sentence),
            (Language::English, "en", english_sentence),
        ] {
            let name = &format!("{case}-{suffix}");
            let (_dir, mut app, ctx, access, _slot) =
                opened_pay_slips(&model, lang, answering, name);
            press(&mut app, &ctx, &access, answering, &ChipReply::LevelsChange);
            if case.starts_with("report") {
                type_sentence(&mut app, &ctx, &access, answering, "3");
                shoot(&app, &format!("{name}-0-bare-level"));
            }
            type_sentence(&mut app, &ctx, &access, answering, sentence);
            assert!(
                has_chip(&app, &ChipReply::LevelsApply),
                "{name}: {:?}",
                app.hermes_chat.chips()
            );
            shoot(&app, &format!("{name}-1-change"));
            press(&mut app, &ctx, &access, answering, &ChipReply::LevelsApply);
            assert!(has_chip(&app, &ChipReply::RuleCreate), "{name}: no dry run");
            shoot(&app, &format!("{name}-2-dry-run"));
        }
    }
    model.shutdown();
}

/// Every file of the archive and the inbox with its content, by path below
/// the root.
fn files_on_disk(root: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(relative, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    for folder in ["archive", "inbox"] {
        walk(root, &root.join(folder), &mut out);
    }
    out
}

/// What the search of the app finds for the words of the pay slips, as paths
/// below the root. The search reads the index, as the app's search does.
fn searched(
    slot: &std::sync::Arc<Mutex<Option<Index>>>,
    root: &std::path::Path,
) -> std::collections::BTreeSet<String> {
    let access = ncrs_filing::query_work::QueryAccess::new(std::sync::Arc::clone(slot));
    ["Entgeltabrechnung", "Gehaltsabrechnung"]
        .iter()
        .flat_map(|word| {
            let spec = ncrs_index::QuerySpec {
                full_text: Some((*word).to_owned()),
                ..ncrs_index::QuerySpec::default()
            };
            access.run(&spec).unwrap()
        })
        .map(|hit| {
            assert!(!hit.missing, "the search finds a gone file: {:?}", hit.path);
            hit.path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn index_knows(slot: &std::sync::Arc<Mutex<Option<Index>>>, path: &std::path::Path) -> bool {
    slot.lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .document_by_path(path)
        .unwrap()
        .is_some()
}
