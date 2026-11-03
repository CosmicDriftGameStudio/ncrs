use std::sync::Mutex;

use chrono::TimeZone as _;
use iced_test::Simulator;
use ncrs_ai::{ChatIntent, Compute, LocalModelStatus, ScriptedChatModel};
use ncrs_filing::chat_work::{ChatAccess, ChatResult, RuleSaveFailure, RunSummary};
use ncrs_hermes_chat::{
    AreaRow, AreasCard, Availability, Chip, ChipReply, CountRow, Ctx, Entry, FileCard, Part,
    RowOutcome, Step, TrialCard, TrialRow,
};
use ncrs_index::{BatchKind, Index, Transfer};
use ncrs_intake::{RuleProblem, TextChain};

use crate::app::*;
use crate::i18n::Language;

const NONE: Modifiers = Modifiers::empty();

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

fn matches_snapshot(app: &App, name: &str) {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the chat does not match tests/snapshots/{name}.png"
    );
}

fn ready(lang: Language) -> Ctx {
    let access = ChatAccess::new(
        Arc::new(Mutex::new(None)),
        Arc::new(TextChain::new()),
        fs::system_trash(None),
    );
    Ctx {
        lang,
        availability: Availability::Ready(access),
        home: PathBuf::from("/home/test"),
        targets: vec![PathBuf::from("/home/test/Documents/Archive")],
        sources: vec![PathBuf::from("/home/test/Downloads")],
        ai_on: true,
        ocr_models_present: true,
        model: None,
        favourites: Vec::new(),
        resolve_folder: |path| Ok(path.to_path_buf()),
        now: chrono::Local
            .with_ymd_and_hms(2026, 11, 2, 12, 0, 0)
            .unwrap(),
    }
}

fn send(app: &mut App, ctx: &Ctx, message: HermesChatMessage) {
    drop(ncrs_hermes_chat::update(&mut app.hermes_chat, message, ctx));
}

fn type_and_submit(app: &mut App, ctx: &Ctx, text: &str) {
    send(app, ctx, HermesChatMessage::InputChanged(text.to_owned()));
    send(app, ctx, HermesChatMessage::Submit);
}

fn opened(lang: Language) -> (App, Ctx) {
    let mut app = App::with_fixed_panels().in_language(lang);
    let ctx = ready(lang);
    send(&mut app, &ctx, HermesChatMessage::Open);
    (app, ctx)
}

fn pick<'a>(lang: Language, german: &'a str, english: &'a str) -> &'a str {
    match lang {
        Language::German | Language::GermanFuturama => german,
        Language::English | Language::EnglishFuturama => english,
    }
}

