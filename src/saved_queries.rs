//! State of the reports page: the list of saved queries, the one under the
//! cursor (or a search not saved yet), what running it found, and the steps of
//! deleting and exporting. The criteria are only shown here; they are changed
//! in the search. No widgets and no tasks; `App` owns it and changes it from
//! `update`, the view is `ui::saved_queries`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::mouse::ScrollDelta;

use ncrs_index::{tag_name, Amount, Kinds, QuerySpec, SavedQuery, SavedQueryId};

use crate::i18n::{Language, Msg};
use crate::list_window::ListWindow;
use crate::query_work::{copies_of, ExportSummary, QueryHit};

/// A ready-made report the user starts from; the search it opens is theirs
/// to change before saving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportTemplate {
    InvoicesOfYear {
        year: i32,
    },
    Contracts,
    /// Invoices that carry the tag for open.
    OpenInvoices,
}

/// The templates on offer: the invoices of `this_year`, contracts and open
/// invoices.
pub fn templates(this_year: i32) -> Vec<ReportTemplate> {
    vec![
        ReportTemplate::InvoicesOfYear { year: this_year },
        ReportTemplate::Contracts,
        ReportTemplate::OpenInvoices,
    ]
}

impl ReportTemplate {
    /// The name of the template, as menu entry and as name of the report.
    pub fn name(&self, lang: Language) -> String {
        match self {
            Self::InvoicesOfYear { .. } => lang
                .text(Msg::SavedQueriesTemplateInvoicesOfYear)
                .to_owned(),
            Self::Contracts => lang.text(Msg::SavedQueriesTemplateContracts).to_owned(),
            Self::OpenInvoices => lang.text(Msg::SavedQueriesTemplateOpenInvoices).to_owned(),
        }
    }

    pub fn spec(&self, lang: Language) -> QuerySpec {
        match self {
            Self::InvoicesOfYear { year } => QuerySpec {
                year: Some(*year),
                kind: Some("invoice".to_owned()),
                ..QuerySpec::default()
            },
            Self::Contracts => QuerySpec {
                kind: Some("contract".to_owned()),
                ..QuerySpec::default()
            },
            Self::OpenInvoices => QuerySpec {
                kind: Some("invoice".to_owned()),
                tags: [tag_name(lang.text(Msg::SavedQueriesTagOpen))].into(),
                ..QuerySpec::default()
            },
        }
    }
}

/// A search not saved as a report yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub spec: QuerySpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    NameMissing,
    /// The work on the index or the files failed; the detail stays English.
    Failed(String),
}

/// What running a query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Results {
    pub hits: Vec<QueryHit>,
    /// The detail lists the files that are gone instead of the first hits.
    pub missing_shown: bool,
}

impl Results {
    /// One sum per currency over every hit with an amount, missing files
    /// included, by currency code. Currencies are never added together.
    pub fn totals(&self) -> Vec<Amount> {
        let mut sums: Vec<Amount> = Vec::new();
        for amount in self
            .hits
            .iter()
            .filter_map(|hit| hit.fields.amount.as_ref())
        {
            match sums.iter_mut().find(|sum| sum.currency == amount.currency) {
                Some(sum) => sum.minor_units = sum.minor_units.saturating_add(amount.minor_units),
                None => sums.push(amount.clone()),
            }
        }
        sums.sort_by(|left, right| left.currency.as_str().cmp(right.currency.as_str()));
        sums
    }

    pub fn missing(&self) -> usize {
        self.hits.iter().filter(|hit| hit.missing).count()
    }
}

/// The export waits for the user's go: where to, and how many files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportStep {
    pub folder: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub id: Option<SavedQueryId>,
    pub name: String,
    pub spec: QuerySpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub hits: Vec<QueryHit>,
    pub folder: PathBuf,
}

