use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chrono::NaiveDate;
use ncrs_filing::filing::ActivityBatch;
use ncrs_filing::review_work::{ReviewItem, TextState};
use ncrs_hermes_ui::activity_page::ActivityPage;
use ncrs_hermes_ui::reviews_page::{Answer, Question, ReviewsPage};
use ncrs_index::{
    BatchKind, DocumentFields, DocumentId, Field, FieldOrigins, KindLanguage, Kinds, ReviewEntryId,
    ReviewReason,
};

use super::*;
use crate::messages::{ActivityMessage, ReviewsMessage};

const WINDOW: iced::Size = iced::Size::new(1740.0, 1130.0);
const SMALL_WINDOW: iced::Size = iced::Size::new(1280.0, 800.0);
const WAITING: u8 = 120;

fn simulator_in(app: &App, size: iced::Size) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        size,
        app.view(),
    )
}

/// Ids the index has given: they cannot be made up outside it.
fn review_ids(count: u8) -> Vec<(ReviewEntryId, DocumentId)> {
    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{ContentHash, DocumentSource, Index, NewDocument};

    let dir = tempfile::TempDir::new().unwrap();
    let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    (1..=count)
        .map(|seed| {
            let document = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: dir.path().join(format!("scan-{seed}.pdf")),
                    source: DocumentSource::Inbox,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: None,
                })
                .unwrap();
            let entry = index
                .add_review(document, ReviewReason::NoMatchingRule)
                .unwrap();
            (entry, document)
        })
        .collect()
}

/// What a document of a wave looks like: where it is, what Hermes read.
struct Wave {
    reason: ReviewReason,
    sender: Option<&'static str>,
    count: usize,
    /// The patterns the documents are filed with, one after the other.
    patterns: &'static [&'static str],
    /// Name of the scan on the disk for the `at`-th document.
    name: fn(usize) -> String,
    /// Year and month Hermes read, if it read one.
    date: fn(usize) -> Option<(i32, u32)>,
}

fn month_of(at: usize) -> u32 {
    (at as u32 - 1) % 12 + 1
}

fn item(ids: (ReviewEntryId, DocumentId), wave: &Wave, at: usize) -> ReviewItem {
    let date =
        (wave.date)(at).map(|(year, month)| NaiveDate::from_ymd_opt(year, month, 9).unwrap());
    let pattern = wave.patterns[at % wave.patterns.len()];
    ReviewItem {
        entry: ids.0,
        document_id: ids.1,
        path: PathBuf::from(format!("/Users/test/Scans/Eingang/{}", (wave.name)(at))),
        reason: wave.reason,
        text_state: TextState::Read,
        fields: DocumentFields {
            sender: wave.sender.map(str::to_owned),
            kind: Some("invoice".to_owned()),
            date,
            ..DocumentFields::default()
        },
        proposed_pattern: (!pattern.is_empty()).then(|| pattern.to_owned()),
        origins: FieldOrigins::default(),
        passages: Vec::new(),
        placement: None,
    }
}

/// 120 documents in six questions, the ones where no rule fits are last.
fn waiting_documents() -> Vec<ReviewItem> {
    let waves = [
        Wave {
            reason: ReviewReason::UnconfirmedRule,
            sender: Some("Stadtwerke Musterhausen"),
            count: 31,
            patterns: &["Wohnung/Nebenkosten/2025/{year}-{month} Stadtwerke"],
            name: |at| format!("Rechnung Stadtwerke {:02}-2025.pdf", month_of(at)),
            date: |at| Some((2025, month_of(at))),
        },
        Wave {
            reason: ReviewReason::LowConfidence(Field::Sender),
            sender: Some("Telekom Deutschland"),
            count: 24,
            patterns: &[
                "Telefon/2025/{year}-{month} Telekom",
                "Telefon/2025/{year}-{month} Telekom",
                "Telefon/Verträge/{year}-{month} Telekom",
            ],
            name: |at| format!("Rechnung Telekom {:02}-2025.pdf", month_of(at)),
            date: |at| Some((2025, month_of(at))),
        },
        Wave {
            reason: ReviewReason::MissingField(Field::Date),
            sender: Some("Versicherung Nordstern"),
            count: 19,
            patterns: &["Versicherung/Nordstern/Kfz/{year}-{month} Beitrag"],
            name: |at| format!("Scan_{:04}.pdf", 412 + at),
            date: |_| None,
        },
        Wave {
            reason: ReviewReason::TargetExists,
            sender: Some("Sparkasse Musterstadt"),
            count: 17,
            patterns: &["Bank/Sparkasse/Kontoauszüge/{year}/{year}-{month} Kontoauszug"],
            name: |at| {
                format!(
                    "Kontoauszug_{}-{:02}.pdf",
                    if at <= 12 { 2024 } else { 2023 },
                    month_of(at)
                )
            },
            date: |at| Some((if at <= 12 { 2024 } else { 2023 }, month_of(at))),
        },
        Wave {
            reason: ReviewReason::MissingField(Field::Amount),
            sender: Some("Finanzamt Musterstadt"),
            count: 9,
            patterns: &["Steuer/2024/{year} Bescheid"],
            name: |at| format!("Bescheid Einkommensteuer 2024 Seite {at}.pdf"),
            date: |_| Some((2024, 6)),
        },
        Wave {
            reason: ReviewReason::NoMatchingRule,
            sender: None,
            count: 20,
            patterns: &[""],
            name: |at| format!("Scan_{:04}.pdf", 460 + at),
            date: |_| Some((2025, 1)),
        },
    ];
    let mut ids = review_ids(WAITING).into_iter();
    let mut items = Vec::new();
    for wave in &waves {
        for at in 1..=wave.count {
            items.push(item(ids.next().unwrap(), wave, at));
        }
    }
    items
}