fn segments(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn chip(label: &str, reply: ChipReply, primary: bool) -> Chip {
    Chip {
        label: label.to_owned(),
        reply,
        primary,
    }
}

fn hermes_says(app: &mut App, parts: Vec<Part>, chips: Vec<Chip>) {
    app.hermes_chat.thread.push(Entry::Hermes { parts, chips });
}

fn user_says(app: &mut App, text: &str) {
    app.hermes_chat.thread.push(Entry::User(text.to_owned()));
}

fn areas_scene(lang: Language) -> App {
    let (mut app, _) = opened(lang);
    user_says(&mut app, pick(lang, "Mein Archiv", "My archive"));
    let row = |name: &str, description: &str, example: &str, documents| AreaRow {
        name: name.to_owned(),
        description: description.to_owned(),
        example: example.to_owned(),
        documents,
    };
    hermes_says(
        &mut app,
        vec![
            Part::Text(
                pick(
                    lang,
                    "Ich habe **412 Dokumente** gelesen. So ist es geordnet:",
                    "I read **412 documents**. This is how it is ordered:",
                )
                .to_owned(),
            ),
            Part::Areas(AreasCard {
                rows: vec![
                    row(
                        "Steuern",
                        "Jahr / Datei",
                        "Steuern/2023/2023-04-12_Finanzamt.pdf",
                        148,
                    ),
                    row(
                        "Versicherungen",
                        "Anbieter / Datei",
                        "Versicherungen/Allianz/Police_2022.pdf",
                        96,
                    ),
                    row("Wohnung", "Datei", "Wohnung/Mietvertrag.pdf", 61),
                ],
                more: None,
                unordered: None,
            }),
        ],
        vec![
            chip(
                pick(lang, "Passt so", "Fine as it is"),
                ChipReply::FineAsIs,
                true,
            ),
            chip(
                pick(lang, "Etwas ändern", "Change something"),
                ChipReply::WantChange,
                false,
            ),
        ],
    );
    app
}

fn change_scene(lang: Language) -> App {
    let (mut app, _) = opened(lang);
    user_says(&mut app, pick(lang, "Steuern nach Monat", "Taxes by month"));
    hermes_says(
        &mut app,
        vec![
            Part::Text(
                pick(
                    lang,
                    "So würde ein Beispiel unter **Steuern** liegen:",
                    "This is where an example would sit under **Taxes**:",
                )
                .to_owned(),
            ),
            Part::File(FileCard {
                segments: segments(&["Archiv", "Steuern", "2025", "11", "Bescheid.pdf"]),
                highlighted: Some(3),
            }),
        ],
        vec![
            chip(
                pick(lang, "So machen", "Do it this way"),
                ChipReply::Always,
                true,
            ),
            chip(
                pick(lang, "Lieber nicht", "Better not"),
                ChipReply::Later,
                false,
            ),
        ],
    );
    app
}

fn trial_scene(lang: Language) -> App {
    let (mut app, _) = opened(lang);
    let unsure = |entry, name: &str| TrialRow {
        entry,
        name: name.to_owned(),
        outcome: RowOutcome::Unsure {
            question: pick(lang, "Wohin damit?", "Where does it go?").to_owned(),
            choices: vec![
                chip(
                    "Steuern",
                    ChipReply::UnsureChoice { entry, choice: 0 },
                    false,
                ),
                chip(
                    "Wohnung",
                    ChipReply::UnsureChoice { entry, choice: 1 },
                    false,
                ),
            ],
            chosen: None,
        },
    };
    hermes_says(
        &mut app,
        vec![
            Part::Text(
                pick(
                    lang,
                    "Probelauf: **5 Dateien** aus Downloads, nichts wird verschoben.",
                    "Trial: **5 files** from Downloads, nothing is moved.",
                )
                .to_owned(),
            ),
            Part::Trial(TrialCard {
                header: pick(lang, "Aus Downloads", "From Downloads").to_owned(),
                rows: vec![
                    TrialRow {
                        entry: 0,
                        name: "Bescheid.pdf".to_owned(),
                        outcome: RowOutcome::Filed {
                            segments: segments(&["Steuern", "2025", "Bescheid.pdf"]),
                        },
                    },
                    TrialRow {
                        entry: 1,
                        name: "Police.pdf".to_owned(),
                        outcome: RowOutcome::AlreadyThere(
                            pick(lang, "liegt schon im Archiv", "is already in the archive")
                                .to_owned(),
                        ),
                    },
                    unsure(2, "Scan_0042.pdf"),
                    TrialRow {
                        entry: 3,
                        name: "notizen.txt".to_owned(),
                        outcome: RowOutcome::Stays(
                            pick(lang, "bleibt liegen", "stays where it is").to_owned(),
                        ),
                    },
                ],
                more: Some(pick(lang, "und 1 weitere", "and 1 more").to_owned()),
            }),
        ],
        vec![
            chip(
                pick(lang, "Alle ablegen", "File all"),
                ChipReply::FileAll,
                true,
            ),
            chip(pick(lang, "Abbrechen", "Cancel"), ChipReply::Cancel, false),
        ],
    );
    app
}

fn group_scene() -> App {
    let (mut app, _) = opened(Language::German);
    user_says(&mut app, "Räum Inbox auf");
    hermes_says(
        &mut app,
        vec![Part::Text(
            "37 Dateien von Vodafone GmbH, z. B. Dokument (100).pdf, Dokument (14).pdf, \
             Dokument (16).pdf. Wohin damit?"
                .to_owned(),
        )],
        vec![
            chip("Bank", ChipReply::GroupArea("Bank".to_owned()), false),
            chip("Steuer", ChipReply::GroupArea("Steuer".to_owned()), false),
            chip(
                "Versicherung",
                ChipReply::GroupArea("Versicherung".to_owned()),
                false,
            ),
            chip("Neuer Ordner …", ChipReply::GroupNewFolder, false),
            chip("Liegen lassen", ChipReply::GroupLeave, false),
        ],
    );
    app
}

fn undo_scene() -> App {
    let (mut app, ctx) = opened(Language::German);
    user_says(&mut app, "Mach das rückgängig");
    let folder = tempfile::tempdir().unwrap();
    let index = Index::create_plain(&folder.path().join("index.db")).unwrap();
    let summary = RunSummary {
        batch: index.begin_batch(BatchKind::Hermes).unwrap(),
        name: "Downloads aufräumen".to_owned(),
        transfer: Transfer::Move,
        items: 14,
        finished_at: chrono::Local
            .with_ymd_and_hms(2026, 11, 2, 9, 30, 0)
            .unwrap()
            .timestamp(),
        source_folder: Some(PathBuf::from("/home/test/Downloads")),
    };
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Done(Box::new(ChatResult::LastRun(Ok(Some(summary))))),
    );
    app
}

