//! The real app flow with the real local model: a folder opened by a typed
//! sentence, its levels, a described rule with its trial, the click that
//! stores it, and the rule on the Rules page. Needs the model:
//! `NCRS_AI_MODELS_DIR` names the folder with it. Screenshots go to
//! `target/acceptance/`.

#![cfg(feature = "local-ai")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::num::NonZeroU32;
use std::sync::Mutex;

use iced_test::Simulator;
use ncrs_ai::{ChatModel, LazyLocalModel, LOCAL_MODEL};
use ncrs_filing::chat_work::ChatAccess;
use ncrs_filing::rules_work::RulesAccess;
use ncrs_hermes_chat::{Availability, ChipReply, Ctx, Entry, Part};
use ncrs_index::{DocumentFields, Index, KindLanguage};
use ncrs_intake::{Extracted, TextChain, TextError, TextSource};
use sha2::Digest as _;

use crate::app::*;
use crate::i18n::Language;
use crate::rules_page::RulesPage;

const MODELS_DIR_VARIABLE: &str = "NCRS_AI_MODELS_DIR";
const AREA: &str = "Bank/Sparkasse Chemnitz/Kontoauszüge";
const MAX_DOCUMENTS: usize = 90;
const MAX_STATEMENTS: usize = 30;
const SHOT_DIR: &str = "target/acceptance";
const SHOT_SIZE: iced::Size = iced::Size::new(1740.0, 1130.0);

/// The test archive keeps its text in files named like PDFs.
#[derive(Debug)]
struct Utf8Text;

impl TextSource for Utf8Text {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        let bytes = std::fs::read(path).map_err(|err| TextError::Unreadable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => Extracted::Text(text),
            Err(_) => Extracted::NotApplicable,
        })
    }
}

fn canonical_folder(folder: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(folder).map_err(|err| err.to_string())
}

fn shoot(app: &App, name: &str) -> PathBuf {
    std::fs::create_dir_all(SHOT_DIR).unwrap();
    let path = PathBuf::from(SHOT_DIR).join(format!("{name}-tiny-skia.png"));
    drop(std::fs::remove_file(&path));
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
    .matches_image(PathBuf::from(SHOT_DIR).join(format!("{name}.png")))
    .unwrap();
    assert!(path.is_file(), "{} was not written", path.display());
    path
}

/// What the app does with the chat's pending work, on the spot.
fn settle(app: &mut App, ctx: &Ctx, access: &ChatAccess, model: &dyn ChatModel) {
    loop {
        let chat = &app.hermes_chat;
        let next = if let Some(work) = chat.pending_work().cloned() {
            HermesChatMessage::Done(Box::new(access.run(work)))
        } else if let Some(naming) = chat.pending_naming() {
            naming.resolve(ctx.resolve_folder)
        } else if let Some((typed, step)) = chat.pending_resolve() {
            HermesChatMessage::TypedFolderResolved {
                typed: typed.to_path_buf(),
                step,
                result: (ctx.resolve_folder)(typed),
            }
        } else if let Some(ask) = chat.pending_rule_ask().cloned() {
            HermesChatMessage::RuleAnswered(ask.run(model))
        } else if let Some(turn) = chat.pending_turn().cloned() {
            HermesChatMessage::Understood(model.understand(&turn).map_err(|err| err.to_string()))
        } else {
            return;
        };
        drop(ncrs_hermes_chat::update(&mut app.hermes_chat, next, ctx));
    }
}

fn has_chip(app: &App, reply: &ChipReply) -> bool {
    app.hermes_chat
        .chips()
        .iter()
        .any(|chip| &chip.reply == reply)
}

fn press(app: &mut App, ctx: &Ctx, access: &ChatAccess, model: &dyn ChatModel, reply: &ChipReply) {
    let chip = app
        .hermes_chat
        .chips()
        .iter()
        .find(|chip| &chip.reply == reply)
        .unwrap_or_else(|| panic!("no chip {reply:?} in {:?}", app.hermes_chat.chips()))
        .clone();
    drop(ncrs_hermes_chat::update(
        &mut app.hermes_chat,
        HermesChatMessage::ChipPressed {
            label: chip.label,
            reply: chip.reply,
        },
        ctx,
    ));
    settle(app, ctx, access, model);
}

fn type_sentence(app: &mut App, ctx: &Ctx, access: &ChatAccess, model: &dyn ChatModel, text: &str) {
    for message in [
        HermesChatMessage::InputChanged(text.to_owned()),
        HermesChatMessage::Submit,
    ] {
        drop(ncrs_hermes_chat::update(&mut app.hermes_chat, message, ctx));
    }
    settle(app, ctx, access, model);
}