fn reviews_app(size: iced::Size) -> App {
    let mut page = ReviewsPage::new();
    page.loaded(waiting_documents(), &Kinds::built_in(KindLanguage::German));
    page.toggle(Question::TargetTaken);
    App::with_reviews(page)
        .in_language(Language::German)
        .with_window_size_for_test(size)
}

/// Where documents are filed below the archive, the year and what they are
/// called there.
const TARGETS: [(&str, u32, &str); 5] = [
    ("Wohnung/Nebenkosten/2025", 2025, "Stadtwerke"),
    ("Telefon/2025", 2025, "Telekom"),
    ("Bank/Sparkasse/Kontoauszüge/2024", 2024, "Kontoauszug"),
    ("Versicherung/Nordstern/Kfz", 2025, "Beitrag"),
    ("Steuer/2024", 2024, "Bescheid"),
];

fn journal() -> Vec<ActivityBatch> {
    let inbox = "/Users/test/Scans/Eingang";
    let archive = "/Users/test/Documents/Archiv";
    let batches = [
        (BatchKind::Filing, 4),
        (BatchKind::Hermes, 7),
        (BatchKind::Filing, 8),
        (BatchKind::Reorganisation, 8),
        (BatchKind::Filing, 5),
        (BatchKind::Filing, 8),
        (BatchKind::Hermes, 6),
        (BatchKind::Filing, 7),
    ];
    batches
        .into_iter()
        .enumerate()
        .map(|(number, (kind, count))| ActivityBatch {
            kind,
            at: 1_800_000_000 - number as i64 * 5 * 3600,
            undone: number == 6,
            moves: (0..count)
                .map(|at| {
                    let (folder, year, label) = TARGETS[(number + at) % TARGETS.len()];
                    let month = (number * 5 + at * 2) % 12 + 1;
                    let name = format!("{year}-{month:02} {label}.pdf");
                    let from = if kind == BatchKind::Reorganisation {
                        format!("{archive}/Sortieren/{name}")
                    } else {
                        format!("{inbox}/Scan_{:04}.pdf", 400 + number * 9 + at)
                    };
                    (
                        PathBuf::from(from),
                        PathBuf::from(format!("{archive}/{folder}/{name}")),
                    )
                })
                .collect(),
        })
        .collect()
}

fn activity_app() -> App {
    let mut page = ActivityPage::new().in_utc();
    page.loaded(journal());
    let mut snapshot = ncrs_pipeline::Snapshot::off();
    snapshot.state = ncrs_pipeline::RunnerState::Running;
    snapshot.worker_target = 2;
    snapshot.steps = vec![ncrs_pipeline::StepSnapshot {
        name: "probe",
        waiting: 12,
        done: 0,
        failed: 3,
        unchanged: 0,
    }];
    let counts = ncrs_index::HermesCounts {
        open_reviews: u64::from(WAITING),
        ..ncrs_index::HermesCounts::default()
    };
    let taken_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    App::with_activity(page, snapshot, taken_at)
        .in_language(Language::German)
        .with_window_size_for_test(WINDOW)
        .with_hermes_counts(counts)
}

fn matches_reference(app: &App, size: iced::Size, name: &str) {
    let matches = simulator_in(app, size)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the page does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_reviews_page_matches_its_snapshots() {
    matches_reference(&reviews_app(WINDOW), WINDOW, "reviews_page_de");
    matches_reference(
        &reviews_app(SMALL_WINDOW),
        SMALL_WINDOW,
        "reviews_page_small_de",
    );
}

#[test]
fn the_activity_page_matches_its_snapshot() {
    matches_reference(&activity_app(), WINDOW, "activity_page_de");
}

#[test]
fn a_group_answers_its_question_with_one_button_and_unfolds_to_its_files() {
    let app = reviews_app(WINDOW);
    for (label, expected) in [
        (
            "Regel übernehmen",
            ReviewsMessage::Answer(Question::RuleUnconfirmed, Answer::CreateRule),
        ),
        (
            "Ja, ablegen",
            ReviewsMessage::Answer(Question::Sender, Answer::Accept),
        ),
        (
            "Später",
            ReviewsMessage::Answer(Question::RuleUnconfirmed, Answer::Later),
        ),
    ] {
        let mut sim = simulator_in(&app, WINDOW);
        sim.click(label).expect("the button of a group");
        let messages: Vec<Message> = sim
            .into_messages()
            .filter(is_action)
            .map(without_focus)
            .collect();
        assert_eq!(messages, [Message::Reviews(expected)]);
    }
    assert!(simulator_in(&app, WINDOW).find("Datei zeigen").is_ok());
}

#[test]
fn the_activity_numbers_open_the_pages_they_count() {
    let app = activity_app();
    for (label, expected) in [
        ("Rückfragen (120)", ActivityMessage::OpenReviews),
        ("Lesefehler (3)", ActivityMessage::OpenReadErrors),
    ] {
        let mut sim = simulator_in(&app, WINDOW);
        sim.click(label).expect("the link");
        let messages: Vec<Message> = sim
            .into_messages()
            .filter(is_action)
            .map(without_focus)
            .collect();
        assert_eq!(messages, [Message::Activity(expected)]);
    }
}
