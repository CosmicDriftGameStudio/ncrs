//! Where a document belongs by the documents already filed: those that carry
//! the same number, the same IBAN or come from the same sender. Pure: nothing
//! here touches a file or the index.
//!
//! The text of a document is data. It is only searched for numbers and IBANs.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ncrs_index::{compile_bounded_regex, DocumentFields, Field, SenderAliasKind, SubtreePattern};
use regex::Regex;

use crate::archive_profile::{learn_archive, subtree_per_document, ArchiveDocument};
use crate::field_detection::OwnNames;
use crate::filing_plan::missing_field;
use crate::reorganisation::PlanFile;
use crate::sender_detection::found_ibans;
use crate::structure::LocationStructure;

/// Lower-case spelling as written in a letter, and how it is shown.
const NUMBER_LABELS: [(&str, &str); 14] = [
    ("darlehensnummer", "Darlehensnummer"),
    ("versicherungsnummer", "Versicherungsnummer"),
    ("vertragsnummer", "Vertragsnummer"),
    ("kundennummer", "Kundennummer"),
    ("mitgliedsnummer", "Mitgliedsnummer"),
    ("personalnummer", "Personalnummer"),
    ("steuernummer", "Steuernummer"),
    ("kennzeichen", "Kennzeichen"),
    ("aktenzeichen", "Aktenzeichen"),
    ("loan number", "Loan number"),
    ("policy number", "Policy number"),
    ("contract number", "Contract number"),
    ("customer number", "Customer number"),
    ("member number", "Member number"),
];
const TAX_NUMBER_LABEL: &str = "Steuernummer";
const MIN_VALUE_CHARS: usize = 3;
const MIN_NUMBER_NEIGHBOURS: usize = 2;
const MIN_IBAN_NEIGHBOURS: usize = 2;
const MIN_SENDER_NEIGHBOURS: usize = 3;
/// After narrowing a sender's documents to one kind, fewer than the
/// sender tier asks for still decide, as the kind is more specific.
const MIN_NARROWED_NEIGHBOURS: usize = 2;
const SURE_PERCENT: usize = 80;

static LABELLED_NUMBER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let labels: Vec<String> = NUMBER_LABELS
        .iter()
        .map(|(spelling, _)| spelling.replace(' ', r"[ \t]+"))
        .collect();
    compile_bounded_regex(&format!(
        r"(?i)\b({})\b[ \t]*(?:nr\.?)?[ \t]*[:\-]?[ \t]*([A-Za-z0-9][A-Za-z0-9/\-]*(?:[ \t][0-9]{{1,5}}\b)?)",
        labels.join("|")
    ))
    .ok()
});

/// A value a letter states under a label, such as a loan number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LabelledNumber {
    label: &'static str,
    /// Upper case, without spaces.
    value: String,
}

/// What a letter says that other letters may say as well.
#[derive(Debug, Default)]
struct Features {
    numbers: BTreeSet<LabelledNumber>,
    ibans: BTreeSet<String>,
}

#[derive(Debug)]
struct Neighbour {
    folder: PathBuf,
    sender: Option<String>,
    kind: Option<String>,
    features: Features,
    subtree: Option<String>,
}

/// The documents of a location a new document can be compared with. Built
/// once per location.
#[derive(Debug)]
pub struct NeighbourIndex {
    neighbours: Vec<Neighbour>,
    patterns: Vec<SubtreePattern>,
    own_ibans: BTreeSet<String>,
    own_tax_ids: BTreeSet<String>,
}

/// Why a document belongs in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    SameNumber { label: String, value: String },
    SameIban,
    SameSenderAndKind,
    SameSender,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Below the root of the location. For an unsure placement the folder
    /// with most of the similar documents.
    pub folder: PathBuf,
    pub signal: Signal,
    /// Similar documents in `folder` (or below it) and all similar ones.
    pub agreeing: u32,
    pub similar: u32,
    /// What the documents of the folder follow, when they follow one; only
    /// for a sure placement.
    pub pattern: Option<String>,
    /// The field `pattern` needs and the document lacks.
    pub missing: Option<Field>,
}

impl Placement {
    pub fn is_sure(&self) -> bool {
        meets_share(self.agreeing, self.similar)
    }
}

fn meets_share(agreeing: u32, total: u32) -> bool {
    u64::from(agreeing) * 100 >= u64::from(total) * u64::try_from(SURE_PERCENT).unwrap_or(100)
}

fn count(items: usize) -> u32 {
    u32::try_from(items).unwrap_or(u32::MAX)
}