fn levels_card_shown(app: &App) -> bool {
    matches!(
        app.hermes_chat.thread.last(),
        Some(Entry::Hermes { parts, .. }) if parts.iter().any(|part| matches!(part, Part::Levels(_)))
    )
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_the_app_makes_a_rule_from_a_typed_folder_and_a_sentence() {
    let models = PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = models.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    let model = LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    );

    let dir = tempfile::tempdir().unwrap();
    let archive = ncrs_intake::test_archive::test_archive();
    archive.write_to(dir.path()).unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let slot = Arc::new(Mutex::new(Some(
        Index::create_plain(&dir.path().join("index.db")).unwrap(),
    )));
    let (mut indexed, mut kept_statements, mut kept_others): (usize, usize, usize) = (0, 0, 0);
    for file in &archive.archive {
        let Some(text) = file.text() else { continue };
        let path = root.join("archive").join(&file.relative_path);
        let relative = file.relative_path.to_string_lossy();
        let sparkasse = format!("archive/{relative}").contains(AREA);
        // A tree of Marc's size: the statements, and other files up to the cap.
        let kept = if sparkasse {
            kept_statements += 1;
            kept_statements <= MAX_STATEMENTS
        } else {
            kept_others += 1;
            kept_others <= MAX_DOCUMENTS - MAX_STATEMENTS
        };
        if !kept {
            std::fs::remove_file(&path).unwrap();
            continue;
        }
        indexed += 1;
        let mut guard = slot.lock().unwrap();
        let index = guard.as_mut().unwrap();
        let hash: [u8; 32] = sha2::Sha256::digest(path.to_string_lossy().as_bytes()).into();
        let id = index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash(hash),
                path,
                source: ncrs_index::DocumentSource::Archive,
                received_at: chrono::Utc::now(),
                stamp: None,
            })
            .unwrap();
        index.set_document_text(id, text).unwrap();
        index
            .set_document_fields(
                id,
                &DocumentFields {
                    sender: sparkasse.then(|| "Sparkasse Chemnitz".to_owned()),
                    kind: sparkasse.then(|| "Kontoauszug".to_owned()),
                    date: file.date,
                    ..DocumentFields::default()
                },
                |_| ncrs_index::FieldOrigin::Manual,
            )
            .unwrap();
    }
    println!("documents in the test archive: {indexed}");
    assert!((50..=100).contains(&indexed), "{indexed} documents");

    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let trash =
        ncrs_fs::JournaledTrash::new(dir.path().join("trash.toml"), ncrs_fs::FolderBackend(bin));
    let location = ncrs_intake::Location::new(
        &root.join("archive"),
        ncrs_intake::LocationMode::Archive,
        false,
    )
    .unwrap();
    let rules = RulesAccess::new(Arc::clone(&slot), vec![location])
        .with_kind_language(KindLanguage::German);
    let access = ChatAccess::new(
        Arc::clone(&slot),
        Arc::new(TextChain::new().with_source(Utf8Text)),
        Arc::new(trash),
    )
    .with_kind_language(KindLanguage::German)
    .with_rules(Some(rules.clone()));
    let ctx = Ctx {
        availability: Availability::Ready(Box::new(access.clone())),
        targets: vec![root.join("archive")],
        sources: vec![root.join("inbox")],
        resolve_folder: canonical_folder,
        home: root.clone(),
        model: Some(Arc::clone(&model) as Arc<dyn ChatModel>),
        ..super::hermes_chat_ui_tests::ready(Language::German)
    };
    let answering: &dyn ChatModel = model.as_ref();

    let mut app = App::with_fixed_panels().in_language(Language::German);
    super::hermes_chat_ui_tests::send(&mut app, &ctx, HermesChatMessage::Open);
    type_sentence(&mut app, &ctx, &access, answering, "öffne mal Kontoauszüge");
    for named in app
        .hermes_chat
        .chips()
        .iter()
        .filter_map(|chip| match &chip.reply {
            ChipReply::NamedFolder(path) if path.to_string_lossy().contains(AREA) => {
                Some(chip.reply.clone())
            }
            _ => None,
        })
        .take(1)
        .collect::<Vec<_>>()
    {
        press(&mut app, &ctx, &access, answering, &named);
    }
    assert!(
        levels_card_shown(&app),
        "{:?}",
        app.hermes_chat.thread.last()
    );
    let levels = shoot(&app, "hermes_rule_1_levels");

    let keep = if has_chip(&app, &ChipReply::LevelsKeep) {
        ChipReply::LevelsKeep
    } else {
        ChipReply::LevelsDefault
    };
    press(&mut app, &ctx, &access, answering, &keep);
    type_sentence(
        &mut app,
        &ctx,
        &access,
        answering,
        "Alle Kontoauszüge der Sparkasse Chemnitz",
    );
    assert!(has_chip(&app, &ChipReply::RuleCreate), "no proposal");
    let proposal = shoot(&app, "hermes_rule_2_proposal");

    press(&mut app, &ctx, &access, answering, &ChipReply::RuleCreate);
    let stored = slot.lock().unwrap().as_ref().unwrap().rules().unwrap();
    assert_eq!(stored.len(), 1, "{stored:?}");
    let rule = stored.first().unwrap();
    assert!(!rule.spec.conditions.list.is_empty(), "{rule:?}");
    assert!(
        rule.spec
            .filing_pattern
            .starts_with("Bank/Sparkasse Chemnitz/Kontoauszüge/"),
        "{rule:?}"
    );
    assert!(!rule.spec.filing_pattern.contains("//"), "{rule:?}");

    let mut page = RulesPage::new();
    page.loaded(rules.load().unwrap());
    assert_eq!(page.selected().map(|row| row.rule.id), Some(rule.id));
    let settings = App::with_rules(page).in_language(Language::German);
    let page_shot = shoot(&settings, "hermes_rule_3_rules_page");

    println!(
        "screenshots:\n{}\n{}\n{}",
        levels.display(),
        proposal.display(),
        page_shot.display()
    );
    model.shutdown();
}
