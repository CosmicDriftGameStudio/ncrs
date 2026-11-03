//! Numbers on the test archive, from the logic the app has today.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ncrs_index::{
    ContentHash, DocumentFields, FindingKind, KindCatalog, KindLanguage, Kinds, NoActionReason,
    PlanOutcome, SenderAliases,
};
use ncrs_intake::test_archive::{test_archive, TestFile, OWN_NAME};
use ncrs_intake::{
    detect_fields, learn_archive, location_structure, plan_reorganisation, render_pattern_in,
    ArchiveDocument, NeighbourIndex, OwnNames, PlanFile, ReorganisationInput,
};
use sha2::{Digest as _, Sha256};

struct FieldCounts {
    sender: usize,
    recipient: usize,
    kind: usize,
    date: usize,
}

fn own_names(names: &[String]) -> OwnNames<'_> {
    OwnNames {
        names,
        ibans: &[],
        tax_ids: &[],
    }
}

fn field_counts(files: &[TestFile]) -> FieldCounts {
    let aliases = SenderAliases::default();
    let catalog = KindCatalog::built_in();
    let names = [OWN_NAME.to_owned()];
    let own = own_names(&names);
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

/// The fields the pipeline would store for the file: what the letter says,
/// and the date the test archive wrote into it when the letter has none.
fn detected_fields(file: &TestFile) -> DocumentFields {
    let names = [OWN_NAME.to_owned()];
    let found = file.text().map(|text| {
        detect_fields(
            text,
            &DocumentFields::default(),
            &SenderAliases::default(),
            &[],
            &own_names(&names),
            &KindCatalog::built_in(),
        )
    });
    DocumentFields {
        date: found.as_ref().and_then(|fields| fields.date).or(file.date),
        sender: found.as_ref().and_then(|fields| fields.sender.clone()),
        kind: found.and_then(|fields| fields.kind),
        ..DocumentFields::default()
    }
}

fn count_photos(files: &[TestFile]) -> usize {
    files.iter().filter(|file| file.text().is_none()).count()
}

#[derive(Default)]
struct PlacementCounts {
    with_target: usize,
    sure_without_pattern: usize,
    unsure: usize,
    missing_field: usize,
    no_neighbours: usize,
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
    let catalog = KindCatalog::built_in();
    let plan_files: Vec<PlanFile> = archive
        .archive
        .iter()
        .zip(0..)
        .map(|(file, recorded_at)| PlanFile {
            relative_path: file.relative_path.clone(),
            hash: ContentHash(Sha256::digest(file.bytes()).into()),
            fields: detected_fields(file),
            text: file.text().map(str::to_owned),
            recorded_at,
        })
        .collect();
    let structure = location_structure(&plan_files, &catalog);
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

    let existing: BTreeSet<PathBuf> = archive
        .archive
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    let kinds = Kinds::built_in(KindLanguage::English);
    let items = plan_reorganisation(&ReorganisationInput {
        files: &plan_files,
        profile: &profile,
        unordered: &structure,
        rules: &[],
        existing: &existing,
        kinds: &kinds,
        sender_aliases: &SenderAliases::default(),
    });
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
        "== SORT AGAIN: {} items; by finding: {}; by outcome: move={} rename={} conflict={} no_action={} duplicate_copy={}",
        items.len(),
        by_finding.join(" "),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Move { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Rename { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::Conflict { .. })),
        outcome_count(|outcome| matches!(outcome, PlanOutcome::NoAction(_))),
        outcome_count(|outcome| matches!(
            outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy)
        )),
    );

    let names = [OWN_NAME.to_owned()];
    let neighbours = NeighbourIndex::new(&plan_files, &own_names(&names), &structure);
    let mut placements = PlacementCounts::default();
    for file in &archive.inbox {
        let inbox_fields = detected_fields(file);
        match neighbours.place(&inbox_fields, file.text()) {
            None => placements.no_neighbours += 1,
            Some(placement) if !placement.is_sure() => placements.unsure += 1,
            Some(placement) if placement.missing.is_some() => placements.missing_field += 1,
            Some(placement) => {
                let rendered = placement
                    .pattern
                    .as_deref()
                    .and_then(|pattern| render_pattern_in(pattern, &inbox_fields, &kinds));
                if rendered.is_some() {
                    placements.with_target += 1;
                } else {
                    placements.sure_without_pattern += 1;
                }
            }
        }
    }
    let inbox = field_counts(&archive.inbox);
    println!(
        "== INBOX: {} files, {} photos; sender {}, recipient {}, kind {}, date {}; with target path: {}, sure without pattern: {}, unsure: {}, missing field: {}, no neighbours: {}",
        archive.inbox.len(),
        count_photos(&archive.inbox),
        inbox.sender,
        inbox.recipient,
        inbox.kind,
        inbox.date,
        placements.with_target,
        placements.sure_without_pattern,
        placements.unsure,
        placements.missing_field,
        placements.no_neighbours,
    );
}
