use std::sync::Mutex;

use ncrs_filing::chat_work::ChatAccess;
use ncrs_hermes_chat::{AreaRow, AreasCard, Availability, ChipReply, CountRow, Ctx, Part};
use ncrs_index::Index;
use ncrs_intake::TextChain;

use super::hermes_chat_ui_tests::{chip, hermes_says, matches_snapshot, ready, send, user_says};
use crate::app::*;
use crate::i18n::Language;

fn count_row(text: &str, documents: u32) -> CountRow {
    CountRow {
        text: text.to_owned(),
        documents,
    }
}

fn area_row(name: &str, description: &str, example: &str, documents: u32) -> AreaRow {
    AreaRow {
        name: name.to_owned(),
        description: description.to_owned(),
        example: example.to_owned(),
        documents,
    }
}

fn english_areas_bubbles(app: &mut App, card: AreasCard) {
    user_says(app, "Downloads");
    hermes_says(
        app,
        vec![
            Part::Text(
                "Done reading: **3779 files** in **20 areas**. This is the order I see:".to_owned(),
            ),
            Part::Areas(card),
            Part::Text("Shall I sort new files in the same way?".to_owned()),
        ],
        vec![
            chip("Yes, carry on like this", ChipReply::KeepYes, true),
            chip("Change something", ChipReply::LevelsChange, false),
        ],
    );
}

fn long_names_card() -> AreasCard {
    AreasCard {
        rows: vec![
            area_row(
                "Taxes",
                "year",
                "Taxes/2023/2023-04-12_Finanzamt.pdf",
                148,
            ),
            area_row(
                "Idle_Game_Concept_Discussion_with_the_whole_team_2025",
                "sender",
                "Idle_Game_Concept_Discussion_with_the_whole_team_2025/Anna/Notes_2025-03-02.pdf",
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
                area_row("Taxes", "year", "Taxes/2023/2023-04-12_Finanzamt.pdf", 148),
                area_row(
                    "Insurance",
                    "insurer",
                    "Insurance/Allianz/Policy_2022.pdf",
                    96,
                ),
                area_row("Flat", "subfolder", "Flat/Lease/Lease_2021.pdf", 61),
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

/// The test archive keeps its text in files named like PDFs.
#[derive(Debug)]
struct Utf8Text;

impl ncrs_intake::TextSource for Utf8Text {
    fn extract(&self, path: &Path) -> Result<ncrs_intake::Extracted, ncrs_intake::TextError> {
        let bytes = std::fs::read(path).map_err(|err| ncrs_intake::TextError::Unreadable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => ncrs_intake::Extracted::Text(text),
            Err(_) => ncrs_intake::Extracted::NotApplicable,
        })
    }
}

fn canonical_folder(folder: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(folder).map_err(|err| err.to_string())
}

/// The areas card of the real flow: the greeting's archive chip pressed, the
/// test archive read by a real chat access.
pub(in crate::app::tests) fn real_flow_areas_app(lang: Language) -> App {
    let dir = tempfile::tempdir().unwrap();
    ncrs_intake::test_archive::test_archive()
        .write_to(dir.path())
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let access = ChatAccess::new(
        Arc::new(Mutex::new(Some(index))),
        Arc::new(TextChain::new().with_source(Utf8Text)),
        Arc::new(ncrs_fs::JournaledTrash::new(
            dir.path().join("trash.toml"),
            ncrs_fs::FolderBackend(bin),
        )),
    );
    let ctx = Ctx {
        availability: Availability::Ready(Box::new(access.clone())),
        targets: vec![root.join("archive")],
        sources: vec![root.join("inbox")],
        resolve_folder: canonical_folder,
        home: root.clone(),
        ..ready(lang)
    };
    let mut app = App::with_fixed_panels().in_language(lang);
    send(&mut app, &ctx, HermesChatMessage::Open);
    // The archive is not a filing folder yet when it is picked: the chat reads
    // it for the order it keeps, and shows it as a card of areas.
    let unlearned = Ctx {
        targets: Vec::new(),
        ..ctx
    };
    let press = |chat_app: &mut App, reply: ChipReply| {
        let chip = chat_app
            .hermes_chat
            .chips()
            .iter()
            .find(|chip| chip.reply == reply)
            .unwrap()
            .clone();
        let mut next = Some(HermesChatMessage::ChipPressed {
            label: chip.label,
            reply: chip.reply,
        });
        while let Some(message) = next.take() {
            drop(ncrs_hermes_chat::update(
                &mut chat_app.hermes_chat,
                message,
                &unlearned,
            ));
            next = chat_app
                .hermes_chat
                .pending_work()
                .cloned()
                .map(|work| HermesChatMessage::Done(Box::new(access.run(work))));
        }
    };
    send(
        &mut app,
        &unlearned,
        HermesChatMessage::FolderPicked(Some(root.join("archive"))),
    );
    press(&mut app, ChipReply::RoleTarget);
    app
}

#[test]
fn the_areas_card_of_the_real_flow_on_the_test_archive() {
    matches_snapshot(
        &real_flow_areas_app(Language::English),
        "hermes_chat_areas_flow_en",
    );
}
