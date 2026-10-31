//! State of the document search page: the words typed, the documents the index
//! found for them, the facets chosen to narrow them and the cursor in the
//! narrowed list. No widgets and no tasks; `App` owns it and changes it from
//! `update`, the view is `ui::document_search`.
//!
//! The facets mirror what a report can save: at most one value of year, tax
//! context, sender and kind, and any number of tags that must all be there.
//! Names are compared like the index compares them, trimmed and without regard
//! to case.

use std::collections::{BTreeMap, BTreeSet};

use chrono::Datelike as _;
use iced::mouse::ScrollDelta;

use ncrs_index::{QuerySpec, TaxContext, TaxContextEntry};

use crate::list_window::ListWindow;
use crate::query_work::QueryHit;

/// Most documents of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetSelection {
    pub year: Option<i32>,
    pub tax_context: Option<TaxContext>,
    pub sender: Option<String>,
    pub kind: Option<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetValue {
    Year(i32),
    TaxContext(TaxContext),
    Sender(String),
    Kind(String),
    Tag(String),
}

/// A facet, to leave it out when its own values are counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Facet {
    Year,
    TaxContext,
    Sender,
    Kind,
    Tags,
}

/// How the index compares names: trimmed, without regard to case.
fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn tax_context_key(tax_context: &TaxContext) -> String {
    name_key(tax_context.as_str())
}

impl FacetSelection {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Whether the value is one of the chosen ones.
    pub fn holds(&self, value: &FacetValue) -> bool {
        let same = |chosen: &Option<String>, other: &str| {
            chosen
                .as_deref()
                .is_some_and(|name| name_key(name) == name_key(other))
        };
        match value {
            FacetValue::Year(year) => self.year == Some(*year),
            FacetValue::TaxContext(other) => self
                .tax_context
                .as_ref()
                .is_some_and(|held| tax_context_key(held) == tax_context_key(other)),
            FacetValue::Sender(other) => same(&self.sender, other),
            FacetValue::Kind(other) => same(&self.kind, other),
            FacetValue::Tag(other) => self
                .tags
                .iter()
                .any(|held| name_key(held) == name_key(other)),
        }
    }