fn fallback_scene() -> App {
    let (mut app, ctx) = opened(Language::German);
    type_and_submit(&mut app, &ctx, "Wie wird das Wetter morgen?");
    app
}

fn with_model(status: LocalModelStatus) -> (App, Ctx) {
    let (app, mut ctx) = opened(Language::German);
    ctx.model = Some(Arc::new(ScriptedChatModel::new(status, [])));
    (app, ctx)
}

fn tidy_confirmation_scene() -> App {
    let (mut app, ctx) = with_model(LocalModelStatus::Loaded {
        name: "Qwen3-4B-Q4_K_M.gguf".to_owned(),
        memory_bytes: 2_700_000_000,
        compute: Compute::Cpu,
    });
    type_and_submit(&mut app, &ctx, "räum Downloads auf");
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Understood(Ok(ChatIntent::TidyFolder {
            source: "Downloads".to_owned(),
            target: None,
            schedule: None,
            transfer: None,
        })),
    );
    send(
        &mut app,
        &ctx,
        HermesChatMessage::NamedFoldersResolved {
            source: vec![PathBuf::from("/home/test/Downloads")],
            target: None,
        },
    );
    app
}

fn meant_folder_scene() -> App {
    let (mut app, ctx) = with_model(LocalModelStatus::Loaded {
        name: "Qwen3-4B-Q4_K_M.gguf".to_owned(),
        memory_bytes: 2_700_000_000,
        compute: Compute::Cpu,
    });
    type_and_submit(&mut app, &ctx, "verschieb Downloads ins Archiv");
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Understood(Ok(ChatIntent::TidyFolder {
            source: "Downloads".to_owned(),
            target: Some("Archive".to_owned()),
            schedule: None,
            transfer: Some(ncrs_ai::Transfer::Move),
        })),
    );
    send(
        &mut app,
        &ctx,
        HermesChatMessage::NamedFoldersResolved {
            source: vec![PathBuf::from("/home/test/Downloads")],
            target: Some(vec![PathBuf::from("/home/test/Documents/Archive")]),
        },
    );
    app
}

fn which_folder_scene() -> App {
    let (mut app, ctx) = with_model(LocalModelStatus::Loaded {
        name: "Qwen3-4B-Q4_K_M.gguf".to_owned(),
        memory_bytes: 2_700_000_000,
        compute: Compute::Cpu,
    });
    type_and_submit(&mut app, &ctx, "räum Belege auf");
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Understood(Ok(ChatIntent::TidyFolder {
            source: "Belege".to_owned(),
            target: None,
            schedule: None,
            transfer: None,
        })),
    );
    send(
        &mut app,
        &ctx,
        HermesChatMessage::NamedFoldersResolved {
            source: vec![
                PathBuf::from("/home/test/Documents/Belege"),
                PathBuf::from("/home/test/Downloads/Belege"),
            ],
            target: None,
        },
    );
    app
}