/// Most reports of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct SavedQueries {
    rows: Vec<SavedQuery>,
    /// `None` is the search that is not saved yet, if there is one.
    cursor: Option<usize>,
    window: ListWindow,
    /// The list is loading or the work is running; the view takes no new task.
    busy: bool,
    /// What is typed in the name field.
    name: String,
    problem: Option<Problem>,
    results: Option<Results>,
    /// Which run the results are waiting for; an older answer is dropped.
    run_serial: u64,
    /// Hits counted so far, by query, for the list.
    hit_counts: HashMap<SavedQueryId, usize>,
    confirming_delete: bool,
    export: Option<ExportStep>,
    exported: Option<ExportSummary>,
    /// The query to put the cursor on when the list is loaded next.
    select_after_load: Option<SavedQueryId>,
    /// A search to show as the unsaved row once the list is there.
    pending_draft: Option<Draft>,
    unsaved: Option<Draft>,
    /// The year the templates count back from.
    this_year: i32,
    kinds: Kinds,
}

impl SavedQueries {
    /// Opens busy: the list loads with the view.
    pub fn new(this_year: i32) -> Self {
        Self {
            rows: Vec::new(),
            cursor: None,
            window: ListWindow::default(),
            busy: true,
            name: String::new(),
            problem: None,
            results: None,
            run_serial: 0,
            hit_counts: HashMap::new(),
            confirming_delete: false,
            export: None,
            exported: None,
            select_after_load: None,
            pending_draft: None,
            unsaved: None,
            this_year,
            kinds: Kinds::default(),
        }
    }

    pub fn with_kinds(self, kinds: Kinds) -> Self {
        Self { kinds, ..self }
    }

    pub fn set_kinds(&mut self, kinds: Kinds) {
        self.kinds = kinds;
    }

    pub const fn kinds(&self) -> &Kinds {
        &self.kinds
    }

    pub fn templates(&self) -> Vec<ReportTemplate> {
        templates(self.this_year)
    }

    pub fn rows(&self) -> &[SavedQuery] {
        &self.rows
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// The first query of the list that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.rows.len(), VISIBLE_ROWS)
    }

    pub fn selected(&self) -> Option<&SavedQuery> {
        let index = self.cursor?;
        self.rows.get(index)
    }

    /// The search that is not saved yet, while its row is the selected one.
    pub fn unsaved(&self) -> Option<&Draft> {
        self.unsaved.as_ref().filter(|_| self.cursor.is_none())
    }

