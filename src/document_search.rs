//! State of the document search page: the words typed, the documents the index
//! found for them, the facets chosen to narrow them and the cursor in the
//! narrowed list. No widgets and no tasks; `App` owns it and changes it from
//! `update`, the view is `ui::document_search`.
//!
//! The facets mirror what a report can save: at most one value of year,
//! sender and kind, and any number of tags that must all be there. A recipient
//! is saved as the tag it makes.
//! Names are compared like the index compares them, trimmed and without regard
//! to case.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Datelike as _;
use iced::mouse::ScrollDelta;

use ncrs_index::{tag_name, Amount, KindCatalog, QuerySpec, SNIPPET_MARK_END, SNIPPET_MARK_START};

use crate::document_preview::{DocumentPreview, PreviewContent, PreviewRequest};
use crate::list_window::ListWindow;
use crate::query_work::QueryHit;

/// Most documents of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 5;

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
enum Facet {
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
    kinds: &KindCatalog,
) -> FacetCounts {
    let among = |except: Option<Facet>| {
        hits.iter()
            .filter(move |hit| selection.admits(hit, except, kinds))
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
            .map(|kind| named_kind(kind, kinds)),
        selection.kind.iter().map(|kind| named_kind(kind, kinds)),
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
pub struct DocumentSearch {
    text: String,
    /// What the index found for the text alone, in display order.
    hits: Vec<QueryHit>,
    selection: FacetSelection,
    /// The hits that fit the selection, as positions in `hits`.
    shown: Vec<usize>,
    counts: FacetCounts,
    cursor: usize,
    window: ListWindow,
    /// The index is being asked; the view takes no new task.
    busy: bool,
    problem: Option<String>,
    /// By the path of the document; only while words are typed.
    snippets: HashMap<PathBuf, Snippet>,
    preview: DocumentPreview,
    kinds: Arc<KindCatalog>,
}

impl DocumentSearch {
    /// Opens busy: the first search starts with the page.
    pub fn new(text: String, kinds: Arc<KindCatalog>) -> Self {
        Self {
            text,
            kinds,
            hits: Vec::new(),
            selection: FacetSelection::default(),
            shown: Vec::new(),
            counts: FacetCounts::default(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            problem: None,
            snippets: HashMap::new(),
            preview: DocumentPreview::default(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
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

    /// The documents the words found, before the facets narrow them.
    pub fn found(&self) -> &[QueryHit] {
        &self.hits
    }

    /// The documents that fit the facets, newest first.
    pub fn shown(&self) -> impl Iterator<Item = &QueryHit> {
        self.shown.iter().filter_map(|at| self.hits.get(*at))
    }

    pub fn shown_len(&self) -> usize {
        self.shown.len()
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

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The first document of the list that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.shown.len(), VISIBLE_ROWS)
    }

    pub fn selected(&self) -> Option<&QueryHit> {
        self.hits.get(*self.shown.get(self.cursor)?)
    }

    /// The words and the facets as one question to the index: what a report
    /// saved from this page would ask.
    pub fn spec(&self) -> QuerySpec {
        let selection = &self.selection;
        let text = self.text.trim();
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

    /// The question the page asks the index: the words alone, so the facets
    /// can be counted and switched here without asking again.
    pub fn begin_search(&mut self) -> QuerySpec {
        self.busy = true;
        self.problem = None;
        let text = self.text.trim();
        QuerySpec {
            full_text: (!text.is_empty()).then(|| text.to_owned()),
            ..QuerySpec::default()
        }
    }

    /// The words changed; the question to ask for them.
    pub fn set_text(&mut self, text: String) -> QuerySpec {
        self.text = text;
        self.preview.clear();
        self.begin_search()
    }

    /// There is something to save as a report: words or a chosen value.
    pub fn can_save_as_report(&self) -> bool {
        !self.text.trim().is_empty() || !self.selection.is_empty()
    }

    /// The paths to ask snippets for and the words to find in them; nothing
    /// without words.
    pub fn snippet_request(&self) -> Option<(Vec<PathBuf>, String)> {
        let words = self.text.trim();
        if words.is_empty() {
            return None;
        }
        let paths: Vec<PathBuf> = self
            .hits
            .iter()
            .filter(|hit| !hit.missing)
            .take(SNIPPET_LIMIT)
            .map(|hit| hit.path.clone())
            .collect();
        (!paths.is_empty()).then(|| (paths, words.to_owned()))
    }

    pub fn set_snippets(&mut self, found: Vec<(PathBuf, String)>) {
        self.snippets = found
            .into_iter()
            .map(|(path, marked)| (path, parse_snippet(&marked)))
            .collect();
    }

    pub fn snippet_of(&self, hit: &QueryHit) -> Option<&Snippet> {
        self.snippets.get(&hit.path)
    }

    pub fn has_snippets(&self) -> bool {
        !self.snippets.is_empty()
    }

    pub fn preview(&self) -> &DocumentPreview {
        &self.preview
    }

    /// The request for the preview of the selected document, when it is not
    /// the one shown. A missing document or none has no preview.
    pub fn preview_request(&mut self) -> Option<PreviewRequest> {
        let Some(hit) = self.selected().filter(|hit| !hit.missing) else {
            self.preview.clear();
            return None;
        };
        let path = hit.path.clone();
        let passages: Vec<String> = self.text.split_whitespace().map(str::to_owned).collect();
        self.preview.request(&path, &passages)
    }

    pub fn preview_loaded(&mut self, serial: u64, content: PreviewContent) {
        self.preview.loaded(serial, content);
    }

    #[cfg(test)]
    pub fn show_preview_for_test(&mut self, content: PreviewContent) {
        self.preview = DocumentPreview::showing(content);
    }

    pub fn ran(&mut self, mut hits: Vec<QueryHit>) {
        hits.sort_by(display_order);
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

    pub fn clear_facets(&mut self) {
        self.selection = FacetSelection::default();
        self.refilter();
    }

    /// The facets and the list after the hits or the choice changed. The
    /// cursor stays on its row number, held inside the narrowed list.
    fn refilter(&mut self) {
        self.shown = self
            .hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| self.selection.admits_hit(hit, &self.kinds))
            .map(|(at, _)| at)
            .collect();
        self.counts = facet_counts(&self.hits, &self.selection, &self.kinds);
        self.cursor = self.cursor.min(self.shown.len().saturating_sub(1));
        self.window
            .ensure_visible(self.cursor, self.shown.len(), VISIBLE_ROWS);
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let Some(last) = self.shown.len().checked_sub(1) else {
            return;
        };
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.window
            .ensure_visible(self.cursor, self.shown.len(), VISIBLE_ROWS);
    }

    pub fn select(&mut self, index: usize) {
        if index < self.shown.len() {
            self.cursor = index;
            self.window
                .ensure_visible(index, self.shown.len(), VISIBLE_ROWS);
        }
    }

    /// The wheel moves the window; the cursor follows when it would leave.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        self.window
            .scroll(delta, self.shown.len(), VISIBLE_ROWS, &mut self.cursor);
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
pub mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;
    use ncrs_index::{Amount, Currency, DocumentFields};

    use super::*;

    pub fn found_hit(
        name: &str,
        date: Option<(i32, u32, u32)>,
        sender: Option<&str>,
        kind: Option<&str>,
        tags: &[&str],
    ) -> QueryHit {
        QueryHit {
            path: PathBuf::from(format!("archive/{name}")),
            fields: DocumentFields {
                sender: sender.map(str::to_owned),
                kind: kind.map(str::to_owned),
                date: date.and_then(|(year, month, day)| NaiveDate::from_ymd_opt(year, month, day)),
                tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                amount: Some(Amount {
                    minor_units: 12_345,
                    currency: Currency::new("EUR").unwrap(),
                }),
                ..DocumentFields::default()
            },
            missing: false,
        }
    }

    /// Six documents: years 2023 to 2025, two senders, two kinds, some tags.
    pub fn six_hits() -> Vec<QueryHit> {
        vec![
            found_hit(
                "a.pdf",
                Some((2025, 3, 1)),
                Some("Firma A"),
                Some("invoice"),
                &["open"],
            ),
            found_hit(
                "b.pdf",
                Some((2025, 6, 1)),
                Some("firma a"),
                Some("Invoice"),
                &["paid", "office"],
            ),
            found_hit(
                "c.pdf",
                Some((2024, 5, 1)),
                Some("Firma A"),
                Some("contract"),
                &["office"],
            ),
            found_hit(
                "d.pdf",
                Some((2024, 8, 1)),
                Some("Agency B"),
                Some("invoice"),
                &["paid"],
            ),
            found_hit(
                "e.pdf",
                Some((2023, 1, 1)),
                Some("Agency B"),
                Some("letter"),
                &[],
            ),
            found_hit("f.pdf", None, None, None, &[]),
        ]
    }

    pub fn loaded_search() -> DocumentSearch {
        let mut search = DocumentSearch::new(String::new(), Arc::default());
        search.ran(six_hits());
        search
    }

    fn names(search: &DocumentSearch) -> Vec<String> {
        search
            .shown()
            .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_list_is_newest_first_with_the_undated_last() {
        let search = loaded_search();

        assert_eq!(
            names(&search),
            ["b.pdf", "a.pdf", "d.pdf", "c.pdf", "e.pdf", "f.pdf"]
        );
    }

    #[test]
    fn choosing_a_year_keeps_the_other_years_in_the_list_with_their_numbers() {
        let mut search = loaded_search();

        search.toggle(FacetValue::Year(2024));

        assert_eq!(names(&search), ["d.pdf", "c.pdf"]);
        assert_eq!(search.counts().years, [(2025, 2), (2024, 2), (2023, 1)]);
        search.toggle(FacetValue::Year(2025));
        assert_eq!(search.selection().year, Some(2025));
        assert_eq!(names(&search), ["b.pdf", "a.pdf"]);
        search.toggle(FacetValue::Year(2025));
        assert_eq!(search.selection().year, None);
        assert_eq!(search.shown_len(), 6);
    }

    #[test]
    fn the_tags_count_over_what_is_left_and_a_chosen_one_stays_visible() {
        let mut search = loaded_search();
        assert_eq!(
            search.counts().tags,
            [
                ("office".to_owned(), 2),
                ("paid".to_owned(), 2),
                ("open".to_owned(), 1)
            ]
        );

        search.toggle(FacetValue::Year(2024));
        search.toggle(FacetValue::Tag("office".to_owned()));
        assert_eq!(names(&search), ["c.pdf"]);
        assert_eq!(search.counts().tags, [("office".to_owned(), 1)]);

        search.toggle(FacetValue::Year(2023));
        assert!(names(&search).is_empty());
        assert_eq!(search.counts().tags, [("office".to_owned(), 0)]);
    }

    #[test]
    fn year_sender_and_tag_narrow_together_and_case_does_not_matter() {
        let mut search = loaded_search();

        search.toggle(FacetValue::Year(2025));
        search.toggle(FacetValue::Sender("FIRMA a".to_owned()));
        search.toggle(FacetValue::Tag("PAID".to_owned()));

        assert_eq!(names(&search), ["b.pdf"]);
        assert_eq!(search.counts().senders, [("firma a".to_owned(), 1)]);
    }

    #[test]
    fn senders_and_kinds_are_counted_without_regard_to_case_and_the_most_first() {
        let search = loaded_search();

        assert_eq!(
            search.counts().senders,
            [("firma a".to_owned(), 3), ("Agency B".to_owned(), 2)]
        );
        assert_eq!(
            search.counts().kinds,
            [
                ("invoice".to_owned(), 3),
                ("contract".to_owned(), 1),
                ("letter".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn spellings_of_a_known_kind_are_one_facet_entry_and_one_choice() {
        let hits = vec![
            found_hit("a.pdf", None, None, Some("invoice"), &[]),
            found_hit("b.pdf", None, None, Some("Rechnung"), &[]),
            found_hit("c.pdf", None, None, Some("Steuerbescheid"), &[]),
        ];
        let mut selection = FacetSelection::default();

        assert_eq!(
            facet_counts(&hits, &selection, &KindCatalog::built_in()).kinds,
            [("invoice".to_owned(), 2), ("Steuerbescheid".to_owned(), 1)]
        );
        selection.toggle(
            FacetValue::Kind("Rechnung".to_owned()),
            &KindCatalog::built_in(),
        );
        let found: Vec<&str> = hits
            .iter()
            .filter(|hit| selection.admits_hit(hit, &KindCatalog::built_in()))
            .filter_map(|hit| hit.path.file_name()?.to_str())
            .collect();

        assert_eq!(found, ["a.pdf", "b.pdf"]);
        assert!(selection.holds(
            &FacetValue::Kind("Invoice".to_owned()),
            &KindCatalog::built_in()
        ));
        selection.toggle(
            FacetValue::Kind("invoice".to_owned()),
            &KindCatalog::built_in(),
        );
        assert!(selection.is_empty());
    }

    #[test]
    fn the_words_of_a_custom_kind_are_one_facet_entry_and_one_choice() {
        let (catalog, skips) = KindCatalog::built_in().extended(&[ncrs_index::CustomKind {
            id: "payslip".to_owned(),
            german: Some("Lohnabrechnung".to_owned()),
            english: None,
            words: vec!["Gehaltsabrechnung".to_owned()],
        }]);
        assert!(skips.is_empty());
        let hits = vec![
            found_hit("a.pdf", None, None, Some("payslip"), &[]),
            found_hit("b.pdf", None, None, Some("Gehaltsabrechnung"), &[]),
        ];
        let mut selection = FacetSelection::default();

        assert_eq!(
            facet_counts(&hits, &selection, &catalog).kinds,
            [("payslip".to_owned(), 2)]
        );
        selection.toggle(FacetValue::Kind("Lohnabrechnung".to_owned()), &catalog);
        assert_eq!(selection.kind.as_deref(), Some("payslip"));
        assert!(hits.iter().all(|hit| selection.admits_hit(hit, &catalog)));
    }

    #[test]
    fn a_value_of_a_single_valued_facet_replaces_the_chosen_one() {
        let mut selection = FacetSelection::default();

        selection.toggle(
            FacetValue::Kind("invoice".to_owned()),
            &KindCatalog::built_in(),
        );
        selection.toggle(
            FacetValue::Kind("contract".to_owned()),
            &KindCatalog::built_in(),
        );
        selection.toggle(FacetValue::Tag("a".to_owned()), &KindCatalog::built_in());
        selection.toggle(FacetValue::Tag("b".to_owned()), &KindCatalog::built_in());
        selection.toggle(FacetValue::Tag("A".to_owned()), &KindCatalog::built_in());
        selection.toggle(FacetValue::Sender("X".to_owned()), &KindCatalog::built_in());
        selection.toggle(
            FacetValue::Sender(" x ".to_owned()),
            &KindCatalog::built_in(),
        );

        assert_eq!(selection.kind.as_deref(), Some("contract"));
        assert_eq!(selection.tags, ["b".to_owned()].into());
        assert_eq!(selection.sender, None);
        assert!(!selection.is_empty());
        selection.toggle(
            FacetValue::Kind("Contract".to_owned()),
            &KindCatalog::built_in(),
        );
        selection.toggle(FacetValue::Tag("b".to_owned()), &KindCatalog::built_in());
        assert!(selection.is_empty());
    }

    #[test]
    fn the_spec_holds_the_trimmed_words_and_the_choices() {
        let mut search = DocumentSearch::new("  rent office ".to_owned(), Arc::default());
        search.toggle(FacetValue::Year(2024));
        search.toggle(FacetValue::Tag("office".to_owned()));

        let spec = search.spec();

        assert_eq!(spec.full_text.as_deref(), Some("rent office"));
        assert_eq!(spec.year, Some(2024));
        assert_eq!(spec.tags, ["office".to_owned()].into());
        let question = search.begin_search();
        assert_eq!(
            question,
            QuerySpec {
                full_text: Some("rent office".to_owned()),
                ..QuerySpec::default()
            }
        );
        assert!(search.is_busy());
        assert_eq!(
            DocumentSearch::new("  ".to_owned(), Arc::default())
                .spec()
                .full_text,
            None
        );
    }

    #[test]
    fn the_cursor_stays_inside_the_narrowed_list() {
        let mut search = loaded_search();
        search.move_cursor(5);
        assert_eq!(search.cursor(), 5);
        assert_eq!(
            search.selected().unwrap().path,
            PathBuf::from("archive/f.pdf")
        );

        search.toggle(FacetValue::Year(2025));

        assert_eq!(search.cursor(), 1);
        search.move_cursor(10);
        assert_eq!(search.cursor(), 1);
        search.move_cursor(-10);
        assert_eq!(search.cursor(), 0);
        search.select(7);
        assert_eq!(search.cursor(), 0);
    }

    #[test]
    fn a_failure_ends_busy_and_is_shown_and_a_search_clears_it() {
        let mut search = DocumentSearch::new(String::new(), Arc::default());
        assert!(search.is_busy());

        search.failed("the index is not open".to_owned());

        assert!(!search.is_busy());
        assert_eq!(search.problem(), Some("the index is not open"));
        search.set_text("rent".to_owned());
        assert!(search.is_busy());
        assert_eq!(search.problem(), None);
    }

    fn marked(snippet: &Snippet) -> Vec<&str> {
        snippet
            .marks
            .iter()
            .map(|mark| &snippet.text[mark.clone()])
            .collect()
    }

    #[test]
    fn a_snippet_loses_its_marks_and_keeps_where_they_stood() {
        let snippet = parse_snippet("…for Herr \u{2}Müller\u{3} and \u{2}Berlin\u{3}\nlines");

        assert_eq!(snippet.text, "…for Herr Müller and Berlin lines");
        assert_eq!(marked(&snippet), ["Müller", "Berlin"]);
    }

    #[test]
    fn stray_marks_in_a_snippet_do_not_panic_or_leave_a_mark_behind() {
        let closing_first = parse_snippet("a\u{3}b \u{2}c\u{3}");
        assert_eq!(closing_first.text, "ab c");
        assert_eq!(marked(&closing_first), ["c"]);

        let never_closed = parse_snippet("one \u{2}two three");
        assert_eq!(never_closed.text, "one two three");
        assert_eq!(marked(&never_closed), ["two three"]);

        let opened_twice = parse_snippet("\u{2}a \u{2}b\u{3} c");
        assert_eq!(opened_twice.text, "a b c");
        assert_eq!(marked(&opened_twice), ["a b"]);

        let empty = parse_snippet("x\u{2}\u{3}y\u{2}");
        assert_eq!(empty.text, "xy");
        assert!(empty.marks.is_empty());

        assert_eq!(
            parse_snippet(""),
            Snippet {
                text: String::new(),
                marks: Vec::new()
            }
        );
    }

    #[test]
    fn a_long_snippet_is_cut_around_its_first_match() {
        let snippet = parse_snippet(&format!(
            "{}\u{2}Müller\u{3}{}",
            "long ".repeat(20),
            " tail".repeat(20)
        ));

        let fitted = snippet.fitted(30);

        assert_eq!(fitted.text.chars().count(), 30);
        assert_eq!(marked(&fitted), ["Müller"]);
        assert_eq!(snippet.fitted(10_000), snippet);
    }

    #[test]
    fn snippets_belong_to_the_hits_found_and_go_with_a_new_search() {
        let mut search = DocumentSearch::new("rent".to_owned(), Arc::default());
        search.ran(six_hits());
        let (paths, words) = search.snippet_request().unwrap();
        assert_eq!(paths.len(), 6);
        assert_eq!(words, "rent");

        search.set_snippets(vec![(
            PathBuf::from("archive/a.pdf"),
            "the \u{2}rent\u{3}".to_owned(),
        )]);
        let hit = search
            .found()
            .iter()
            .find(|hit| hit.path.ends_with("a.pdf"))
            .unwrap();
        assert_eq!(marked(search.snippet_of(hit).unwrap()), ["rent"]);

        search.ran(six_hits());
        assert!(!search.has_snippets());
        assert!(DocumentSearch::new("  ".to_owned(), Arc::default())
            .snippet_request()
            .is_none());
    }

    #[test]
    fn the_preview_follows_the_selection_and_a_missing_document_has_none() {
        let mut search = loaded_search();
        let first = search.preview_request().unwrap();
        assert_eq!(first.path, PathBuf::from("archive/b.pdf"));
        assert!(search.preview_request().is_none());

        search.move_cursor(1);
        let second = search.preview_request().unwrap();
        assert_eq!(second.path, PathBuf::from("archive/a.pdf"));
        assert_ne!(second.serial, first.serial);

        let mut gone = found_hit("g.pdf", Some((2030, 1, 1)), None, None, &[]);
        gone.missing = true;
        let mut hits = six_hits();
        hits.push(gone);
        search.ran(hits);
        search.select(0);
        assert!(search.preview_request().is_none());
    }

    #[test]
    fn only_words_or_a_chosen_value_can_be_saved_as_a_report() {
        let mut search = loaded_search();
        assert!(!search.can_save_as_report());
        search.toggle(FacetValue::Year(2024));
        assert!(search.can_save_as_report());
        search.clear_facets();
        search.set_text("rent".to_owned());
        assert!(search.can_save_as_report());
    }

    /// Saving what the page shows as a report finds the same documents.
    #[test]
    fn a_saved_search_finds_the_documents_the_page_showed() {
        use std::sync::{Arc, Mutex};

        use chrono::{TimeZone as _, Utc};
        use ncrs_index::{ContentHash, DocumentSource, Index, NewDocument};

        use crate::query_work::QueryAccess;

        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        type Stock<'a> = (&'a str, i32, &'a str, &'a str, &'a [&'a str], &'a str);
        let stock: [Stock; 7] = [
            (
                "a",
                2025,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "rent for the office",
            ),
            (
                "b",
                2025,
                "firma a ",
                "Invoice",
                &["Office", "paid"],
                "office rent march",
            ),
            (
                "c",
                2025,
                "Firma A",
                "invoice",
                &["office"],
                "rent without the other tag",
            ),
            (
                "d",
                2024,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "rent of another year",
            ),
            (
                "e",
                2025,
                "Agency B",
                "invoice",
                &["office", "paid"],
                "rent of another sender",
            ),
            (
                "f",
                2025,
                "Firma A",
                "letter",
                &["office", "paid"],
                "rent of another kind",
            ),
            (
                "g",
                2025,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "insurance, no such word",
            ),
        ];
        for (seed, (name, year, sender, kind, tags, text)) in (1..).zip(stock) {
            let id = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: PathBuf::from(format!("archive/{name}.pdf")),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
                    stamp: None,
                })
                .unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        sender: Some(sender.to_owned()),
                        kind: Some(kind.to_owned()),
                        date: NaiveDate::from_ymd_opt(year, 5, 1),
                        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                        ..DocumentFields::default()
                    },
                    |_| ncrs_index::FieldOrigin::Unknown,
                )
                .unwrap();
            index.set_document_text(id, text).unwrap();
        }
        let access = QueryAccess::new(Arc::new(Mutex::new(Some(index))));

        // The last word is a prefix while it is typed.
        let mut search = DocumentSearch::new("office ren".to_owned(), Arc::default());
        let asked = search.begin_search();
        search.ran(access.run(&asked).unwrap());
        search.toggle(FacetValue::Year(2025));
        search.toggle(FacetValue::Sender("Firma A".to_owned()));
        search.toggle(FacetValue::Kind("invoice".to_owned()));
        search.toggle(FacetValue::Tag("office".to_owned()));
        search.toggle(FacetValue::Tag("paid".to_owned()));
        let shown: BTreeSet<PathBuf> = search.shown().map(|hit| hit.path.clone()).collect();
        assert_eq!(
            shown,
            BTreeSet::from([
                PathBuf::from("archive/a.pdf"),
                PathBuf::from("archive/b.pdf")
            ])
        );

        let id = access.save(None, "Office rent", &search.spec()).unwrap();
        let saved = access.load().unwrap();
        let query = saved.iter().find(|query| query.id == id).unwrap();
        let rerun: BTreeSet<PathBuf> = access
            .run(&query.spec)
            .unwrap()
            .into_iter()
            .map(|hit| hit.path)
            .collect();

        assert_eq!(rerun, shown);
    }

    fn hit_for(
        name: &str,
        recipient: &str,
        kind: &str,
        own: &[&str],
        cents: i64,
        currency: &str,
    ) -> QueryHit {
        let mut hit = found_hit(name, Some((2026, 3, 9)), Some("Telekom"), Some(kind), own);
        hit.fields.recipient = Some(recipient.to_owned());
        hit.fields.amount = Some(Amount {
            minor_units: cents,
            currency: ncrs_index::Currency::new(currency).unwrap(),
        });
        hit
    }

    #[test]
    fn the_recipient_narrows_counts_and_is_saved_as_the_tag_it_makes() {
        let mut search = DocumentSearch::new(String::new(), Arc::default());
        search.ran(vec![
            hit_for("a.pdf", "Firma A GmbH", "Rechnung", &["steuer"], 100, "EUR"),
            hit_for("b.pdf", "Marc privat", "Rechnung", &[], 250, "EUR"),
        ]);
        assert_eq!(
            search.counts().recipients,
            [
                ("Firma A GmbH".to_owned(), 1),
                ("Marc privat".to_owned(), 1)
            ]
        );

        search.toggle(FacetValue::Recipient("Firma A GmbH".to_owned()));

        assert_eq!(names(&search), ["a.pdf"]);
        assert!(search.spec().tags.contains("firma-a-gmbh"));
        assert_eq!(search.spec().year, None);
    }

    #[test]
    fn a_chosen_tag_also_matches_the_automatic_and_the_more_specific_tags() {
        let mut search = DocumentSearch::new(String::new(), Arc::default());
        search.ran(vec![
            hit_for("a.pdf", "Firma A GmbH", "Rechnung", &[], 100, "EUR"),
            hit_for("b.pdf", "Firma AB", "Rechnung", &["steuer"], 100, "EUR"),
        ]);
        assert_eq!(search.counts().tags, [("steuer".to_owned(), 1)]);

        search.toggle(FacetValue::Tag("firma-a".to_owned()));
        assert_eq!(names(&search), ["a.pdf"]);
        search.toggle(FacetValue::Tag("firma-a".to_owned()));
        search.toggle(FacetValue::Tag("rechnung".to_owned()));
        assert_eq!(search.shown_len(), 2);
    }

    #[test]
    fn the_total_is_only_given_for_one_currency() {
        let mut search = DocumentSearch::new(String::new(), Arc::default());
        search.ran(vec![
            hit_for("a.pdf", "X", "Rechnung", &[], 100, "EUR"),
            hit_for("b.pdf", "X", "Rechnung", &[], 250, "EUR"),
        ]);
        assert_eq!(search.total_amount().map(|sum| sum.minor_units), Some(350));

        search.ran(vec![
            hit_for("a.pdf", "X", "Rechnung", &[], 100, "EUR"),
            hit_for("b.pdf", "X", "Rechnung", &[], 250, "USD"),
        ]);
        assert_eq!(search.total_amount(), None);
    }
}