fn labelled_numbers(text: &str, own_tax_ids: &BTreeSet<String>) -> BTreeSet<LabelledNumber> {
    let Some(regex) = LABELLED_NUMBER.as_ref() else {
        return BTreeSet::new();
    };
    regex
        .captures_iter(text)
        .filter_map(|captures| {
            let spelling = captures
                .get(1)?
                .as_str()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let label = NUMBER_LABELS
                .iter()
                .find(|(known, _)| *known == spelling)?
                .1;
            let found = captures.get(2)?.as_str();
            let value = found.split_whitespace().collect::<String>().to_uppercase();
            let long_enough = value.chars().count() >= MIN_VALUE_CHARS;
            let has_digit = value.chars().any(|character| character.is_ascii_digit());
            let own_tax_id = label == TAX_NUMBER_LABEL
                && own_tax_ids.contains(&SenderAliasKind::TaxId.normalise(found));
            (long_enough && has_digit && !own_tax_id).then_some(LabelledNumber { label, value })
        })
        .collect()
}

fn normalised(values: &[String], kind: SenderAliasKind) -> BTreeSet<String> {
    values.iter().map(|value| kind.normalise(value)).collect()
}

impl NeighbourIndex {
    /// Documents in unordered folders are not neighbours, and of copies with
    /// one hash only the one the index saw first counts.
    pub fn new(documents: &[PlanFile], own: &OwnNames<'_>, structure: &LocationStructure) -> Self {
        let own_ibans = normalised(own.ibans, SenderAliasKind::Iban);
        let own_tax_ids = normalised(own.tax_ids, SenderAliasKind::TaxId);
        let mut by_hash: BTreeMap<[u8; 32], &PlanFile> = BTreeMap::new();
        for document in documents
            .iter()
            .filter(|document| !structure.is_unordered(&document.relative_path))
        {
            let seen = |file: &PlanFile| (file.recorded_at, file.relative_path.clone());
            let entry = by_hash.entry(document.hash.0).or_insert(document);
            if seen(document) < seen(entry) {
                *entry = document;
            }
        }
        let mut kept: Vec<&PlanFile> = by_hash.into_values().collect();
        kept.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

        let archive: Vec<ArchiveDocument> = kept
            .iter()
            .map(|document| ArchiveDocument {
                relative_path: document.relative_path.clone(),
                date: document.fields.date,
            })
            .collect();
        let patterns = learn_archive(&archive);
        let subtrees = subtree_per_document(&archive);
        let neighbours = kept
            .iter()
            .zip(subtrees)
            .map(|(document, subtree)| Neighbour {
                folder: document
                    .relative_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
                sender: document.fields.sender.clone(),
                kind: document.fields.kind.clone(),
                features: features_of(document.text.as_deref(), &own_ibans, &own_tax_ids),
                subtree,
            })
            .collect();
        Self {
            neighbours,
            patterns,
            own_ibans,
            own_tax_ids,
        }
    }

    /// Where similar documents lie, or `None` when none are similar enough to
    /// say anything.
    pub fn place(&self, fields: &DocumentFields, text: Option<&str>) -> Option<Placement> {
        let features = features_of(text, &self.own_ibans, &self.own_tax_ids);
        let same_sender =
            |neighbour: &&Neighbour| fields.sender.is_some() && neighbour.sender == fields.sender;
        let same_kind =
            |neighbour: &&Neighbour| fields.kind.is_some() && neighbour.kind == fields.kind;

        let numbered = features
            .numbers
            .iter()
            .map(|number| {
                let group = self.similar(|neighbour| neighbour.features.numbers.contains(number));
                (number, group)
            })
            .max_by_key(|(_, group)| group.len());
        if let Some((number, group)) =
            numbered.filter(|(_, group)| group.len() >= MIN_NUMBER_NEIGHBOURS)
        {
            let signal = Signal::SameNumber {
                label: number.label.to_owned(),
                value: number.value.clone(),
            };
            return Some(self.settle(signal, &group, fields));
        }

        let with_iban = features
            .ibans
            .iter()
            .map(|iban| self.similar(|neighbour| neighbour.features.ibans.contains(iban)))
            .max_by_key(Vec::len);
        if let Some(group) = with_iban.filter(|group| group.len() >= MIN_IBAN_NEIGHBOURS) {
            return Some(self.settle(Signal::SameIban, &group, fields));
        }

        let of_sender = self.similar(|neighbour| same_sender(&neighbour));
        let of_kind: Vec<&Neighbour> = of_sender.iter().copied().filter(same_kind).collect();
        if of_kind.len() >= MIN_SENDER_NEIGHBOURS {
            return Some(self.settle(Signal::SameSenderAndKind, &of_kind, fields));
        }
        if of_sender.len() >= MIN_SENDER_NEIGHBOURS {
            let placement = self.settle(Signal::SameSender, &of_sender, fields);
            if placement.is_sure() || of_kind.len() < MIN_NARROWED_NEIGHBOURS {
                return Some(placement);
            }
            let narrowed = self.settle(Signal::SameSenderAndKind, &of_kind, fields);
            return Some(if narrowed.is_sure() {
                narrowed
            } else {
                placement
            });
        }
        None
    }

