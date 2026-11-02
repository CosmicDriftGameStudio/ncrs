#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use ncrs_index::{ContentHash, KindCatalog, KindLanguage, SenderAliases};
use sha2::{Digest as _, Sha256};

use super::*;
use crate::archive_profile::learn_archive;
use crate::field_detection::{detect_fields, OwnNames};
use crate::reorganisation_plan::location_structure;
use crate::test_archive::{test_archive, OWN_NAME};

const DOWNLOADS_FOLDER: &str = "Downloads alt";
const DOWNLOAD_COPIES: usize = 40;

fn plan_of_the_test_archive() -> Vec<PlanItem> {
    let catalog = KindCatalog::built_in();
    let names = [OWN_NAME.to_owned()];
    let own = OwnNames {
        names: &names,
        ibans: &[],
        tax_ids: &[],
    };
    let files: Vec<PlanFile> = test_archive()
        .archive
        .iter()
        .zip(0..)
        .map(|(file, recorded_at)| {
            let found = file.text().map(|text| {
                detect_fields(
                    text,
                    &DocumentFields::default(),
                    &SenderAliases::default(),
                    &[],
                    &own,
                    &catalog,
                )
            });
            PlanFile {
                relative_path: file.relative_path.clone(),
                hash: ContentHash(Sha256::digest(file.bytes()).into()),
                fields: DocumentFields {
                    date: found
                        .as_ref()
                        .and_then(|detected| detected.date)
                        .or(file.date),
                    sender: found.as_ref().and_then(|detected| detected.sender.clone()),
                    kind: found.and_then(|detected| detected.kind),
                    ..DocumentFields::default()
                },
                text: file.text().map(str::to_owned),
                recorded_at,
            }
        })
        .collect();
    let documents: Vec<ArchiveDocument> = files
        .iter()
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
        })
        .collect();
    let existing: BTreeSet<PathBuf> = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    plan_reorganisation(&ReorganisationInput {
        files: &files,
        profile: &learn_archive(&documents),
        unordered: &location_structure(&files, &catalog),
        rules: &[],
        existing: &existing,
        kinds: &Kinds::built_in(KindLanguage::English),
    })
}

#[test]
fn nothing_is_moved_into_a_duplicates_folder_and_every_download_copy_is_noted() {
    let items = plan_of_the_test_archive();

    for item in &items {
        if let PlanOutcome::Move { target } | PlanOutcome::Rename { target } = &item.outcome {
            assert!(!target.starts_with("duplicates"), "{item:?}");
        }
    }
    let downloads: Vec<&PlanItem> = items
        .iter()
        .filter(|item| item.source.starts_with(DOWNLOADS_FOLDER))
        .collect();
    assert_eq!(downloads.len(), DOWNLOAD_COPIES);
    for item in downloads {
        assert_eq!(
            item.outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy),
            "{item:?}"
        );
        let Some(Finding::Duplicate { keeper }) = item.findings.first() else {
            panic!("{item:?}");
        };
        assert!(!keeper.starts_with(DOWNLOADS_FOLDER), "{item:?}");
    }
}

#[test]
fn copies_filed_under_the_tax_year_are_no_duplicates() {
    let items = plan_of_the_test_archive();

    let copies: Vec<&PlanItem> = items
        .iter()
        .filter(|item| {
            item.source.starts_with("Steuer") && item.source.to_string_lossy().contains("Belege")
        })
        .filter(|item| {
            item.findings
                .iter()
                .any(|finding| finding.kind() == FindingKind::Duplicate)
        })
        .collect();

    assert!(copies.is_empty(), "{copies:?}");
}

#[test]
fn a_tax_notice_dated_after_its_year_folder_is_no_year_mismatch() {
    let items = plan_of_the_test_archive();

    let notices: Vec<&PlanItem> = items
        .iter()
        .filter(|item| item.source.to_string_lossy().contains("Bescheid/"))
        .filter(|item| item.source.starts_with("Steuer"))
        .collect();

    for item in notices {
        assert!(
            item.findings
                .iter()
                .all(|finding| finding.kind() != FindingKind::YearMismatch),
            "{item:?}"
        );
    }
}
