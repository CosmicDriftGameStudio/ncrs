#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use ncrs_index::{DocumentFields, KindCatalog, SenderAliases};

use super::*;
use crate::test_archive::{test_archive, OWN_NAME};
use crate::{detect_fields, OwnNames};

fn archive_structure() -> (LocationStructure, Vec<StructureDocument>) {
    let aliases = SenderAliases::default();
    let catalog = KindCatalog::built_in();
    let names = [OWN_NAME.to_owned()];
    let own = OwnNames {
        names: &names,
        ibans: &[],
        tax_ids: &[],
    };
    let documents: Vec<StructureDocument> = test_archive()
        .archive
        .iter()
        .map(|file| {
            let found = file.text().map(|text| {
                detect_fields(
                    text,
                    &DocumentFields::default(),
                    &aliases,
                    &[],
                    &own,
                    &catalog,
                )
            });
            StructureDocument {
                relative_path: file.relative_path.clone(),
                date: found.as_ref().and_then(|fields| fields.date).or(file.date),
                sender: found.as_ref().and_then(|fields| fields.sender.clone()),
                kind: found.and_then(|fields| fields.kind),
            }
        })
        .collect();
    (learn_structure(&documents, &catalog), documents)
}

fn area<'a>(structure: &'a LocationStructure, folder: &str) -> &'a Area {
    structure
        .areas
        .iter()
        .find(|area| area.folder == folder)
        .unwrap_or_else(|| panic!("no area {folder}: {:#?}", structure.describe()))
}

fn literal(name: &str) -> Level {
    Level::Literal(name.to_owned())
}

fn is_placeholder(level: &Level, label: PlaceholderLabel) -> bool {
    matches!(level, Level::Placeholder(placeholder) if placeholder.label == label)
}

fn choice_of(level: &Level) -> Option<&[String]> {
    match level {
        Level::Choice(names) => Some(names),
        _ => None,
    }
}

fn all_branches(structure: &LocationStructure) -> impl Iterator<Item = &Branch> {
    structure.areas.iter().flat_map(|area| &area.branches)
}

#[test]
fn the_areas_and_the_unordered_folders_are_the_top_folders() {
    let (structure, _) = archive_structure();

    let mut areas: Vec<&str> = structure.areas.iter().map(|a| a.folder.as_str()).collect();
    areas.sort_unstable();
    assert_eq!(
        areas,
        [
            "Arbeit",
            "Auto",
            "Bank",
            "Fotos Belege",
            "Gesundheit",
            "Kinder",
            "Steuer",
            "Versicherung",
            "Wohnung"
        ]
    );
    let unordered: Vec<&str> = structure
        .unordered
        .iter()
        .map(|folder| folder.folder.as_str())
        .collect();
    assert_eq!(unordered, ["Downloads alt", "Scans"]);
    assert!(structure.is_unordered(Path::new("Downloads alt/a.pdf")));
    assert!(structure.is_unordered(Path::new("Scans/scan_0001.pdf")));
    assert!(!structure.is_unordered(Path::new("Bank/x/y.pdf")));
    assert!(structure.areas.iter().all(|a| !a.folder.is_empty()));
}

#[test]
fn every_area_has_a_branch_with_a_name_scheme_or_a_placeholder() {
    let (structure, _) = archive_structure();

    for area in &structure.areas {
        assert!(
            area.branches.iter().any(|branch| branch.name.is_some()
                || branch
                    .levels
                    .iter()
                    .any(|level| matches!(level, Level::Placeholder(_)))),
            "{}: {:#?}",
            area.folder,
            structure.describe()
        );
    }
}

#[test]
fn bank_has_a_branch_per_account_and_the_financing_folders() {
    let (structure, _) = archive_structure();
    let bank = area(&structure, "Bank");

    let find = |first: &str| {
        bank.branches
            .iter()
            .find(|branch| branch.levels.first() == Some(&literal(first)))
            .unwrap_or_else(|| panic!("no {first}: {:#?}", structure.describe()))
    };
    let sparkasse = find("Sparkasse Chemnitz");
    assert_eq!(sparkasse.levels.get(1), Some(&literal("Kontoauszüge")));
    assert!(sparkasse
        .levels
        .get(2)
        .is_some_and(|level| is_placeholder(level, PlaceholderLabel::Year)));
    assert_eq!(
        sparkasse.name.as_ref().map(|scheme| scheme.parts.clone()),
        Some(vec![
            NamePart::Literal("Kontoauszug".to_owned()),
            NamePart::Separator('_'),
            NamePart::Date(DateFormat::Iso),
        ])
    );
    assert_eq!((sparkasse.documents, sparkasse.following), (84, 84));

    let scalable = find("Scalable Capital");
    assert_eq!(
        scalable.name.as_ref().map(|scheme| scheme.parts.clone()),
        Some(vec![
            NamePart::Literal("Wertpapierabrechnung".to_owned()),
            NamePart::Separator('_'),
            NamePart::Date(DateFormat::Iso),
            NamePart::Separator('_'),
            NamePart::Number,
        ])
    );
    assert_eq!(scalable.documents, 150);

    let dkb = find("DKB");
    assert_eq!(dkb.levels.get(1), Some(&literal("Kreditkarte")));
    assert_eq!(
        dkb.name.as_ref().map(|scheme| scheme.parts.clone()),
        Some(vec![
            NamePart::Literal("Kreditkartenabrechnung".to_owned()),
            NamePart::Separator(' '),
            NamePart::Date(DateFormat::Dotted),
        ])
    );
    assert_eq!(dkb.documents, 36);

    let financing = find("Finanzierung");
    assert!(financing
        .levels
        .get(1)
        .is_some_and(|level| is_placeholder(level, PlaceholderLabel::Property)));
    let stages = financing.levels.get(2).and_then(choice_of).unwrap();
    assert_eq!(stages, ["Entwurf", "Korrespondenz", "Unterschrieben"]);
    assert_eq!(financing.documents, 84);
}

