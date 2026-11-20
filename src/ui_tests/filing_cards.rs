use super::*;

fn plain_file_card() -> App {
    App::with_fixed_filing(Phase::PlainFileWarning)
}

/// Compared with the plain panels rendered here and now, not with the
/// committed reference: that one was made on macOS and is not comparable
/// elsewhere, where a card would count as not drawn.
fn differs_from_the_plain_panels(app: &App, plain_panels: &std::path::Path) -> bool {
    !simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_hash(plain_panels)
        .expect("compare with the panels")
}

#[test]
fn every_filing_card_is_drawn_over_the_panels() {
    let cards = [
        Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 3,
            typed: SecretText::default(),
            mismatch: false,
        },
        Phase::AskRecoveryKey {
            reason: KeyProblem::Missing,
            typed: SecretText::default(),
            confirm_replace: false,
            busy: false,
            error: None,
        },
        Phase::AskRecoveryKey {
            reason: KeyProblem::Mismatch,
            typed: SecretText::default(),
            confirm_replace: false,
            busy: false,
            error: None,
        },
        Phase::PlainFileWarning,
    ];
    let folder = tempfile::tempdir().expect("a folder for the plain panels");
    let plain_panels = folder.path().join("plain_panels");
    assert!(
        simulator(&App::with_fixed_panels())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash(&plain_panels)
            .expect("the first rendering writes the hash"),
        "the plain panels must be recorded"
    );
    for phase in cards {
        let name = format!("{phase:?}");
        assert!(
            differs_from_the_plain_panels(&App::with_fixed_filing(phase), &plain_panels),
            "{name} is not drawn"
        );
    }
}

#[test]
fn the_master_key_card_matches_its_snapshot() {
    let app = App::with_fixed_filing(Phase::ShowRecoveryKey {
        key: FilingModule::varied_recovery_key(),
        group: 1,
        typed: SecretText::default(),
        mismatch: false,
    });
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/recovery_key.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the card does not match tests/snapshots/recovery_key.png"
    );
}

#[test]
fn the_master_key_card_matches_its_german_snapshot() {
    let app = App::with_fixed_filing(Phase::ShowRecoveryKey {
        key: FilingModule::varied_recovery_key(),
        group: 1,
        typed: SecretText::default(),
        mismatch: false,
    })
    .in_language(Language::German);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/recovery_key_de.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the card does not match tests/snapshots/recovery_key_de.png"
    );
}

fn ask_recovery_key(reason: KeyProblem) -> App {
    App::with_fixed_filing(Phase::AskRecoveryKey {
        reason,
        typed: SecretText::default(),
        confirm_replace: false,
        busy: false,
        error: None,
    })
}

fn shows(app: &App, label: &str) -> bool {
    simulator(app).find(label).is_ok()
}

const MISSING_EXPLANATION: &str = "The key for the index is not in the system keystore. Enter the recovery key to open the index.";
const MISMATCH_EXPLANATION: &str = "A key is stored, but it does not open this index. The stored key is replaced only if the recovery key you enter opens the index.";
const REPLACE_CHECKBOX: &str = "Replace the stored key";

#[test]
fn each_key_card_shows_its_own_prompt() {
    let confirmation = App::with_fixed_filing(Phase::ShowRecoveryKey {
        key: FilingModule::fixed_recovery_key(),
        group: 3,
        typed: SecretText::default(),
        mismatch: false,
    });
    assert!(shows(
        &confirmation,
        "Type block 4 (the highlighted four characters) to show that you saved the key."
    ));
    assert!(shows(&confirmation, "Copy"));

    let restore = ask_recovery_key(KeyProblem::Missing);
    assert!(shows(&restore, MISSING_EXPLANATION));
    assert!(!shows(&restore, MISMATCH_EXPLANATION));
    assert!(!shows(&restore, REPLACE_CHECKBOX));

    let mismatch = ask_recovery_key(KeyProblem::Mismatch);
    assert!(shows(&mismatch, MISMATCH_EXPLANATION));
    assert!(shows(&mismatch, REPLACE_CHECKBOX));
    assert!(!shows(&mismatch, MISSING_EXPLANATION));
}

#[test]
fn the_plain_file_card_confirms_with_its_button() {
    let app = plain_file_card();
    let mut sim = simulator(&app);
    sim.click("Use this file")
        .expect("the Use this file button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert_eq!(messages, [Message::Filing(FilingMessage::ConfirmPlainFile)]);
}

fn messages_for_a_click_on_a_panel_row(app: &App) -> Vec<Message> {
    let mut sim = simulator(app);
    // Directories are drawn with a leading slash.
    sim.click("/Documents").expect("a panel row");
    sim.into_messages().filter(is_action).collect()
}

#[test]
fn a_double_click_on_a_panel_row_activates_it() {
    let app = App::with_fixed_panels();
    let mut sim = simulator(&app);
    sim.click("/Documents").expect("a panel row");
    sim.click("/Documents").expect("a panel row");
    let messages: Vec<Message> = sim.into_messages().filter(is_action).collect();
    assert!(
        matches!(
            messages.as_slice(),
            [
                Message::Panel(PanelMessage::RowClicked { .. }),
                Message::Panel(PanelMessage::RowClicked { .. }),
                Message::Panel(PanelMessage::RowActivated { .. })
            ]
        ),
        "{messages:?}"
    );
}

#[test]
fn the_panels_stay_usable_while_a_card_is_up() {
    let without_card = messages_for_a_click_on_a_panel_row(&App::with_fixed_panels());
    assert!(
        matches!(
            without_card.as_slice(),
            [Message::Panel(PanelMessage::RowClicked { .. })]
        ),
        "the click misses the row: {without_card:?}"
    );

    let with_card = messages_for_a_click_on_a_panel_row(&plain_file_card());
    assert_eq!(with_card, without_card);
}
