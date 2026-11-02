use std::sync::Mutex;

use chrono::TimeZone as _;
use iced_test::Simulator;
use ncrs_filing::chat_work::{ChatAccess, ChatResult, RunSummary};
use ncrs_hermes_chat::{
    AreaRow, AreasCard, Availability, Chip, ChipReply, Ctx, Entry, FileCard, Part, RowOutcome,
    TrialCard, TrialRow,
};
use ncrs_index::{BatchKind, Index, Transfer};
use ncrs_intake::TextChain;

use crate::app::*;
use crate::i18n::Language;

const NONE: Modifiers = Modifiers::empty();

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
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
        model: None,
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
    let row = |name: &str, description: &str, documents| AreaRow {
        name: name.to_owned(),
        description: description.to_owned(),
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
                    row("Steuern", "nach Jahr", 148),
                    row("Versicherungen", "nach Anbieter", 96),
                    row("Wohnung", "nach Thema", 61),
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