#[test]
fn insurers_are_a_placeholder_over_a_closed_list_of_subfolders() {
    let (structure, _) = archive_structure();
    let branch = area(&structure, "Versicherung")
        .branches
        .iter()
        .find(|branch| !branch.levels.is_empty())
        .unwrap();

    let Some(Level::Placeholder(insurer)) = branch.levels.first() else {
        panic!("{:#?}", structure.describe());
    };
    assert_eq!(insurer.label, PlaceholderLabel::Insurer);
    assert!(!insurer.examples.is_empty());
    assert!(insurer.examples.iter().all(|example| [
        "Allianz",
        "ERGO",
        "HUK-Coburg",
        "Kranich Versicherung"
    ]
    .contains(&example.as_str())));
    let mut topics = branch.levels.get(1).and_then(choice_of).unwrap().to_vec();
    topics.sort();
    assert_eq!(topics, ["Beitragsrechnung", "Schaden", "Vertrag"]);
}

#[test]
fn tax_is_a_year_and_a_choice_of_subfolders() {
    let (structure, _) = archive_structure();

    let found = area(&structure, "Steuer").branches.iter().any(|branch| {
        branch
            .levels
            .first()
            .is_some_and(|level| is_placeholder(level, PlaceholderLabel::Year))
            && branch
                .levels
                .get(1)
                .and_then(choice_of)
                .is_some_and(|names| {
                    ["Belege", "Bescheid", "Erklärung"]
                        .iter()
                        .all(|name| names.iter().any(|candidate| candidate == name))
                })
    });
    assert!(found, "{:#?}", structure.describe());
}

#[test]
fn the_stages_of_the_financing_are_never_a_placeholder() {
    let (structure, _) = archive_structure();

    for branch in all_branches(&structure) {
        for level in &branch.levels {
            if let Level::Placeholder(placeholder) = level {
                assert!(
                    placeholder.examples.iter().all(|name| ![
                        "Entwurf",
                        "Unterschrieben",
                        "Korrespondenz"
                    ]
                    .contains(&name.as_str())),
                    "{placeholder:?}"
                );
            }
        }
    }
}

#[test]
fn no_branch_repeats_a_label_or_has_a_digits_only_constant_or_a_false_kind() {
    let (structure, _) = archive_structure();
    let catalog = KindCatalog::built_in();

    for branch in all_branches(&structure) {
        let mut labels: Vec<PlaceholderLabel> = Vec::new();
        for level in &branch.levels {
            match level {
                Level::Placeholder(placeholder) => {
                    assert!(!labels.contains(&placeholder.label), "{branch:?}");
                    labels.push(placeholder.label);
                    if placeholder.kind == PlaceholderKind::Kind {
                        assert!(placeholder
                            .examples
                            .iter()
                            .all(|name| catalog.resolve(name).is_some()));
                    }
                }
                Level::Literal(name) => assert!(!is_digits(name), "{name}"),
                Level::Choice(names) => {
                    assert!(names.iter().all(|name| !is_digits(name)), "{names:?}");
                }
            }
        }
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

#[test]
fn no_branch_or_area_has_an_empty_folder() {
    let (structure, _) = archive_structure();

    for branch in all_branches(&structure) {
        for level in &branch.levels {
            match level {
                Level::Literal(name) => assert!(!name.is_empty()),
                Level::Choice(names) => assert!(names.iter().all(|name| !name.is_empty())),
                Level::Placeholder(_) => {}
            }
        }
    }
}

#[test]
fn files_in_the_location_root_are_an_unordered_folder() {
    let documents: Vec<StructureDocument> = ["a.pdf", "b.pdf"]
        .into_iter()
        .map(|name| StructureDocument {
            relative_path: name.into(),
            date: None,
            sender: None,
            kind: None,
        })
        .collect();

    let structure = learn_structure(&documents, &KindCatalog::built_in());

    assert!(structure.areas.is_empty());
    assert_eq!(
        structure.unordered,
        [UnorderedFolder {
            folder: String::new(),
            documents: 2
        }]
    );
    assert!(structure.is_unordered(Path::new("c.pdf")));
}