    fn similar(&self, matches: impl Fn(&Neighbour) -> bool) -> Vec<&Neighbour> {
        self.neighbours
            .iter()
            .filter(|neighbour| matches(neighbour))
            .collect()
    }

    fn settle(&self, signal: Signal, group: &[&Neighbour], fields: &DocumentFields) -> Placement {
        let similar = count(group.len());
        let Some(folder) = shared_folder(group) else {
            let (folder, agreeing) = busiest_folder(group);
            return Placement {
                folder,
                signal,
                agreeing: count(agreeing),
                similar,
                pattern: None,
                missing: None,
            };
        };
        let agreeing: Vec<&Neighbour> = group
            .iter()
            .copied()
            .filter(|neighbour| neighbour.folder.starts_with(&folder))
            .collect();
        let pattern = self
            .followed_pattern(&agreeing)
            .filter(|_| !other_kind_than_neighbours(fields, &agreeing));
        let missing = pattern
            .as_deref()
            .and_then(|followed| missing_field(followed, fields));
        Placement {
            folder,
            signal,
            agreeing: count(agreeing.len()),
            similar,
            pattern,
            missing,
        }
    }

    /// The pattern most of the documents follow, when they follow one.
    fn followed_pattern(&self, documents: &[&Neighbour]) -> Option<String> {
        let mut per_subtree: BTreeMap<&str, usize> = BTreeMap::new();
        for subtree in documents
            .iter()
            .filter_map(|document| document.subtree.as_deref())
        {
            *per_subtree.entry(subtree).or_default() += 1;
        }
        let (subtree, followers) = per_subtree
            .into_iter()
            .max_by_key(|(subtree, followers)| (*followers, Reverse(*subtree)))?;
        if !meets_share(count(followers), count(documents.len())) {
            return None;
        }
        self.patterns
            .iter()
            .find(|learned| learned.subtree == subtree)
            .and_then(|learned| learned.pattern.clone())
    }
}

/// A number or an account is shared by documents of several kinds; a
/// document of another kind than those lying there does not take their name.
fn other_kind_than_neighbours(fields: &DocumentFields, neighbours: &[&Neighbour]) -> bool {
    let Some(kind) = fields.kind.as_deref() else {
        return false;
    };
    let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
    for neighbour_kind in neighbours
        .iter()
        .filter_map(|neighbour| neighbour.kind.as_deref())
    {
        *per_kind.entry(neighbour_kind.to_lowercase()).or_default() += 1;
    }
    per_kind
        .into_iter()
        .max_by_key(|(name, count)| (*count, Reverse(name.clone())))
        .is_some_and(|(common, _)| common != kind.to_lowercase())
}

fn features_of(
    text: Option<&str>,
    own_ibans: &BTreeSet<String>,
    own_tax_ids: &BTreeSet<String>,
) -> Features {
    let Some(letter) = text else {
        return Features::default();
    };
    Features {
        numbers: labelled_numbers(letter, own_tax_ids),
        ibans: found_ibans(letter)
            .iter()
            .map(|iban| SenderAliasKind::Iban.normalise(iban))
            .filter(|iban| !own_ibans.contains(iban))
            .collect(),
    }
}

/// The deepest folder that holds at least 80 percent of the documents, itself
/// or below; never the root of the location.
fn shared_folder(group: &[&Neighbour]) -> Option<PathBuf> {
    let mut holding: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for neighbour in group {
        for ancestor in neighbour
            .folder
            .ancestors()
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
        {
            *holding.entry(ancestor.to_path_buf()).or_default() += 1;
        }
    }
    let total = count(group.len());
    holding
        .into_iter()
        .filter(|(_, held)| meets_share(*held, total))
        .max_by_key(|(folder, held)| (folder.components().count(), *held))
        .map(|(folder, _)| folder)
}

fn busiest_folder(group: &[&Neighbour]) -> (PathBuf, usize) {
    let mut per_folder: BTreeMap<&Path, usize> = BTreeMap::new();
    for neighbour in group {
        *per_folder.entry(neighbour.folder.as_path()).or_default() += 1;
    }
    per_folder
        .into_iter()
        .max_by_key(|(folder, documents)| (*documents, Reverse(*folder)))
        .map_or_else(Default::default, |(folder, documents)| {
            (folder.to_path_buf(), documents)
        })
}

#[cfg(test)]
mod tests;
