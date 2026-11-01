//! The filters of the search view: the documents the index found for the
//! words, the facets chosen to narrow them and the counts that offer the
//! values. No widgets and no tasks; `SearchView` owns it and `App` changes it
//! from `update`.
//!
//! The facets mirror what a report can save: at most one value of year,
//! sender and kind, and any number of tags that must all be there. A recipient
//! is saved as the tag it makes.
//! Names are compared like the index compares them, trimmed and without regard
//! to case.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Datelike as _;
use ncrs_index::{
    tag_name, Amount, KindCatalog, QueryHit, QuerySpec, SavedQueryId, SNIPPET_MARK_END,
    SNIPPET_MARK_START,
};

/// Most documents a snippet is asked for; the first of the list, which is what
/// is looked at.
pub const SNIPPET_LIMIT: usize = 500;

/// A piece of a document's text around the words, with where they stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Byte ranges of `text`.
    pub marks: Vec<Range<usize>>,
}

impl Snippet {
    /// The part of the text that fits `max_chars`, cut so the first match is
    /// in it; the marks follow.
    pub fn fitted(&self, max_chars: usize) -> Snippet {
        let total = self.text.chars().count();
        if total <= max_chars {
            return self.clone();
        }
        let first_mark = self.marks.first().map_or(0, |mark| {
            self.text
                .char_indices()
                .take_while(|(byte, _)| *byte < mark.start)
                .count()
        });
        let from = first_mark
            .saturating_sub(max_chars.checked_div(4).unwrap_or_default())
            .min(total - max_chars);
        let byte_of = |chars: usize| {
            self.text
                .char_indices()
                .nth(chars)
                .map_or(self.text.len(), |(byte, _)| byte)
        };
        let (start, end) = (byte_of(from), byte_of(from + max_chars));
        let marks = self
            .marks
            .iter()
            .map(|mark| mark.start.max(start)..mark.end.min(end))
            .filter(|mark| mark.start < mark.end)
            .map(|mark| mark.start - start..mark.end - start)
            .collect();
        Snippet {
            text: self.text.get(start..end).unwrap_or_default().to_owned(),
            marks,
        }
    }
}