fn rule_refused_scene() -> App {
    let (mut app, ctx) = opened(Language::German);
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Done(Box::new(ChatResult::RuleSaved(Err(
            RuleSaveFailure::Refused(RuleProblem::NoFilingTarget),
        )))),
    );
    app
}

fn typed_folder_confirmation_scene() -> App {
    let (mut app, ctx) = opened(Language::German);
    type_and_submit(&mut app, &ctx, "~/Documents/../Documents/Rechnungen");
    send(
        &mut app,
        &ctx,
        HermesChatMessage::TypedFolderResolved {
            typed: PathBuf::from("/home/test/Documents/../Documents/Rechnungen"),
            step: Step::ReadFolder,
            result: Ok(PathBuf::from("/home/test/Documents/Rechnungen")),
        },
    );
    app
}

fn thinking_scene() -> App {
    let (mut app, ctx) = with_model(LocalModelStatus::Loaded {
        name: "Qwen3-4B-Q4_K_M.gguf".to_owned(),
        memory_bytes: 2_700_000_000,
        compute: Compute::Cpu,
    });
    type_and_submit(&mut app, &ctx, "räum Downloads auf");
    app
}

fn waking_scene() -> App {
    let (mut app, ctx) = with_model(LocalModelStatus::NotLoadedYet);
    type_and_submit(&mut app, &ctx, "räum Downloads auf");
    app
}

/// The chat right after opening, with the language model not downloaded.
fn download_offer_scene(lang: Language) -> App {
    let mut app = App::with_fixed_panels().in_language(lang);
    let mut ctx = ready(lang);
    ctx.model = Some(Arc::new(ScriptedChatModel::new(
        LocalModelStatus::NotDownloaded,
        [],
    )));
    send(&mut app, &ctx, HermesChatMessage::Open);
    app
}

/// The chat right after opening, with the AI switched off.
fn ai_off_scene() -> App {
    let mut app = App::with_fixed_panels().in_language(Language::German);
    let mut ctx = ready(Language::German);
    ctx.ai_on = false;
    send(&mut app, &ctx, HermesChatMessage::Open);
    app
}

fn scope_scene() -> App {
    let (mut app, _) = opened(Language::German);
    user_says(&mut app, "Kontoauszüge lieber nach Monat");
    hermes_says(
        &mut app,
        vec![
            Part::Text("Klar. Dann sieht ein Kontoauszug so aus:".to_owned()),
            Part::File(FileCard {
                segments: segments(&["Archiv", "Kontoauszüge", "Sparkasse", "11", "Auszug.pdf"]),
                highlighted: Some(3),
            }),
            Part::Text("Für alle Banken?".to_owned()),
        ],
        vec![
            chip("Alle Banken", ChipReply::ScopeAll, true),
            chip(
                "Nur Sparkasse",
                ChipReply::ScopeOnly("Sparkasse".to_owned()),
                false,
            ),
            chip("Abbrechen", ChipReply::Cancel, false),
        ],
    );
    app
}

fn needs_setup_scene() -> App {
    let mut app = App::with_fixed_panels().in_language(Language::German);
    let mut ctx = ready(Language::German);
    ctx.availability = Availability::NeedsSetup;
    send(&mut app, &ctx, HermesChatMessage::Open);
    app
}

#[test]
fn the_greeting_matches_its_snapshots() {
    for (lang, name) in [
        (Language::German, "hermes_chat_greeting_de"),
        (Language::English, "hermes_chat_greeting_en"),
    ] {
        matches_snapshot(&opened(lang).0, name);
    }
}

#[test]
fn the_areas_matches_its_snapshot() {
    matches_snapshot(&areas_scene(Language::German), "hermes_chat_areas_de");
}