    /// What the selected row asks the index.
    pub fn selected_spec(&self) -> Option<&QuerySpec> {
        match self.selected() {
            Some(query) => Some(&query.spec),
            None => self.unsaved().map(|draft| &draft.spec),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    pub fn results(&self) -> Option<&Results> {
        self.results.as_ref()
    }

    pub fn hit_count(&self, id: SavedQueryId) -> Option<usize> {
        self.hit_counts.get(&id).copied()
    }

    pub fn is_confirming_delete(&self) -> bool {
        self.confirming_delete
    }

    pub fn export_step(&self) -> Option<&ExportStep> {
        self.export.as_ref()
    }

    /// What the last export did, until something else is done.
    pub fn exported(&self) -> Option<&ExportSummary> {
        self.exported.as_ref()
    }

    /// How many files the export would copy.
    pub fn copies(&self) -> usize {
        self.results
            .as_ref()
            .map_or(0, |results| copies_of(&results.hits))
    }

    /// A step the user can still back out of is up.
    pub fn has_open_step(&self) -> bool {
        self.confirming_delete || self.export.is_some()
    }

    /// Backs out of the step that is up; whether there was one.
    pub fn cancel_step(&mut self) -> bool {
        let was_open = self.has_open_step();
        self.confirming_delete = false;
        self.export = None;
        was_open
    }

    /// The list the work returned. The cursor goes to the query that was just
    /// saved, else stays on the one it was on if it is still there, else on
    /// the first.
    pub fn loaded(&mut self, rows: Vec<SavedQuery>) {
        let wanted = self
            .select_after_load
            .take()
            .or_else(|| self.selected().map(|query| query.id));
        self.rows = rows;
        self.busy = false;
        if let Some(draft) = self.pending_draft.take() {
            self.cursor = None;
            self.name.clone_from(&draft.name);
            self.unsaved = Some(draft);
            self.leave_query();
            return;
        }
        self.cursor = wanted.and_then(|id| self.rows.iter().position(|query| query.id == id));
        if self.cursor.is_none() && self.unsaved.is_none() && !self.rows.is_empty() {
            self.cursor = Some(0);
        }
        if let Some(cursor) = self.cursor {
            self.window
                .ensure_visible(cursor, self.rows.len(), VISIBLE_ROWS);
        }
        if let Some(name) = self.selected().map(|query| query.name.clone()) {
            self.name = name;
        }
    }

    /// Puts the cursor on the query `id` once the list has loaded.
    pub fn select_when_loaded(&mut self, id: SavedQueryId) {
        self.select_after_load = Some(id);
    }

    /// The name field holds another name than the selected report has.
    pub fn name_is_changed(&self) -> bool {
        self.selected()
            .is_some_and(|query| query.name != self.name.trim())
    }

    /// Shows `draft` as the unsaved row as soon as the list has loaded, so the
    /// list does not replace it.
    pub fn prefill(&mut self, draft: Draft) {
        self.pending_draft = Some(draft);
    }

    /// The unsaved row the list is waiting to show.
    pub fn is_prefilled(&self) -> bool {
        self.pending_draft.is_some()
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.problem = Some(Problem::Failed(detail));
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let Some(last) = self.rows.len().checked_sub(1) else {
            return;
        };
        let next = match self.cursor {
            Some(index) => index.saturating_add_signed(delta).min(last),
            None if delta > 0 => 0,
            None => last,
        };
        self.select(next);
    }

    pub fn select(&mut self, index: usize) {
        if self.busy || self.cursor == Some(index) {
            return;
        }
        let Some(query) = self.rows.get(index) else {
            return;
        };
        self.name.clone_from(&query.name);
        self.cursor = Some(index);
        self.unsaved = None;
        self.window
            .ensure_visible(index, self.rows.len(), VISIBLE_ROWS);
        self.leave_query();
    }

    /// The wheel moves the window; a query under the cursor follows it when
    /// the cursor would leave. The unsaved row stays the one shown.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        let mut moved = self.cursor.unwrap_or_default();
        self.window
            .scroll(delta, self.rows.len(), VISIBLE_ROWS, &mut moved);
        if self.cursor.is_some() {
            self.select(moved);
        }
    }

    fn leave_query(&mut self) {
        self.problem = None;
        self.results = None;
        self.confirming_delete = false;
        self.export = None;
        self.exported = None;
        self.run_serial = self.run_serial.wrapping_add(1);
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
        self.problem = None;
    }

    /// The report as the name field holds it, to be saved: new, or a rename of
    /// the one under the cursor. A missing name is reported and nothing is
    /// returned.
    pub fn begin_save(&mut self) -> Option<SaveRequest> {
        if self.busy {
            return None;
        }
        let spec = self.selected_spec()?.clone();
        let name = self.name.trim().to_owned();
        if name.is_empty() {
            self.problem = Some(Problem::NameMissing);
            return None;
        }
        self.busy = true;
        self.problem = None;
        Some(SaveRequest {
            id: self.selected().map(|query| query.id),
            name,
            spec,
        })
    }

    /// The query is saved; the list is loaded again, with the cursor on it.
    pub fn saved(&mut self, id: SavedQueryId) {
        self.select_after_load = Some(id);
        self.unsaved = None;
        self.hit_counts.remove(&id);
        self.results = None;
    }

    /// Asks "really delete?" in the page, for the query under the cursor.
    pub fn ask_delete(&mut self) {
        if !self.busy && self.selected().is_some() {
            self.export = None;
            self.confirming_delete = true;
        }
    }

    pub fn begin_delete(&mut self) -> Option<SavedQueryId> {
        if self.busy || !self.confirming_delete {
            return None;
        }
        let id = self.selected()?.id;
        self.confirming_delete = false;
        self.busy = true;
        self.problem = None;
        Some(id)
    }

    /// The query is gone; the list is loaded again with the cursor nowhere.
    pub fn deleted(&mut self, id: SavedQueryId) {
        self.hit_counts.remove(&id);
        self.cursor = None;
        self.name.clear();
        self.leave_query();
    }

    /// The selected criteria as a question to the index, with the number of
    /// the run its answer is for; nothing when no row is selected.
    pub fn begin_run(&mut self) -> Option<(u64, QuerySpec)> {
        let spec = self.selected_spec()?.clone();
        self.run_serial = self.run_serial.wrapping_add(1);
        self.problem = None;
        self.results = None;
        self.export = None;
        self.exported = None;
        Some((self.run_serial, spec))
    }

    /// The answer of run `serial`; one of an older run is dropped.
    pub fn ran(&mut self, serial: u64, hits: Vec<QueryHit>) {
        if serial != self.run_serial {
            return;
        }
        if let Some(id) = self.selected().map(|query| query.id) {
            self.hit_counts.insert(id, hits.len());
        }
        self.results = Some(Results {
            hits,
            missing_shown: false,
        });
    }

    /// The hits counted for the list.
    pub fn counted(&mut self, counts: Vec<(SavedQueryId, usize)>) {
        self.hit_counts.extend(counts);
    }

    pub fn toggle_missing(&mut self) {
        if let Some(results) = self.results.as_mut() {
            results.missing_shown = !results.missing_shown;
        }
    }

    /// Opens the step that asks before anything is copied. Only a result with
    /// files to copy can be exported.
    pub fn start_export(&mut self, folder: PathBuf) {
        if !self.busy && self.copies() > 0 {
            self.confirming_delete = false;
            self.exported = None;
            self.export = Some(ExportStep { folder });
        }
    }

    pub fn set_export_folder(&mut self, folder: &Path) {
        if let Some(step) = self.export.as_mut() {
            folder.clone_into(&mut step.folder);
        }
    }

    /// What to copy and where, once the user said go.
    pub fn begin_export(&mut self) -> Option<ExportRequest> {
        if self.busy {
            return None;
        }
        let folder = self.export.take()?.folder;
        let hits = self.results.as_ref()?.hits.clone();
        self.busy = true;
        self.problem = None;
        Some(ExportRequest { hits, folder })
    }

    pub fn export_done(&mut self, summary: ExportSummary) {
        self.busy = false;
        self.exported = Some(summary);
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
    use ncrs_index::{DocumentFields, Index};

    use super::*;

    pub const THIS_YEAR: i32 = 2026;

    /// Ids come from a real index: only the index hands them out.
    pub fn stored_queries() -> Vec<SavedQuery> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        for (name, year) in [("Company A 2024", 2024), ("Taxes 2023", 2023)] {
            let spec = QuerySpec {
                year: Some(year),
                ..QuerySpec::default()
            };
            index.save_query(name, &spec).unwrap();
        }
        index.saved_queries().unwrap()
    }

    pub fn hit(name: &str, missing: bool) -> QueryHit {
        QueryHit {
            path: PathBuf::from(format!("archive/{name}")),
            fields: DocumentFields::default(),
            missing,
            stamp: None,
        }
    }

    pub fn loaded_list() -> SavedQueries {
        let mut list = SavedQueries::new(THIS_YEAR);
        list.loaded(stored_queries());
        list
    }

    pub fn ran_list() -> SavedQueries {
        let mut list = loaded_list();
        let (serial, _) = list.begin_run().unwrap();
        list.ran(
            serial,
            vec![hit("a.pdf", false), hit("b.pdf", false), hit("c.pdf", true)],
        );
        list
    }

    fn amount(minor_units: i64, currency: &str) -> Amount {
        Amount {
            minor_units,
            currency: ncrs_index::Currency::new(currency).unwrap(),
        }
    }

    fn hit_with(amount: Option<Amount>, missing: bool) -> QueryHit {
        QueryHit {
            fields: DocumentFields {
                amount,
                ..DocumentFields::default()
            },
            ..hit("x.pdf", missing)
        }
    }

    #[test]
    fn templates_are_the_invoices_of_this_year_then_contracts_and_open_invoices() {
        assert_eq!(
            templates(2026),
            vec![
                ReportTemplate::InvoicesOfYear { year: 2026 },
                ReportTemplate::Contracts,
                ReportTemplate::OpenInvoices,
            ]
        );
    }

    #[test]
    fn a_template_asks_for_exactly_its_criteria() {
        for lang in [Language::English, Language::German] {
            assert_eq!(
                ReportTemplate::InvoicesOfYear { year: 2026 }.spec(lang),
                QuerySpec {
                    year: Some(2026),
                    kind: Some("invoice".to_owned()),
                    ..QuerySpec::default()
                }
            );
            assert_eq!(
                ReportTemplate::Contracts.spec(lang).kind,
                Some("contract".to_owned())
            );
        }
        assert_eq!(
            ReportTemplate::OpenInvoices.spec(Language::German),
            QuerySpec {
                kind: Some("invoice".to_owned()),
                tags: ["offen".to_owned()].into(),
                ..QuerySpec::default()
            }
        );
        assert_eq!(
            ReportTemplate::OpenInvoices.name(Language::English),
            "Open invoices"
        );
    }

    #[test]
    fn totals_add_up_per_currency_by_code_and_skip_hits_without_amount() {
        let results = Results {
            hits: vec![
                hit_with(Some(amount(1_000, "USD")), false),
                hit_with(Some(amount(250, "EUR")), false),
                hit_with(None, false),
                hit_with(Some(amount(-50, "EUR")), true),
                hit_with(Some(amount(1, "USD")), false),
            ],
            missing_shown: false,
        };

        assert_eq!(
            results.totals(),
            vec![amount(200, "EUR"), amount(1_001, "USD")]
        );
        let none = Results {
            hits: vec![hit_with(None, false)],
            missing_shown: false,
        };
        assert!(none.totals().is_empty());
    }

    #[test]
    fn totals_saturate_instead_of_wrapping() {
        let results = Results {
            hits: vec![
                hit_with(Some(amount(i64::MAX, "EUR")), false),
                hit_with(Some(amount(5, "EUR")), false),
            ],
            missing_shown: false,
        };

        assert_eq!(results.totals(), vec![amount(i64::MAX, "EUR")]);
    }

    #[test]
    fn the_page_opens_busy_and_the_loaded_list_selects_the_first_report() {
        let mut list = SavedQueries::new(THIS_YEAR);
        assert!(list.is_busy());
        assert!(list.selected().is_none());

        list.loaded(stored_queries());

        assert!(!list.is_busy());
        assert_eq!(list.cursor(), Some(0));
        assert_eq!(list.name(), "Company A 2024");
    }

    #[test]
    fn selecting_a_report_takes_its_name_and_clears_what_was_found() {
        let mut list = ran_list();
        assert!(list.results().is_some());

        list.select(1);

        assert_eq!(list.cursor(), Some(1));
        assert_eq!(list.name(), "Taxes 2023");
        assert!(list.results().is_none());
    }

    #[test]
    fn running_asks_for_the_criteria_of_the_selection_and_counts_the_hits() {
        let mut list = loaded_list();
        let id = list.selected().unwrap().id;

        let (serial, spec) = list.begin_run().unwrap();
        assert_eq!(spec.year, Some(2024));
        list.ran(serial, vec![hit("a.pdf", false), hit("b.pdf", true)]);

        assert_eq!(list.results().unwrap().hits.len(), 2);
        assert_eq!(list.results().unwrap().missing(), 1);
        assert_eq!(list.hit_count(id), Some(2));
    }

    #[test]
    fn the_answer_of_an_older_run_is_dropped() {
        let mut list = loaded_list();
        let (old, _) = list.begin_run().unwrap();
        list.select(1);
        let (current, _) = list.begin_run().unwrap();

        list.ran(old, vec![hit("a.pdf", false)]);
        assert!(list.results().is_none());
        list.ran(current, vec![hit("b.pdf", false)]);
        assert_eq!(list.results().unwrap().hits.len(), 1);
    }

    #[test]
    fn a_search_handed_over_is_the_unsaved_row_until_it_is_saved() {
        let mut list = SavedQueries::new(THIS_YEAR);
        let spec = QuerySpec {
            year: Some(2025),
            ..QuerySpec::default()
        };
        list.prefill(Draft {
            name: "Mine".to_owned(),
            spec: spec.clone(),
        });
        assert!(list.is_prefilled());

        list.loaded(stored_queries());

        assert_eq!(list.cursor(), None);
        assert_eq!(list.unsaved().map(|draft| &draft.spec), Some(&spec));
        assert_eq!(list.name(), "Mine");
        let request = list.begin_save().unwrap();
        assert_eq!(request.id, None);
        assert_eq!(request.spec, spec);

        list.saved(stored_queries()[0].id);
        assert!(list.unsaved().is_none());
    }

    #[test]
    fn leaving_the_unsaved_row_drops_it() {
        let mut list = SavedQueries::new(THIS_YEAR);
        list.prefill(Draft {
            name: String::new(),
            spec: QuerySpec::default(),
        });
        list.loaded(stored_queries());

        list.select(0);

        assert!(list.unsaved().is_none());
        assert_eq!(list.cursor(), Some(0));
    }

    #[test]
    fn saving_needs_a_name_and_a_rename_changes_the_selected_report() {
        let mut list = loaded_list();
        let id = list.selected().unwrap().id;

        list.set_name("  ".to_owned());
        assert!(list.begin_save().is_none());
        assert_eq!(list.problem(), Some(&Problem::NameMissing));

        list.set_name("Renamed".to_owned());
        let request = list.begin_save().unwrap();
        assert_eq!(request.id, Some(id));
        assert_eq!(request.name, "Renamed");
        assert!(list.is_busy());
    }

    #[test]
    fn deleting_asks_first_and_only_the_second_step_deletes() {
        let mut list = loaded_list();
        assert!(list.begin_delete().is_none());

        list.ask_delete();
        assert!(list.is_confirming_delete());
        assert!(list.cancel_step());
        assert!(list.begin_delete().is_none());

        list.ask_delete();
        let id = list.begin_delete().unwrap();
        list.deleted(id);
        assert_eq!(list.cursor(), None);
    }

    #[test]
    fn the_missing_files_are_listed_on_demand() {
        let mut list = ran_list();
        assert!(!list.results().unwrap().missing_shown);

        list.toggle_missing();

        assert!(list.results().unwrap().missing_shown);
    }

    #[test]
    fn the_export_asks_first_and_only_the_go_hands_over_the_files() {
        let mut list = ran_list();
        assert!(list.begin_export().is_none());

        list.start_export(PathBuf::from("out"));
        assert_eq!(list.copies(), 2);
        let request = list.begin_export().unwrap();
        assert_eq!(request.folder, PathBuf::from("out"));
        assert_eq!(request.hits.len(), 3);

        list.export_done(ExportSummary {
            copied: 2,
            skipped_existing: 0,
            skipped_missing: 1,
            csv_path: PathBuf::from("out/x.csv"),
        });
        assert!(list.exported().is_some());
        assert!(!list.is_busy());
    }

    #[test]
    fn nothing_to_copy_means_no_export_step() {
        let mut list = loaded_list();
        let (serial, _) = list.begin_run().unwrap();
        list.ran(serial, vec![hit("gone.pdf", true)]);

        list.start_export(PathBuf::from("out"));

        assert!(list.export_step().is_none());
    }

    #[test]
    fn the_counts_of_the_list_are_kept_by_report() {
        let mut list = loaded_list();
        let ids: Vec<_> = list.rows().iter().map(|query| query.id).collect();

        list.counted(vec![(ids[0], 38), (ids[1], 7)]);

        assert_eq!(list.hit_count(ids[0]), Some(38));
        assert_eq!(list.hit_count(ids[1]), Some(7));
    }

    #[test]
    fn a_failure_ends_busy_and_is_shown() {
        let mut list = SavedQueries::new(THIS_YEAR);

        list.failed("boom".to_owned());

        assert!(!list.is_busy());
        assert_eq!(list.problem(), Some(&Problem::Failed("boom".to_owned())));
    }
}
