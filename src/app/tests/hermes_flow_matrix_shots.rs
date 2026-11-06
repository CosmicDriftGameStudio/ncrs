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

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_changed_levels_and_the_dry_run_are_shot_for_each_change() {
    let models =
        std::path::PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = models.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    let model = LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    );
    let answering: &dyn ChatModel = model.as_ref();

    for (case, german_sentence, english_sentence) in CASES {
        for (lang, suffix, sentence) in [
            (Language::German, "de", german_sentence),
            (Language::English, "en", english_sentence),
        ] {
            let name = &format!("{case}-{suffix}");
            let dir = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(dir.path()).unwrap();
            let slot = std::sync::Arc::new(Mutex::new(Some(
                Index::create_plain(&dir.path().join("index.db")).unwrap(),
            )));
            pay_slip_archive(dir.path(), &slot);
            let bin = dir.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let trash = ncrs_fs::JournaledTrash::new(
                dir.path().join("trash.toml"),
                ncrs_fs::FolderBackend(bin),
            );
            let archive_location = ncrs_intake::Location::new(
                &root.join("archive"),
                ncrs_intake::LocationMode::Archive,
                false,
            )
            .unwrap();
            let inbox_location = ncrs_intake::Location::new(
                &root.join("inbox"),
                ncrs_intake::LocationMode::Inbox,
                false,
            )
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
                model: Some(std::sync::Arc::clone(&model) as std::sync::Arc<dyn ChatModel>),
                ..super::hermes_chat_ui_tests::ready(lang)
            };

            let mut app = App::with_fixed_panels().in_language(lang);
            super::hermes_chat_ui_tests::send(&mut app, &ctx, HermesChatMessage::Open);
            type_sentence(&mut app, &ctx, &access, answering, "öffne mal Arbeit");
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
                press(&mut app, &ctx, &access, answering, reply);
            }
            assert!(
                levels_card_shown(&app),
                "{name}: {:?}",
                app.hermes_chat.thread.last()
            );
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
