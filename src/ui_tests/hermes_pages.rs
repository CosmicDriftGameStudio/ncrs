use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ncrs_filing::filing::ActivityBatch;
use ncrs_hermes_ui::activity_page::ActivityPage;
use ncrs_index::BatchKind;

use super::*;
use crate::messages::ActivityMessage;

const WINDOW: iced::Size = iced::Size::new(1740.0, 1130.0);
const WAITING: usize = 120;

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
    let taken_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    App::with_activity(page, snapshot, taken_at)
        .in_language(Language::German)
        .with_window_size_for_test(WINDOW)
        .with_questions(WAITING)
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
fn the_activity_page_matches_its_snapshot() {
    matches_reference(&activity_app(), WINDOW, "activity_page_de");
}

#[test]
fn the_activity_numbers_open_the_pages_they_count() {
    let app = activity_app();
    for (label, expected) in [
        ("120 warten auf dich", ActivityMessage::OpenWaiting),
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