/// Turns what the index marked into the text on one line and the marked
/// ranges. A mark that is never closed runs to the end, one that is never
/// opened or empty is dropped.
pub fn parse_snippet(marked: &str) -> Snippet {
    let mut text = String::new();
    let mut marks = Vec::new();
    let mut open: Option<usize> = None;
    for character in marked.chars() {
        if character.to_string() == SNIPPET_MARK_START {
            open.get_or_insert(text.len());
        } else if character.to_string() == SNIPPET_MARK_END {
            if let Some(start) = open.take().filter(|start| *start < text.len()) {
                marks.push(start..text.len());
            }
        } else if character.is_whitespace() {
            text.push(' ');
        } else {
            text.push(character);
        }
    }
    if let Some(start) = open.filter(|start| *start < text.len()) {
        marks.push(start..text.len());
    }
    Snippet { text, marks }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetSelection {
    pub year: Option<i32>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub kind: Option<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetValue {
    Year(i32),
    Sender(String),
    Recipient(String),
    Kind(String),
    Tag(String),
}

/// A facet, to leave it out when its own values are counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Year,
    Sender,
    Recipient,
    Kind,
    Tags,
}

/// How the index compares names: trimmed, without regard to case.
fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

impl FacetSelection {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Drops everything chosen of the facet.
    pub fn clear(&mut self, facet: Facet) {
        match facet {
            Facet::Year => self.year = None,
            Facet::Sender => self.sender = None,
            Facet::Recipient => self.recipient = None,
            Facet::Kind => self.kind = None,
            Facet::Tags => self.tags.clear(),
        }
    }

    /// Whether the value is one of the chosen ones.
    pub fn holds(&self, value: &FacetValue, kinds: &KindCatalog) -> bool {
        let same = |chosen: &Option<String>, other: &str| {
            chosen
                .as_deref()
                .is_some_and(|name| name_key(name) == name_key(other))
        };
        match value {
            FacetValue::Year(year) => self.year == Some(*year),
            FacetValue::Sender(other) => same(&self.sender, other),
            FacetValue::Recipient(other) => same(&self.recipient, other),
            FacetValue::Kind(other) => self
                .kind
                .as_deref()
                .is_some_and(|chosen| kinds.kind_key(chosen) == kinds.kind_key(other)),
            FacetValue::Tag(other) => self
                .tags
                .iter()
                .any(|held| name_key(held) == name_key(other)),
        }
    }

    /// Chooses the value, or drops it when it is chosen already. A value of a
    /// facet that takes one replaces the other; a tag joins the others.
    pub fn toggle(&mut self, value: FacetValue, kinds: &KindCatalog) {
        match value {
            FacetValue::Year(year) => {
                self.year = (self.year != Some(year)).then_some(year);
            }
            FacetValue::Sender(chosen) => {
                let chosen_already = self
                    .sender
                    .as_ref()
                    .is_some_and(|held| name_key(held) == name_key(&chosen));
                self.sender = (!chosen_already).then_some(chosen);
            }
            FacetValue::Recipient(chosen) => {
                let chosen_already = self
                    .recipient
                    .as_ref()
                    .is_some_and(|held| name_key(held) == name_key(&chosen));
                self.recipient = (!chosen_already).then_some(chosen);
            }
            FacetValue::Kind(chosen) => {
                let chosen_already = self
                    .kind
                    .as_ref()
                    .is_some_and(|held| kinds.kind_key(held) == kinds.kind_key(&chosen));
                self.kind = (!chosen_already).then(|| kinds.stored_kind(&chosen));
            }
            FacetValue::Tag(chosen) => {
                let before = self.tags.len();
                self.tags.retain(|held| name_key(held) != name_key(&chosen));
                if self.tags.len() == before {
                    self.tags.insert(chosen);
                }
            }
        }
    }

    /// Whether the hit fits the chosen values, leaving `except` out. The same
    /// meaning as `Index::run_query` gives a `QuerySpec` with these values.
    fn admits(&self, hit: &QueryHit, except: Option<Facet>, kinds: &KindCatalog) -> bool {
        let fields = &hit.fields;
        let counts = |facet: Facet| except != Some(facet);
        if let Some(year) = self.year.filter(|_| counts(Facet::Year)) {
            if fields.date.map(|date| date.year()) != Some(year) {
                return false;
            }
        }
        let same_name = |wanted: &Option<String>, held: &Option<String>| {
            wanted.as_ref().is_none_or(|wanted_name| {
                held.as_ref()
                    .is_some_and(|held_name| name_key(held_name) == name_key(wanted_name))
            })
        };
        if counts(Facet::Sender) && !same_name(&self.sender, &fields.sender) {
            return false;
        }
        if counts(Facet::Recipient) && !same_name(&self.recipient, &fields.recipient) {
            return false;
        }
        if counts(Facet::Kind) {
            let same_kind = self.kind.as_ref().is_none_or(|wanted| {
                fields
                    .kind
                    .as_ref()
                    .is_some_and(|held| kinds.kind_key(held) == kinds.kind_key(wanted))
            });
            if !same_kind {
                return false;
            }
        }
        if counts(Facet::Tags) {
            let held = fields.all_tags(kinds);
            let has = |wanted: &String| held.iter().any(|tag| kinds.tag_matches(tag, wanted));
            if !self.tags.iter().all(has) {
                return false;
            }
        }
        true
    }

    pub fn admits_hit(&self, hit: &QueryHit, kinds: &KindCatalog) -> bool {
        self.admits(hit, None, kinds)
    }
}

/// Per facet the values found with the number of documents that have them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetCounts {
    /// Newest year first.
    pub years: Vec<(i32, usize)>,
    pub recipients: Vec<(String, usize)>,
    pub senders: Vec<(String, usize)>,
    pub kinds: Vec<(String, usize)>,
    pub tags: Vec<(String, usize)>,
}

/// Counts by name key, the name as first seen kept for the display; a chosen
/// value stays in the list with its count even when it is zero. Most documents
/// first, then by name.
fn tally<T: Clone>(
    found: impl Iterator<Item = (String, T)>,
    chosen: impl Iterator<Item = (String, T)>,
) -> Vec<(T, usize)> {
    let mut counted: BTreeMap<String, (T, usize)> = BTreeMap::new();
    for (key, value) in found {
        counted.entry(key).or_insert((value, 0)).1 += 1;
    }
    for (key, value) in chosen {
        counted.entry(key).or_insert((value, 0));
    }
    let mut ordered: Vec<(String, (T, usize))> = counted.into_iter().collect();
    ordered.sort_by(|(first_key, (_, first)), (second_key, (_, second))| {
        second.cmp(first).then_with(|| first_key.cmp(second_key))
    });
    ordered.into_iter().map(|(_, entry)| entry).collect()
}

fn named(name: &str) -> (String, String) {
    (name_key(name), name.trim().to_owned())
}

/// A known kind counts under its ID whatever it was written as.
fn named_kind(kind: &str, kinds: &KindCatalog) -> (String, String) {
    (kinds.kind_key(kind), kinds.stored_kind(kind))
}