#[test]
fn the_change_preview_matches_its_snapshot() {
    matches_snapshot(&change_scene(Language::German), "hermes_chat_change_de");
}

#[test]
fn the_trial_with_an_unsure_row_matches_its_snapshots() {
    matches_snapshot(&trial_scene(Language::German), "hermes_chat_trial_de");
    matches_snapshot(&trial_scene(Language::English), "hermes_chat_trial_en");
}

#[test]
fn the_question_about_a_group_of_staying_files_matches_its_snapshot() {
    matches_snapshot(&group_scene(), "hermes_chat_group_de");
}

#[test]
fn the_undo_question_matches_its_snapshot() {
    matches_snapshot(&undo_scene(), "hermes_chat_undo_de");
}

#[test]
fn the_fallback_matches_its_snapshot() {
    matches_snapshot(&fallback_scene(), "hermes_chat_fallback_de");
}

#[test]
fn the_setup_hint_matches_its_snapshot() {
    matches_snapshot(&needs_setup_scene(), "hermes_chat_needs_setup_de");
}

#[test]
fn the_tidy_confirmation_matches_its_snapshot() {
    matches_snapshot(&tidy_confirmation_scene(), "hermes_chat_confirm_tidy_de");
}

#[test]
fn the_typed_folder_confirmation_matches_its_snapshot() {
    matches_snapshot(
        &typed_folder_confirmation_scene(),
        "hermes_chat_confirm_typed_folder_de",
    );
}

#[test]
fn the_meant_folder_question_matches_its_snapshot() {
    matches_snapshot(&meant_folder_scene(), "hermes_chat_confirm_meant_folder_de");
}

#[test]
fn the_which_folder_question_matches_its_snapshot() {
    matches_snapshot(&which_folder_scene(), "hermes_chat_which_folder_de");
}

#[test]
fn the_refused_rule_matches_its_snapshot() {
    matches_snapshot(&rule_refused_scene(), "hermes_chat_rule_refused_de");
}

#[test]
fn the_thinking_line_matches_its_snapshot() {
    matches_snapshot(&thinking_scene(), "hermes_chat_thinking_de");
}

#[test]
fn the_waking_line_matches_its_snapshot() {
    matches_snapshot(&waking_scene(), "hermes_chat_waking_de");
}

#[test]
fn the_download_offer_matches_its_snapshot() {
    matches_snapshot(
        &download_offer_scene(Language::German),
        "hermes_chat_download_offer_de",
    );
    matches_snapshot(
        &download_offer_scene(Language::English),
        "hermes_chat_download_offer_en",
    );
}

#[test]
fn the_ai_off_offer_matches_its_snapshot() {
    matches_snapshot(&ai_off_scene(), "hermes_chat_ai_off_de");
}

#[test]
fn the_question_for_which_banks_matches_its_snapshot() {
    matches_snapshot(&scope_scene(), "hermes_chat_scope_de");
}

fn open_chat() -> App {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::HermesChat(HermesChatMessage::Open)));
    app
}

#[test]
fn bare_f2_opens_the_chat_and_escape_closes_it() {
    let idle = PromptKeyState::default();
    assert_eq!(
        route_key(&idle, Key::Named(Named::F2), NONE),
        Some(Message::HermesChat(HermesChatMessage::Open))
    );
    let app = open_chat();
    assert!(app.hermes_chat.open);
    let open = PromptKeyState {
        hermes_chat: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&open, Key::Named(Named::F2), NONE),
        Some(Message::HermesChat(HermesChatMessage::Close))
    );
}

#[test]
fn shift_f2_in_the_chat_closes_it_and_focuses_the_command_line() {
    let open = PromptKeyState {
        hermes_chat: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&open, Key::Named(Named::F2), Modifiers::SHIFT),
        Some(Message::HermesChatToCommandLine)
    );
    let mut app = open_chat();
    drop(app.update(Message::HermesChatToCommandLine));
    assert!(!app.hermes_chat.open);
    assert!(app.command_line.is_focused());
}