    /// Chooses the value, or drops it when it is chosen already. A value of a
    /// facet that takes one replaces the other; a tag joins the others.
    pub fn toggle(&mut self, value: FacetValue) {
        match value {
            FacetValue::Year(year) => {
                self.year = (self.year != Some(year)).then_some(year);
            }
            FacetValue::TaxContext(chosen) => {
                let chosen_already = self
                    .tax_context
                    .as_ref()
                    .is_some_and(|held| tax_context_key(held) == tax_context_key(&chosen));
                self.tax_context = (!chosen_already).then_some(chosen);
            }
            FacetValue::Sender(chosen) => {
                let chosen_already = self
                    .sender
                    .as_ref()
                    .is_some_and(|held| name_key(held) == name_key(&chosen));
                self.sender = (!chosen_already).then_some(chosen);
            }
            FacetValue::Kind(chosen) => {
                let chosen_already = self
                    .kind
                    .as_ref()
                    .is_some_and(|held| name_key(held) == name_key(&chosen));
                self.kind = (!chosen_already).then_some(chosen);
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
    fn admits(&self, hit: &QueryHit, except: Option<Facet>) -> bool {
        let fields = &hit.fields;
        let counts = |facet: Facet| except != Some(facet);
        if let Some(year) = self.year.filter(|_| counts(Facet::Year)) {
            if fields.date.map(|date| date.year()) != Some(year) {
                return false;
            }
        }
        if let Some(wanted) = self
            .tax_context
            .as_ref()
            .filter(|_| counts(Facet::TaxContext))
        {
            let same = fields
                .tax_context
                .as_ref()
                .is_some_and(|held| tax_context_key(held) == tax_context_key(wanted));
            if !same {
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
        if counts(Facet::Kind) && !same_name(&self.kind, &fields.kind) {
            return false;
        }
        if counts(Facet::Tags) {
            let held: BTreeSet<String> = fields.tags.iter().map(|tag| name_key(tag)).collect();
            if !self.tags.iter().all(|tag| held.contains(&name_key(tag))) {
                return false;
            }
        }
        true
    }

    pub fn admits_hit(&self, hit: &QueryHit) -> bool {
        self.admits(hit, None)
    }
}

/// Per facet the values found with the number of documents that have them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetCounts {
    /// Newest year first.
    pub years: Vec<(i32, usize)>,
    pub tax_contexts: Vec<(TaxContext, usize)>,
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

/// The values of every facet. A facet that takes one value counts over the
/// hits that fit all the other choices, so the user can switch from one value
/// to another; the tags count over the hits that fit everything chosen.
pub fn facet_counts(hits: &[QueryHit], selection: &FacetSelection) -> FacetCounts {
    let among =
        |except: Option<Facet>| hits.iter().filter(move |hit| selection.admits(hit, except));

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

    let tax_contexts = tally(
        among(Some(Facet::TaxContext))
            .filter_map(|hit| hit.fields.tax_context.clone())
            .map(|tax_context| (tax_context_key(&tax_context), tax_context)),
        selection
            .tax_context
            .iter()
            .map(|tax_context| (tax_context_key(tax_context), tax_context.clone())),
    );
    let senders = tally(
        among(Some(Facet::Sender))
            .filter_map(|hit| hit.fields.sender.as_deref())
            .filter(|sender| !sender.trim().is_empty())
            .map(named),
        selection.sender.iter().map(|sender| named(sender)),
    );
    let kinds = tally(
        among(Some(Facet::Kind))
            .filter_map(|hit| hit.fields.kind.as_deref())
            .filter(|kind| !kind.trim().is_empty())
            .map(named),
        selection.kind.iter().map(|kind| named(kind)),
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
        tax_contexts,
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
    tax_contexts: Vec<TaxContextEntry>,
}

impl DocumentSearch {
    /// Opens busy: the first search starts with the page.
    pub fn new(text: String) -> Self {
        Self {
            text,
            hits: Vec::new(),
            selection: FacetSelection::default(),
            shown: Vec::new(),
            counts: FacetCounts::default(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            problem: None,
            tax_contexts: Vec::new(),
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

    pub fn counts(&self) -> &FacetCounts {
        &self.counts
    }

    pub fn tax_contexts(&self) -> &[TaxContextEntry] {
        &self.tax_contexts
    }

    pub fn set_tax_contexts(&mut self, tax_contexts: Vec<TaxContextEntry>) {
        self.tax_contexts = tax_contexts;
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
    #[cfg(test)]
    pub fn spec(&self) -> QuerySpec {
        let selection = &self.selection;
        let text = self.text.trim();
        QuerySpec {
            year: selection.year,
            tax_context: selection.tax_context.clone(),
            sender: selection.sender.clone(),
            kind: selection.kind.clone(),
            tags: selection.tags.clone(),
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
        self.begin_search()
    }

    pub fn ran(&mut self, mut hits: Vec<QueryHit>) {
        hits.sort_by(display_order);
        self.hits = hits;
        self.busy = false;
        self.problem = None;
        self.refilter();
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.problem = Some(detail);
    }

    pub fn toggle(&mut self, value: FacetValue) {
        self.selection.toggle(value);
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
            .filter(|(_, hit)| self.selection.admits_hit(hit))
            .map(|(at, _)| at)
            .collect();
        self.counts = facet_counts(&self.hits, &self.selection);
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
                tax_context: Some(TaxContext::named("company")),
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
        let mut search = DocumentSearch::new(String::new());
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
                ("Invoice".to_owned(), 3),
                ("contract".to_owned(), 1),
                ("letter".to_owned(), 1)
            ]
        );
        assert_eq!(
            search.counts().tax_contexts,
            [(TaxContext::named("company"), 6)]
        );
    }

    #[test]
    fn a_value_of_a_single_valued_facet_replaces_the_chosen_one() {
        let mut selection = FacetSelection::default();

        selection.toggle(FacetValue::Kind("invoice".to_owned()));
        selection.toggle(FacetValue::Kind("contract".to_owned()));
        selection.toggle(FacetValue::Tag("a".to_owned()));
        selection.toggle(FacetValue::Tag("b".to_owned()));
        selection.toggle(FacetValue::Tag("A".to_owned()));
        selection.toggle(FacetValue::Sender("X".to_owned()));
        selection.toggle(FacetValue::Sender(" x ".to_owned()));

        assert_eq!(selection.kind.as_deref(), Some("contract"));
        assert_eq!(selection.tags, ["b".to_owned()].into());
        assert_eq!(selection.sender, None);
        assert!(!selection.is_empty());
        selection.toggle(FacetValue::Kind("Contract".to_owned()));
        selection.toggle(FacetValue::Tag("b".to_owned()));
        assert!(selection.is_empty());
    }

    #[test]
    fn the_spec_holds_the_trimmed_words_and_the_choices() {
        let mut search = DocumentSearch::new("  rent office ".to_owned());
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
        assert_eq!(DocumentSearch::new("  ".to_owned()).spec().full_text, None);
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
        let mut search = DocumentSearch::new(String::new());
        assert!(search.is_busy());

        search.failed("the index is not open".to_owned());

        assert!(!search.is_busy());
        assert_eq!(search.problem(), Some("the index is not open"));
        search.set_text("rent".to_owned());
        assert!(search.is_busy());
        assert_eq!(search.problem(), None);
    }
}