/// The values of every facet. A facet that takes one value counts over the
/// hits that fit all the other choices, so the user can switch from one value
/// to another; the tags count over the hits that fit everything chosen.
pub fn facet_counts(
    hits: &[QueryHit],
    selection: &FacetSelection,
    catalog: &KindCatalog,
) -> FacetCounts {
    let among = |except: Option<Facet>| {
        hits.iter()
            .filter(move |hit| selection.admits(hit, except, catalog))
    };

    let mut years = tally(
        among(Some(Facet::Year))
            .filter_map(|hit| hit.fields.date)
            .map(|date| (date.year().to_string(), date.year())),
        selection
            .year
            .into_iter()
            .map(|year| (year.to_string(), year)),
    );
    years.sort_by(|(first, _), (second, _)| second.cmp(first));

    let senders = tally(
        among(Some(Facet::Sender))
            .filter_map(|hit| hit.fields.sender.as_deref())
            .filter(|sender| !sender.trim().is_empty())
            .map(named),
        selection.sender.iter().map(|sender| named(sender)),
    );
    let recipients = tally(
        among(Some(Facet::Recipient))
            .filter_map(|hit| hit.fields.recipient.as_deref())
            .filter(|recipient| !recipient.trim().is_empty())
            .map(named),
        selection.recipient.iter().map(|recipient| named(recipient)),
    );
    let kinds = tally(
        among(Some(Facet::Kind))
            .filter_map(|hit| hit.fields.kind.as_deref())
            .filter(|kind| !kind.trim().is_empty())
            .map(|kind| named_kind(kind, catalog)),
        selection.kind.iter().map(|kind| named_kind(kind, catalog)),
    );
    let tags = tally(
        among(None)
            .flat_map(|hit| hit.fields.tags.iter())
            .filter(|tag| !tag.trim().is_empty())
            .map(|tag| named(tag)),
        selection.tags.iter().map(|tag| named(tag)),
    );

    FacetCounts {
        years,
        recipients,
        senders,
        kinds,
        tags,
    }
}

