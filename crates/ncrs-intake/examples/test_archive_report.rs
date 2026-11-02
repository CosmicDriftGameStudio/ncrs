//! Numbers on the test archive, from the logic the app has today.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ncrs_index::{
    ContentHash, DocumentFields, FindingKind, KindCatalog, KindLanguage, Kinds, PlanOutcome,
    SenderAliases,
};
use ncrs_intake::test_archive::{test_archive, TestFile, OWN_NAME};
use ncrs_intake::{
    detect_fields, learn_archive, learn_structure, plan_reorganisation, ArchiveDocument, OwnNames,
    PlanFile, ReorganisationInput, StructureDocument, DUPLICATES_FOLDER,
};
use sha2::{Digest as _, Sha256};

struct FieldCounts {
    sender: usize,
    recipient: usize,
    kind: usize,
    date: usize,
}

fn field_counts(files: &[TestFile]) -> FieldCounts {
    let aliases = SenderAliases::default();
    let catalog = KindCatalog::built_in();
    let names = [OWN_NAME.to_owned()];
    let own = OwnNames {
        names: &names,
        ibans: &[],
        tax_ids: &[],
    };
    let mut counts = FieldCounts {
        sender: 0,
        recipient: 0,
        kind: 0,
        date: 0,
    };
    for text in files.iter().filter_map(TestFile::text) {
        let found = detect_fields(
            text,
            &DocumentFields::default(),
            &aliases,
            &[],
            &own,
            &catalog,
        );
        counts.sender += usize::from(found.sender.is_some());
        counts.recipient += usize::from(found.recipient.is_some());
        counts.kind += usize::from(found.kind.is_some());
        counts.date += usize::from(found.date.is_some());
    }
    counts
}

fn structure_documents(files: &[TestFile]) -> Vec<StructureDocument> {
    let aliases = SenderAliases::default();
    let catalog = KindCatalog::built_in();
    let names = [OWN_NAME.to_owned()];
    let own = OwnNames {
        names: &names,
        ibans: &[],
        tax_ids: &[],
    };
    files
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
        .collect()
}

fn count_photos(files: &[TestFile]) -> usize {
    files.iter().filter(|file| file.text().is_none()).count()
}

fn main() {
    let archive = test_archive();
    let texts = archive.archive.len() - count_photos(&archive.archive);
    println!(
        "== ARCHIVE: {} files, {texts} with text, {} photos",
        archive.archive.len(),
        count_photos(&archive.archive)
    );

    let documents: Vec<ArchiveDocument> = archive
        .archive
        .iter()
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.date,
        })
        .collect();
    let profile = learn_archive(&documents);
    let fields = field_counts(&archive.archive);
    let structure = learn_structure(
        &structure_documents(&archive.archive),
        &KindCatalog::built_in(),
    );
    println!(
        "== AREAS: {} areas, {} unordered folders",
        structure.areas.len(),
        structure.unordered.len()
    );
    for line in structure.describe() {
        println!("  {line}");
    }
    println!(
        "== PROFILE: {} subtrees, {} with a pattern, {} documents follow",
        profile.len(),
        profile
            .iter()
            .filter(|entry| entry.pattern.is_some())
            .count(),
        profile.iter().map(|entry| entry.documents).sum::<u32>()
    );

    println!(
        "== FIELDS (archive texts): sender {}, recipient {}, kind {}, date {} of {texts}",
        fields.sender, fields.recipient, fields.kind, fields.date
    );

    let plan_files: Vec<PlanFile> = archive
        .archive
        .iter()
        .zip(0..)
        .map(|(file, recorded_at)| PlanFile {
            relative_path: file.relative_path.clone(),
            hash: ContentHash(Sha256::digest(file.bytes()).into()),
            fields: Default::default(),
            text: file.text().map(str::to_owned),
            recorded_at,
        })
        .collect();
    let existing: BTreeSet<PathBuf> = archive
        .archive
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    let items = plan_reorganisation(&ReorganisationInput {
        files: &plan_files,
        profile: &profile,
        rules: &[],
        existing: &existing,
        kinds: &Kinds::built_in(KindLanguage::English),
    });
    let moved_to_duplicates = items
        .iter()
        .filter(|item| {
            matches!(&item.outcome, PlanOutcome::Move { target } if target.starts_with(DUPLICATES_FOLDER))
        })
        .count();
    let by_finding: Vec<String> = [
        FindingKind::Duplicate,
        FindingKind::Misplaced,
        FindingKind::YearMismatch,
        FindingKind::NameOutlier,
    ]
    .into_iter()
    .map(|kind| {
        let count = items
            .iter()
            .flat_map(|item| &item.findings)
            .filter(|finding| finding.kind() == kind)
            .count();
        format!("{}={count}", kind.as_str())
    })
    .collect();
    let outcome_count = |matches_outcome: fn(&PlanOutcome) -> bool| {
        items
            .iter()
            .filter(|item| matches_outcome(&item.outcome))
            .count()
    };
    println!(
        "== SORT AGAIN: {} items; duplicates moved into {DUPLICATES_FOLDER}/: {moved_to_duplicates}; by finding: {}; by outcome: move={} rename={} conflict={} no_action={}",
        items.len(),
        by_finding.join(" "),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Move { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Rename { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Conflict { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::NoAction(_))),
    );

    let inbox = field_counts(&archive.inbox);
    println!(
        "== INBOX: {} files, {} photos; sender {}, recipient {}, kind {}, date {}; with target path: 0",
        archive.inbox.len(),
        count_photos(&archive.inbox),
        inbox.sender,
        inbox.recipient,
        inbox.kind,
        inbox.date
    );
}