#[test]
fn other_function_keys_are_swallowed_while_the_chat_is_open() {
    let open = PromptKeyState {
        hermes_chat: true,
        ..PromptKeyState::default()
    };
    assert_eq!(route_key(&open, Key::Named(Named::F5), NONE), None);
}

fn count_row(text: &str, documents: u32) -> CountRow {
    CountRow {
        text: text.to_owned(),
        documents,
    }
}

fn area_row(name: &str, description: &str, documents: u32) -> AreaRow {
    AreaRow {
        name: name.to_owned(),
        description: description.to_owned(),
        example: String::new(),
        documents,
    }
}

fn english_areas_bubbles(app: &mut App, card: AreasCard) {
    user_says(app, "Downloads");
    hermes_says(
        app,
        vec![
            Part::Text(
                "Done reading: **3779 files**. You already have a good order, I see **20 areas**:"
                    .to_owned(),
            ),
            Part::Areas(card),
            Part::Text("Shall I file new files the same way?".to_owned()),
        ],
        vec![
            chip("Yes, the same way", ChipReply::KeepYes, true),
            chip("Change something", ChipReply::WantChange, false),
        ],
    );
}

fn long_names_card() -> AreasCard {
    AreasCard {
        rows: vec![
            area_row("Taxes", "by year", 148),
            area_row(
                "Idle_Game_Concept_Discussion_with_the_whole_team_2025",
                "by sender",
                1334,
            ),
        ],
        more: Some(count_row(
            "Idle_Game_Concept_Discussion (1), CashColt_ALLE_Logos, dist-chrome-2026.6.1, Archive-2025-12-06-13-45-22 and 13 more",
            1334,
        )),
        unordered: Some(count_row(
            "No fixed order: directly in Downloads, Idle_Game_Concept_Discussion_Final_Version",
            57,
        )),
    }
}

fn long_names_scene() -> App {
    let mut app = App::with_fixed_panels().in_language(Language::English);
    let ctx = ready(Language::English);
    send(&mut app, &ctx, HermesChatMessage::Open);
    english_areas_bubbles(&mut app, long_names_card());
    app
}

/// The area card as Marc sees it: English, the dracula preset applied the way
/// the start applies a configured one.
fn areas_dracula_scene() -> App {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[theme]\npreset = \"dracula\"\n").unwrap();
    let mut app = App::with_fixed_panels()
        .with_config(crate::config::load(&path))
        .in_language(Language::English);
    let ctx = ready(Language::English);
    send(&mut app, &ctx, HermesChatMessage::Open);
    english_areas_bubbles(
        &mut app,
        AreasCard {
            rows: vec![
                area_row("Taxes", "by year", 148),
                area_row("Insurance", "by provider", 96),
                area_row("Flat", "by topic", 61),
            ],
            more: Some(count_row("Cars, Health, Travel, Work and 14 more", 702)),
            unordered: Some(count_row("No fixed order: Misc, Scans", 38)),
        },
    );
    app
}

#[test]
fn the_long_names_stay_inside_the_areas_card() {
    matches_snapshot(&long_names_scene(), "hermes_chat_areas_long_names_en");
}

#[test]
fn the_areas_card_in_the_dracula_preset_uses_one_font() {
    matches_snapshot(&areas_dracula_scene(), "hermes_chat_areas_dracula_en");
}

fn read_downloads() -> Arc<ncrs_filing::chat_work::FolderReading> {
    let file = |name: &str| ncrs_intake::PlanFile {
        relative_path: PathBuf::from(name),
        hash: ncrs_index::ContentHash([0; 32]),
        fields: ncrs_index::DocumentFields::default(),
        text: None,
        recorded_at: 0,
    };
    Arc::new(ncrs_filing::chat_work::FolderReading {
        root: PathBuf::from("/home/test/Downloads"),
        files: vec![file("Taxes/a.pdf"), file("Misc/b.pdf"), file("c.pdf")],
        photos: 0,
        without_text: 0,
        structure: ncrs_intake::LocationStructure {
            areas: vec![ncrs_intake::Area {
                folder: "Taxes".to_owned(),
                documents: 1,
                branches: Vec::new(),
            }],
            unordered: vec![
                ncrs_intake::UnorderedFolder {
                    folder: String::new(),
                    documents: 1,
                },
                ncrs_intake::UnorderedFolder {
                    folder: "Misc".to_owned(),
                    documents: 1,
                },
            ],
        },
        duplicates: Vec::new(),
        hashes: std::collections::HashMap::new(),
    })
}