/// Newest first, the undated last, then by path.
fn display_order(first: &QueryHit, second: &QueryHit) -> std::cmp::Ordering {
    second
        .fields
        .date
        .is_some()
        .cmp(&first.fields.date.is_some())
        .then_with(|| second.fields.date.cmp(&first.fields.date))
        .then_with(|| first.path.cmp(&second.path))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchFilters {
    /// The words the hits stand for.
    words: String,
    /// What the index found for the words alone, in display order.
    hits: Vec<QueryHit>,
    /// Where a path is in `hits`.
    position_of: HashMap<PathBuf, usize>,
    selection: FacetSelection,
    /// The hits that fit the selection, as positions in `hits`.
    shown: Vec<usize>,
    counts: FacetCounts,
    /// The index is being asked.
    busy: bool,
    problem: Option<String>,
    /// By the path of the document; only while words are typed.
    snippets: HashMap<PathBuf, Snippet>,
    kinds: Arc<KindCatalog>,
    /// Answers the question asked last; an older answer is stale.
    asked: u64,
    /// The report the search was opened to change.
    report: Option<SavedQueryId>,
}

impl SearchFilters {
    pub fn new(kinds: Arc<KindCatalog>) -> Self {
        Self {
            words: String::new(),
            hits: Vec::new(),
            position_of: HashMap::new(),
            selection: FacetSelection::default(),
            shown: Vec::new(),
            counts: FacetCounts::default(),
            busy: false,
            problem: None,
            snippets: HashMap::new(),
            kinds,
            asked: 0,
            report: None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn selection(&self) -> &FacetSelection {
        &self.selection
    }

    pub fn kinds(&self) -> &KindCatalog {
        &self.kinds
    }

    pub fn counts(&self) -> &FacetCounts {
        &self.counts
    }

    /// A value is chosen, so the index answers the search instead of the walk.
    pub fn is_active(&self) -> bool {
        !self.selection.is_empty()
    }

    /// The documents that fit the facets, newest first.
    pub fn shown(&self) -> impl Iterator<Item = &QueryHit> {
        self.shown.iter().filter_map(|at| self.hits.get(*at))
    }

    /// What the index knows of the document at `path`, when it was found.
    pub fn hit_at(&self, path: &Path) -> Option<&QueryHit> {
        self.hits.get(*self.position_of.get(path)?)
    }

    /// The amounts of the documents shown added up; none when no document
    /// has one or the currencies differ.
    pub fn total_amount(&self) -> Option<Amount> {
        let mut total: Option<Amount> = None;
        for amount in self.shown().filter_map(|hit| hit.fields.amount.as_ref()) {
            total = Some(match total {
                None => amount.clone(),
                Some(sum) if sum.currency == amount.currency => Amount {
                    minor_units: sum.minor_units.checked_add(amount.minor_units)?,
                    currency: sum.currency,
                },
                Some(_) => return None,
            });
        }
        total
    }

    /// The words and the facets as one question to the index: what a report
    /// saved from this search would ask.
    pub fn spec(&self) -> QuerySpec {
        let selection = &self.selection;
        let text = self.words.trim();
        let mut tags = selection.tags.clone();
        tags.extend(
            selection
                .recipient
                .iter()
                .map(|recipient| tag_name(recipient))
                .filter(|name| !name.is_empty()),
        );
        QuerySpec {
            year: selection.year,
            sender: selection.sender.clone(),
            kind: selection.kind.clone(),
            tags,
            full_text: (!text.is_empty()).then(|| text.to_owned()),
        }
    }

    /// The question the search asks the index: the words alone, so the facets
    /// can be counted and switched here without asking again. `words` are
    /// what the hits will stand for.
    pub fn ask(&mut self, words: &str) -> (u64, QuerySpec) {
        self.asked = self.asked.wrapping_add(1);
        self.busy = true;
        self.problem = None;
        self.words = words.trim().to_owned();
        let question = QuerySpec {
            full_text: (!self.words.is_empty()).then(|| self.words.clone()),
            ..QuerySpec::default()
        };
        (self.asked, question)
    }

    pub fn generation(&self) -> u64 {
        self.asked
    }

    pub fn is_current(&self, asked: u64) -> bool {
        self.asked == asked
    }

    /// There is something to save as a report: words or a chosen value.
    pub fn can_save_as_report(&self) -> bool {
        !self.words.is_empty() || !self.selection.is_empty()
    }

    /// The report the search was opened to change, if any.
    pub fn report(&self) -> Option<SavedQueryId> {
        self.report
    }

    /// Starts from the criteria of `report`: the search changes that one.
    pub fn edit_report(&mut self, report: SavedQueryId, spec: &QuerySpec) {
        self.start_from(spec);
        self.report = Some(report);
    }

    /// Starts from `spec` as a search of its own.
    pub fn start_from(&mut self, spec: &QuerySpec) {
        self.report = None;
        self.selection = FacetSelection {
            year: spec.year,
            sender: spec.sender.clone(),
            recipient: None,
            kind: spec.kind.clone(),
            tags: spec.tags.clone(),
        };
        self.refilter();
    }

    /// The search no longer changes a report.
    pub fn leave_report(&mut self) {
        self.report = None;
    }

    /// The paths of the hits shown that have no snippet yet, and the words to
    /// find in them; nothing without words.
    pub fn snippet_request(&self) -> Option<(Vec<PathBuf>, String)> {
        if self.words.is_empty() {
            return None;
        }
        let paths: Vec<PathBuf> = self
            .shown()
            .filter(|hit| !hit.missing && !self.snippets.contains_key(&hit.path))
            .take(SNIPPET_LIMIT)
            .map(|hit| hit.path.clone())
            .collect();
        (!paths.is_empty()).then(|| (paths, self.words.clone()))
    }

    pub fn add_snippets(&mut self, found: Vec<(PathBuf, String)>) {
        self.snippets.extend(
            found
                .into_iter()
                .map(|(path, marked)| (path, parse_snippet(&marked))),
        );
    }

    pub fn snippet_of(&self, hit: &QueryHit) -> Option<&Snippet> {
        self.snippets.get(&hit.path)
    }

    /// The answer of the index. A document the index knows is gone is left out:
    /// there is no file to go to.
    pub fn ran(&mut self, mut hits: Vec<QueryHit>) {
        hits.retain(|hit| !hit.missing);
        hits.sort_by(display_order);
        self.position_of = hits
            .iter()
            .enumerate()
            .map(|(at, hit)| (hit.path.clone(), at))
            .collect();
        self.hits = hits;
        self.snippets.clear();
        self.busy = false;
        self.problem = None;
        self.refilter();
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.problem = Some(detail);
    }

    pub fn toggle(&mut self, value: FacetValue) {
        self.selection.toggle(value, &self.kinds);
        self.refilter();
    }

    pub fn clear(&mut self, facet: Facet) {
        self.selection.clear(facet);
        self.refilter();
    }

    /// The facets and the list after the hits or the choice changed.
    fn refilter(&mut self) {
        self.shown = self
            .hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| self.selection.admits_hit(hit, &self.kinds))
            .map(|(at, _)| at)
            .collect();
        self.counts = facet_counts(&self.hits, &self.selection, &self.kinds);
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
pub mod tests;
