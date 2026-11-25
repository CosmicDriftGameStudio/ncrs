//! Where a document belongs by the documents already filed: those that carry
//! the same number, the same IBAN or come from the same sender. Pure: nothing
//! here touches a file or the index.
//!
//! The text of a document is data. It is only searched for numbers and IBANs.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use chrono::Datelike as _;
use ncrs_index::{
    compile_bounded_regex, DocumentFields, Field, KindCatalog, SenderAliasKind, SubtreePattern,
};
use regex::Regex;

use crate::archive_profile::{
    date_in_file_stem, learn_archive, subtree_per_document, year_of, ArchiveDocument,
};
use crate::field_detection::OwnNames;
use crate::filing_plan::missing_field;
use crate::reorganisation::PlanFile;
use crate::sender_detection::found_ibans;
use crate::structure::LocationStructure;
use sender_key::SenderKey;

/// Lower-case spelling as written in a letter, and how it is shown.
const NUMBER_LABELS: [(&str, &str); 20] = [
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
    ("konto-nr", "Konto-Nr."),
    ("kontonummer", "Kontonummer"),
    ("stamm-nr", "Stamm-Nr."),
    ("depot-nr", "Depot-Nr."),
    ("depotnummer", "Depotnummer"),
    ("account number", "Account number"),
];
/// The numbers of an account. Letters of other senders may state one's own
/// account as well, so they only decide when the folders agree.
const ACCOUNT_LABELS: [&str; 6] = [
    "Konto-Nr.",
    "Kontonummer",
    "Stamm-Nr.",
    "Depot-Nr.",
    "Depotnummer",
    "Account number",
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
/// How many folders an unsure placement offers at most.
const MAX_CANDIDATES: usize = 4;
/// The numbers a folder name counts as a year level for.
const YEAR_LEVELS: std::ops::RangeInclusive<u32> = 1900..=2100;

static LABELLED_NUMBER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let labels: Vec<String> = NUMBER_LABELS
        .iter()
        .map(|(spelling, _)| spelling.replace(' ', r"[ \t]+"))
        .collect();
    compile_bounded_regex(&format!(
        r"(?i)\b({})\b[ \t]*(?:nr)?\.?[ \t]*[:\-]?[ \t]*([A-Za-z0-9][A-Za-z0-9/\-]*(?:[ \t][0-9]{{1,5}}\b)?)",
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
    sender_key: Option<SenderKey>,
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
    catalog: Arc<KindCatalog>,
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
    /// For an unsure placement, the folder with the next most similar
    /// documents; ties go to the path that sorts first.
    pub runner_up: Option<PathBuf>,
    /// For an unsure placement, where the document could go: the folders of
    /// the similar documents without their date levels, the one with most of
    /// them first. Empty for a sure placement.
    pub candidates: Vec<Candidate>,
}

/// A folder an unsure document could go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Below the root of the location, without date levels (year, month).
    pub folder: PathBuf,
    /// What the similar documents of this folder follow, when they follow
    /// one; rendered with the date of the document it gives the folder with
    /// the year of that document.
    pub pattern: Option<String>,
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
                sender_key: document.fields.sender.as_deref().and_then(SenderKey::of),
                kind: document.fields.kind.clone(),
                features: features_of(document.text.as_deref(), &own_ibans, &own_tax_ids),
                subtree,
            })
            .collect();
        Self {
            neighbours,
            patterns,
            catalog: Arc::new(KindCatalog::built_in()),
            own_ibans,
            own_tax_ids,
        }
    }

    /// Compares kinds by the ID the catalog gives them, so kinds written
    /// differently but meaning the same are the same kind.
    #[must_use]
    pub fn with_kinds(mut self, catalog: Arc<KindCatalog>) -> Self {
        self.catalog = catalog;
        self
    }

    fn kind_key(&self, kind: &str) -> String {
        self.catalog.kind_key(kind)
    }

    /// Where similar documents lie, or `None` when none are similar enough to
    /// say anything.
    pub fn place(&self, fields: &DocumentFields, text: Option<&str>) -> Option<Placement> {
        self.place_named(fields, text, None)
    }

    /// Like `place`, with the name of the file: a document whose year has
    /// neighbours of its own lying deeper is placed in their folder, unless the
    /// name states another year than the letter.
    pub fn place_named(
        &self,
        fields: &DocumentFields,
        text: Option<&str>,
        file_name: Option<&str>,
    ) -> Option<Placement> {
        let narrow_year = fields
            .date
            .map(|date| date.year())
            .filter(|year| name_allows_year(file_name, *year));
        let features = features_of(text, &self.own_ibans, &self.own_tax_ids);
        let sender_key = fields.sender.as_deref().and_then(SenderKey::of);
        let same_sender = |neighbour: &&Neighbour| {
            sender_key.as_ref().is_some_and(|wanted| {
                neighbour
                    .sender_key
                    .as_ref()
                    .is_some_and(|other| wanted.matches(other))
            })
        };
        let wanted_kind = fields.kind.as_deref().map(|kind| self.kind_key(kind));
        let same_kind = |neighbour: &&Neighbour| {
            wanted_kind.is_some()
                && neighbour.kind.as_deref().map(|kind| self.kind_key(kind)) == wanted_kind
        };

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
            let placement = self.settle(signal, &group, fields, narrow_year);
            if placement.is_sure() || !ACCOUNT_LABELS.contains(&number.label) {
                return Some(placement);
            }
        }

        let with_iban = features
            .ibans
            .iter()
            .map(|iban| self.similar(|neighbour| neighbour.features.ibans.contains(iban)))
            .max_by_key(Vec::len);
        if let Some(group) = with_iban.filter(|group| group.len() >= MIN_IBAN_NEIGHBOURS) {
            let placement = self.settle(Signal::SameIban, &group, fields, narrow_year);
            // An account also stands on what others write to its holder, like
            // the rent account in a lease: a document of another sender and
            // kind than the account's documents is not placed by it alone.
            let from_elsewhere = sender_key.is_some()
                && wanted_kind.is_some()
                && !group
                    .iter()
                    .any(|neighbour| same_sender(neighbour) || same_kind(neighbour));
            if !(placement.is_sure() && from_elsewhere) {
                return Some(placement);
            }
        }

        let of_sender = self.similar(|neighbour| same_sender(&neighbour));
        let of_kind: Vec<&Neighbour> = of_sender.iter().copied().filter(same_kind).collect();
        if of_kind.len() >= MIN_SENDER_NEIGHBOURS {
            return Some(self.settle(Signal::SameSenderAndKind, &of_kind, fields, narrow_year));
        }
        if of_sender.len() >= MIN_SENDER_NEIGHBOURS {
            let placement = self.settle(Signal::SameSender, &of_sender, fields, narrow_year);
            if placement.is_sure() || of_kind.len() < MIN_NARROWED_NEIGHBOURS {
                return Some(placement);
            }
            let narrowed = self.settle(Signal::SameSenderAndKind, &of_kind, fields, narrow_year);
            return Some(if narrowed.is_sure() {
                narrowed
            } else {
                placement
            });
        }
        None
    }

    /// What alike documents share, so that they are asked about once: the
    /// account number when the text states one, else the sender and the kind.
    pub fn group_key(&self, fields: &DocumentFields, text: Option<&str>) -> Option<String> {
        let features = features_of(text, &self.own_ibans, &self.own_tax_ids);
        if let Some(account) = features
            .numbers
            .iter()
            .find(|number| ACCOUNT_LABELS.contains(&number.label))
        {
            return Some(format!("account:{}", account.value));
        }
        let sender = fields.sender.as_deref().and_then(SenderKey::of)?;
        let kind = fields.kind.as_deref().map(|kind| self.kind_key(kind))?;
        Some(format!("sender:{}|{kind}", sender.joined()))
    }

    fn similar(&self, matches: impl Fn(&Neighbour) -> bool) -> Vec<&Neighbour> {
        self.neighbours
            .iter()
            .filter(|neighbour| matches(neighbour))
            .collect()
    }

    fn settle(
        &self,
        signal: Signal,
        group: &[&Neighbour],
        fields: &DocumentFields,
        narrow_year: Option<i32>,
    ) -> Placement {
        let similar = count(group.len());
        let Some(folder) = shared_folder(group) else {
            let (folder, agreeing, runner_up) = busiest_folders(group);
            return Placement {
                candidates: self.candidates(group),
                folder,
                signal,
                agreeing: count(agreeing),
                similar,
                pattern: None,
                missing: None,
                runner_up,
            };
        };
        let agreeing: Vec<&Neighbour> = group
            .iter()
            .copied()
            .filter(|neighbour| neighbour.folder.starts_with(&folder))
            .collect();
        let pattern = self
            .followed_pattern(&agreeing)
            .filter(|_| self.kind_allows_pattern(&signal, fields, &agreeing));
        if let Some((deeper, same_year)) =
            narrow_to_year(&folder, &agreeing, narrow_year).filter(|_| pattern.is_none())
        {
            return Placement {
                folder: deeper,
                signal,
                agreeing: count(same_year.len()),
                similar: count(same_year.len()),
                pattern: None,
                missing: None,
                runner_up: None,
                candidates: Vec::new(),
            };
        }
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
            runner_up: None,
            candidates: Vec::new(),
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

/// A number or an account is shared by documents of several kinds, so the
/// name of the folder is not offered to a document of another kind than the
/// one lying there. Of the same sender alone, only to a document of that kind.
impl NeighbourIndex {
    fn kind_allows_pattern(
        &self,
        signal: &Signal,
        fields: &DocumentFields,
        neighbours: &[&Neighbour],
    ) -> bool {
        let common = self.most_common_kind(neighbours);
        let kind = fields.kind.as_deref().map(|kind| self.kind_key(kind));
        match signal {
            Signal::SameSender => kind.is_some() && kind == common,
            Signal::SameNumber { .. } | Signal::SameIban | Signal::SameSenderAndKind => {
                kind.is_none() || common.is_none() || kind == common
            }
        }
    }

    fn most_common_kind(&self, neighbours: &[&Neighbour]) -> Option<String> {
        let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
        for kind in neighbours
            .iter()
            .filter_map(|neighbour| neighbour.kind.as_deref())
        {
            *per_kind.entry(self.kind_key(kind)).or_default() += 1;
        }
        per_kind
            .into_iter()
            .max_by_key(|(name, count)| (*count, Reverse(name.clone())))
            .map(|(name, _)| name)
    }

    /// The folders of `group` cut at their first date level, the one with
    /// most documents first, each with the pattern its documents follow.
    fn candidates(&self, group: &[&Neighbour]) -> Vec<Candidate> {
        let mut per_folder: BTreeMap<PathBuf, Vec<&Neighbour>> = BTreeMap::new();
        for neighbour in group {
            per_folder
                .entry(without_date_levels(&neighbour.folder))
                .or_default()
                .push(neighbour);
        }
        let mut ranked: Vec<(PathBuf, Vec<&Neighbour>)> = per_folder.into_iter().collect();
        ranked.sort_by_key(|(folder, members)| (Reverse(members.len()), folder.clone()));
        ranked
            .into_iter()
            .take(MAX_CANDIDATES)
            .map(|(folder, members)| Candidate {
                pattern: self.followed_pattern(&members),
                folder,
            })
            .collect()
    }
}

fn is_year_level(segment: &str) -> bool {
    let Some(year) = segment
        .get(..4)
        .and_then(|digits| digits.parse::<u32>().ok())
    else {
        return false;
    };
    let rest = segment.get(4..).unwrap_or_default();
    YEAR_LEVELS.contains(&year)
        && rest
            .chars()
            .next()
            .is_none_or(|next| matches!(next, '-' | '.' | '_' | ' '))
}

/// The folder up to its first year level.
fn without_date_levels(folder: &Path) -> PathBuf {
    folder
        .components()
        .take_while(|component| !is_year_level(&component.as_os_str().to_string_lossy()))
        .collect()
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

/// The year a file name states may not differ from the letter's: a balance
/// notice of December 31st arrives in January.
fn name_allows_year(file_name: Option<&str>, year: i32) -> bool {
    file_name
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .and_then(date_in_file_stem)
        .is_none_or(|date| date.year() == year)
}

fn lies_in_year(folder: &Path, year: i32) -> bool {
    folder
        .components()
        .any(|part| part.as_os_str().to_str().and_then(year_of) == Some(year))
}

/// The folder below `folder` that the neighbours of `year` share, with those
/// neighbours, when there are enough of them.
fn narrow_to_year<'a>(
    folder: &Path,
    agreeing: &[&'a Neighbour],
    narrow_year: Option<i32>,
) -> Option<(PathBuf, Vec<&'a Neighbour>)> {
    let year = narrow_year?;
    let same_year: Vec<&Neighbour> = agreeing
        .iter()
        .copied()
        .filter(|neighbour| lies_in_year(&neighbour.folder, year))
        .collect();
    if same_year.len() < MIN_NARROWED_NEIGHBOURS {
        return None;
    }
    let deeper = shared_folder(&same_year)?;
    (deeper != folder && deeper.starts_with(folder)).then_some((deeper, same_year))
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

/// The folder with most of the group, how many it holds, and the second one.
fn busiest_folders(group: &[&Neighbour]) -> (PathBuf, usize, Option<PathBuf>) {
    let mut per_folder: BTreeMap<&Path, usize> = BTreeMap::new();
    for neighbour in group {
        *per_folder.entry(neighbour.folder.as_path()).or_default() += 1;
    }
    let mut ranked: Vec<(&Path, usize)> = per_folder.into_iter().collect();
    ranked.sort_by_key(|(folder, documents)| (Reverse(*documents), *folder));
    let mut folders = ranked.into_iter();
    let (busiest, documents) = folders
        .next()
        .map_or_else(Default::default, |(folder, documents)| {
            (folder.to_path_buf(), documents)
        });
    (
        busiest,
        documents,
        folders.next().map(|(folder, _)| folder.to_path_buf()),
    )
}

mod sender_key;
#[cfg(test)]
mod tests;