fn visible_texts(app: &App) -> Vec<String> {
    let mut found = Vec::new();
    for entry in app.hermes_chat.thread() {
        let Entry::Hermes { parts, chips } = entry else {
            continue;
        };
        for part in parts {
            match part {
                Part::Text(text) => found.push(text.clone()),
                Part::Areas(card) => {
                    for row in &card.rows {
                        found.extend([row.name.clone(), row.description.clone()]);
                    }
                    found.extend(
                        card.more
                            .iter()
                            .chain(&card.unordered)
                            .map(|row| row.text.clone()),
                    );
                }
                _ => {}
            }
        }
        found.extend(chips.iter().map(|chip| chip.label.clone()));
    }
    found
}

/// The pieces of a German text between its `{placeholders}` and `**` marks.
fn fixed_pieces(german: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for character in german.replace("**", "").chars() {
        match character {
            '{' => {
                pieces.push(std::mem::take(&mut current));
                inside = true;
            }
            '}' => inside = false,
            _ if !inside => current.push(character),
            _ => {}
        }
    }
    pieces.push(current);
    pieces
        .into_iter()
        .map(|piece| piece.trim().to_owned())
        .filter(|piece| piece.chars().count() >= 8)
        .collect()
}

#[test]
fn an_english_chat_shows_no_german_text_from_the_greeting_to_the_download_offer() {
    let lang = Language::English;
    let mut app = App::with_fixed_panels().in_language(lang);
    let model = Arc::new(ScriptedChatModel::new(
        LocalModelStatus::Loaded {
            name: "test".to_owned(),
            memory_bytes: 1,
            compute: Compute::Cpu,
        },
        [],
    ));
    let mut ctx = ready(lang);
    ctx.model = Some(Arc::clone(&model) as Arc<dyn ncrs_ai::ChatModel>);
    let downloads = PathBuf::from("/home/test/Downloads");
    ctx.sources.clear();
    ctx.targets = vec![downloads.clone()];
    send(&mut app, &ctx, HermesChatMessage::Open);
    let label = app
        .hermes_chat
        .chips()
        .iter()
        .find(|chip| chip.reply == ChipReply::Folder(downloads.clone()))
        .unwrap()
        .label
        .clone();
    send(
        &mut app,
        &ctx,
        HermesChatMessage::ChipPressed {
            label,
            reply: ChipReply::Folder(downloads),
        },
    );
    send(
        &mut app,
        &ctx,
        HermesChatMessage::Done(Box::new(ChatResult::Read(Ok(read_downloads())))),
    );
    model.set_status(LocalModelStatus::NotDownloaded);
    type_and_submit(&mut app, &ctx, "test");

    let shown = visible_texts(&app).join("\n");
    assert!(shown.contains("Done reading"), "{shown}");
    assert!(shown.contains("directly in Downloads"), "{shown}");
    assert!(shown.contains("language model"), "{shown}");
    for msg in ncrs_i18n::ALL_MESSAGES {
        if !msg.key().starts_with("hermes_chat.") {
            continue;
        }
        let english = Language::English.text(*msg);
        for piece in fixed_pieces(Language::German.text(*msg)) {
            assert!(
                english.contains(&piece) || !shown.contains(&piece),
                "{} shows the German {piece:?}:\n{shown}",
                msg.key()
            );
        }
    }

    let mut screen = simulator(&app);
    for header in [
        Msg::HermesChatSubtitle,
        Msg::HermesChatEscHint,
        Msg::HermesChatPlaceholderReply,
    ] {
        screen.find(Language::English.text(header)).unwrap();
        screen.find(Language::German.text(header)).unwrap_err();
    }
}
