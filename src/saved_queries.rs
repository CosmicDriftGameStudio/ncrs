//! State of the saved queries page: the list of queries, the form of the one
//! under the cursor (or a new one), what running it found, and the steps of
//! deleting and exporting. No widgets and no tasks; `App` owns it and changes
//! it from `update`, the view is `ui::saved_queries`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use iced::mouse::ScrollDelta;

use ncrs_index::{tag_name, Amount, QuerySpec, SavedQuery, SavedQueryId};

use crate::i18n::{Language, Msg};
use crate::list_window::ListWindow;
use crate::query_work::{copies_of, ExportSummary, QueryHit};

/// A text field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryField {
    Name,
    Year,
    Sender,
    Kind,
    /// The field a tag is typed in; the tags themselves are chips.
    Tags,
    FullText,
}

/// A ready-made report the user starts from; the form it fills is theirs to
/// change before saving.
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
    /// The name of the template, as button and as name of the report.
    pub fn name(&self, lang: Language) -> String {
        match self {
            Self::InvoicesOfYear { .. } => lang
                .text(Msg::SavedQueriesTemplateInvoicesOfYear)
                .to_owned(),
            Self::Contracts => lang.text(Msg::SavedQueriesTemplateContracts).to_owned(),
            Self::OpenInvoices => lang.text(Msg::SavedQueriesTemplateOpenInvoices).to_owned(),
        }
    }

    pub fn form(&self, lang: Language) -> Form {
        let named = Form {
            name: self.name(lang),
            ..Form::default()
        };
        match self {
            Self::InvoicesOfYear { year } => Form {
                year: year.to_string(),
                kind: "invoice".to_owned(),
                ..named
            },
            Self::Contracts => Form {
                kind: "contract".to_owned(),
                ..named
            },
            Self::OpenInvoices => Form {
                kind: "invoice".to_owned(),
                tags: BTreeSet::from([tag_name(lang.text(Msg::SavedQueriesTagOpen))]),
                ..named
            },
        }
    }
}

/// The criteria as the user sees and edits them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Form {
    pub name: String,
    pub year: String,
    pub sender: String,
    pub kind: String,
    pub tags: BTreeSet<String>,
    /// What is typed for the next tag.
    pub tag_input: String,
    pub full_text: String,
}

impl Form {
    /// A form that asks what `spec` asks, without a name.
    pub fn of_spec(spec: &QuerySpec) -> Self {
        Self {
            name: String::new(),
            year: spec.year.map(|year| year.to_string()).unwrap_or_default(),
            sender: spec.sender.clone().unwrap_or_default(),
            kind: spec.kind.clone().unwrap_or_default(),
            tags: spec.tags.clone(),
            tag_input: String::new(),
            full_text: spec.full_text.clone().unwrap_or_default(),
        }
    }

    fn of(query: &SavedQuery) -> Self {
        Self {
            name: query.name.clone(),
            ..Self::of_spec(&query.spec)
        }
    }

    pub fn text(&self, field: QueryField) -> &str {
        match field {
            QueryField::Name => &self.name,
            QueryField::Year => &self.year,
            QueryField::Sender => &self.sender,
            QueryField::Kind => &self.kind,
            QueryField::Tags => &self.tag_input,
            QueryField::FullText => &self.full_text,
        }
    }

    fn text_mut(&mut self, field: QueryField) -> &mut String {
        match field {
            QueryField::Name => &mut self.name,
            QueryField::Year => &mut self.year,
            QueryField::Sender => &mut self.sender,
            QueryField::Kind => &mut self.kind,
            QueryField::Tags => &mut self.tag_input,
            QueryField::FullText => &mut self.full_text,
        }
    }

    /// The tags of the chips and those typed but not added yet, comma
    /// separated.
    fn tags_with_input(&self) -> BTreeSet<String> {
        let typed = self.tag_input.split(',').map(tag_name);
        self.tags
            .iter()
            .cloned()
            .chain(typed)
            .filter(|tag| !tag.is_empty())
            .collect()
    }

    /// What the form asks for; a year that is no year is a problem, never
    /// silently dropped.
    fn spec(&self) -> Result<QuerySpec, Problem> {
        let year = match self.year.trim() {
            "" => None,
            text => Some(
                text.parse::<i32>()
                    .ok()
                    .filter(|year| (1..=9999).contains(year))
                    .ok_or(Problem::InvalidYear)?,
            ),
        };
        let non_empty = |typed: &str| {
            let text = typed.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        Ok(QuerySpec {
            year,
            sender: non_empty(&self.sender),
            kind: non_empty(&self.kind),
            tags: self.tags_with_input(),
            full_text: non_empty(&self.full_text),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    InvalidYear,
    NameMissing,
    /// The work on the index or the files failed; the detail stays English.
    Failed(String),
}

/// What running a query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Results {
    pub hits: Vec<QueryHit>,
    /// The list of the hits is shown.
    pub expanded: bool,
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

/// Most queries of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct SavedQueries {
    rows: Vec<SavedQuery>,
    /// `None` is a new query, not saved yet.
    cursor: Option<usize>,
    window: ListWindow,
    /// The list is loading or the work is running; the view takes no new task.
    busy: bool,
    form: Form,
    problem: Option<Problem>,
    results: Option<Results>,
    /// Hits counted so far, by query, for the list.
    hit_counts: HashMap<SavedQueryId, usize>,
    confirming_delete: bool,
    export: Option<ExportStep>,
    exported: Option<ExportSummary>,
    /// The query to put the cursor on when the list is loaded next.
    select_after_load: Option<SavedQueryId>,
    /// A new report to start from once the list is there.
    prefilled: Option<Form>,
    /// The year the templates count back from.
    this_year: i32,
}

impl SavedQueries {
    /// Opens busy: the list loads with the view.
    pub fn new(this_year: i32) -> Self {
        Self {
            rows: Vec::new(),
            cursor: None,
            window: ListWindow::default(),
            busy: true,
            form: Form::default(),
            problem: None,
            results: None,
            hit_counts: HashMap::new(),
            confirming_delete: false,
            export: None,
            exported: None,
            select_after_load: None,
            prefilled: None,
            this_year,
        }
    }

    pub fn templates(&self) -> Vec<ReportTemplate> {
        templates(self.this_year)
    }

    /// Starts a new report from a template's form.
    pub fn use_template(&mut self, form: Form) {
        if self.busy {
            return;
        }
        self.cursor = None;
        self.form = form;
        self.leave_query();
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

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn form(&self) -> &Form {
        &self.form
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
    /// saved, else stays on the one it was on if it is still there.
    pub fn loaded(&mut self, rows: Vec<SavedQuery>) {
        self.load_rows(rows);
        if let Some(form) = self.prefilled.take() {
            self.cursor = None;
            self.form = form;
            self.leave_query();
        }
    }

    /// Opens a new report with `form` as soon as the list has loaded, so the
    /// list does not replace it.
    pub fn prefill(&mut self, form: Form) {
        self.prefilled = Some(form);
    }

    /// The new report the list is waiting to open.
    pub fn is_prefilled(&self) -> bool {
        self.prefilled.is_some()
    }

    fn load_rows(&mut self, rows: Vec<SavedQuery>) {
        let wanted = self
            .select_after_load
            .take()
            .or_else(|| self.selected().map(|query| query.id));
        let previous = self.cursor.is_some();
        self.rows = rows;
        self.busy = false;
        self.cursor = wanted.and_then(|id| self.rows.iter().position(|query| query.id == id));
        if let Some(cursor) = self.cursor {
            self.window
                .ensure_visible(cursor, self.rows.len(), VISIBLE_ROWS);
        }
        if let Some(query) = self.selected().cloned() {
            self.form = Form::of(&query);
        } else if previous {
            self.form = Form::default();
            self.results = None;
        }
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
        self.form = Form::of(query);
        self.cursor = Some(index);
        self.window
            .ensure_visible(index, self.rows.len(), VISIBLE_ROWS);
        self.leave_query();
    }

    /// The wheel moves the window; a query under the cursor follows it when
    /// the cursor would leave. A new query stays the one being edited.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        let mut moved = self.cursor.unwrap_or_default();
        self.window
            .scroll(delta, self.rows.len(), VISIBLE_ROWS, &mut moved);
        if self.cursor.is_some() {
            self.select(moved);
        }
    }

    /// An empty form for a query that is not saved yet.
    pub fn start_new(&mut self) {
        if self.busy {
            return;
        }
        self.cursor = None;
        self.form = Form::default();
        self.leave_query();
    }

    fn leave_query(&mut self) {
        self.problem = None;
        self.results = None;
        self.confirming_delete = false;
        self.export = None;
        self.exported = None;
    }

    /// Turns what is typed for tags into chips.
    pub fn add_tags(&mut self) {
        self.form.tags = self.form.tags_with_input();
        self.form.tag_input.clear();
        self.problem = None;
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.form.tags.remove(tag);
        self.problem = None;
    }

    pub fn set_text(&mut self, field: QueryField, text: String) {
        *self.form.text_mut(field) = text;
        self.problem = None;
    }

    /// The query as the form holds it, to be saved: new, or a change of the
    /// one under the cursor. A missing name or a bad year is reported and
    /// nothing is returned.
    pub fn begin_save(&mut self) -> Option<SaveRequest> {
        if self.busy {
            return None;
        }
        let spec = match self.form.spec() {
            Ok(spec) => spec,
            Err(problem) => {
                self.problem = Some(problem);
                return None;
            }
        };
        let name = self.form.name.trim().to_owned();
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
        self.form = Form::default();
        self.leave_query();
    }

    /// The form as a question to the index; a bad year is reported and
    /// nothing is returned. Works on a query that is not saved too.
    pub fn begin_run(&mut self) -> Option<QuerySpec> {
        if self.busy {
            return None;
        }
        match self.form.spec() {
            Ok(spec) => {
                self.busy = true;
                self.problem = None;
                self.results = None;
                self.export = None;
                self.exported = None;
                Some(spec)
            }
            Err(problem) => {
                self.problem = Some(problem);
                None
            }
        }
    }

    pub fn ran(&mut self, hits: Vec<QueryHit>) {
        self.busy = false;
        if let Some(id) = self.selected().map(|query| query.id) {
            self.hit_counts.insert(id, hits.len());
        }
        self.results = Some(Results {
            hits,
            expanded: false,
        });
    }

    pub fn toggle_hits(&mut self) {
        if let Some(results) = self.results.as_mut() {
            results.expanded = !results.expanded;
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
        }
    }

    pub fn loaded_list() -> SavedQueries {
        let mut list = SavedQueries::new(THIS_YEAR);
        list.loaded(stored_queries());
        list
    }

    fn ran_list() -> SavedQueries {
        let mut list = loaded_list();
        list.select(0);
        list.begin_run().unwrap();
        list.ran(vec![
            hit("a.pdf", false),
            hit("b.pdf", false),
            hit("c.pdf", true),
        ]);
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
    fn a_template_fills_the_form_with_exactly_its_criteria() {
        let invoices = ReportTemplate::InvoicesOfYear { year: 2026 };
        for lang in [Language::English, Language::German] {
            let spec = invoices.form(lang).spec().unwrap();
            assert_eq!(
                spec,
                QuerySpec {
                    year: Some(2026),
                    kind: Some("invoice".to_owned()),
                    ..QuerySpec::default()
                }
            );
            assert_eq!(invoices.form(lang).year, "2026");
        }

        let open = ReportTemplate::OpenInvoices
            .form(Language::German)
            .spec()
            .unwrap();
        assert_eq!(
            open,
            QuerySpec {
                kind: Some("invoice".to_owned()),
                tags: ["offen".to_owned()].into(),
                ..QuerySpec::default()
            }
        );
        assert_eq!(
            ReportTemplate::OpenInvoices.form(Language::English).name,
            "Open invoices"
        );
        assert_eq!(
            ReportTemplate::Contracts
                .form(Language::English)
                .spec()
                .unwrap()
                .kind,
            Some("contract".to_owned())
        );
    }

    #[test]
    fn using_a_template_starts_a_new_report_without_results() {
        let mut list = ran_list();
        assert!(list.cursor().is_some() && list.results().is_some());
        let form = ReportTemplate::Contracts.form(Language::English);

        list.use_template(form.clone());

        assert_eq!(list.cursor(), None);
        assert_eq!(list.form(), &form);
        assert!(list.results().is_none());
    }

    #[test]
    fn a_busy_page_ignores_a_template() {
        let mut list = SavedQueries::new(THIS_YEAR);

        list.use_template(ReportTemplate::Contracts.form(Language::English));

        assert_eq!(list.form(), &Form::default());
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
            expanded: false,
        };

        assert_eq!(
            results.totals(),
            vec![amount(200, "EUR"), amount(1_001, "USD")]
        );
        let none = Results {
            hits: vec![hit_with(None, false)],
            expanded: false,
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
            expanded: false,
        };

        assert_eq!(results.totals(), vec![amount(i64::MAX, "EUR")]);
    }

    #[test]
    fn the_page_opens_busy_with_a_new_query() {
        let list = SavedQueries::new(THIS_YEAR);

        assert!(list.is_busy());
        assert!(list.rows().is_empty());
        assert_eq!(list.cursor(), None);
    }

    #[test]
    fn selecting_a_query_fills_the_form_and_clears_what_was_found() {
        let mut list = ran_list();

        list.select(1);

        assert_eq!(list.form().name, "Taxes 2023");
        assert_eq!(list.form().year, "2023");
        assert!(list.results().is_none());
    }

    #[test]
    fn running_asks_for_the_form_as_typed_and_counts_the_hits_of_the_query() {
        let mut list = loaded_list();
        list.select(0);
        list.set_text(QueryField::Year, "2025".to_owned());
        list.set_text(QueryField::Tags, "Office, tax,".to_owned());
        list.add_tags();
        list.set_text(QueryField::Tags, "home office".to_owned());

        let spec = list.begin_run().unwrap();

        assert_eq!(spec.year, Some(2025));
        assert_eq!(
            spec.tags,
            BTreeSet::from([
                "home-office".to_owned(),
                "office".to_owned(),
                "tax".to_owned()
            ])
        );
        assert_eq!(list.form().tags.len(), 2);
        list.remove_tag("tax");
        assert_eq!(list.form().tags, BTreeSet::from(["office".to_owned()]));
        assert!(list.is_busy());
        list.ran(vec![hit("a.pdf", false)]);
        let id = list.selected().unwrap().id;
        assert_eq!(list.hit_count(id), Some(1));
        assert!(!list.is_busy());
    }

    #[test]
    fn a_year_that_is_no_year_is_a_problem_and_nothing_runs() {
        let mut list = loaded_list();
        for year in ["20x4", "0", "12345"] {
            list.set_text(QueryField::Year, year.to_owned());

            assert!(list.begin_run().is_none(), "{year}");
            assert_eq!(list.problem(), Some(&Problem::InvalidYear));
            assert!(!list.is_busy());
        }
        list.set_text(QueryField::Year, String::new());
        assert!(list.begin_run().is_some());
    }

    #[test]
    fn saving_needs_a_name_and_keeps_the_cursor_on_the_saved_query() {
        let mut list = loaded_list();
        list.start_new();
        assert!(list.begin_save().is_none());
        assert_eq!(list.problem(), Some(&Problem::NameMissing));

        list.set_text(QueryField::Name, "  Agency B  ".to_owned());
        let request = list.begin_save().unwrap();
        assert_eq!((request.id, request.name.as_str()), (None, "Agency B"));

        let mut stored = stored_queries();
        let id = stored[1].id;
        list.saved(id);
        stored.push(stored[0].clone());
        list.loaded(stored);
        assert_eq!(list.selected().unwrap().id, id);
        assert_eq!(list.form().name, "Taxes 2023");
    }

    #[test]
    fn saving_a_selected_query_changes_it() {
        let mut list = loaded_list();
        list.select(1);

        let request = list.begin_save().unwrap();

        assert_eq!(request.id, Some(list.selected().unwrap().id));
    }

    #[test]
    fn deleting_asks_first_and_only_the_second_step_deletes() {
        let mut list = loaded_list();
        list.select(0);
        assert!(list.begin_delete().is_none(), "no go without the question");

        list.ask_delete();
        assert!(list.is_confirming_delete());
        assert!(list.cancel_step());
        assert!(list.begin_delete().is_none());

        list.ask_delete();
        let id = list.begin_delete().unwrap();
        assert_eq!(id, list.selected().unwrap().id);
        list.deleted(id);
        assert_eq!(list.cursor(), None);
        assert_eq!(list.form(), &Form::default());
    }

    #[test]
    fn the_hits_are_summed_up_and_the_list_folds_open_and_shut() {
        let mut list = ran_list();

        let results = list.results().unwrap();
        assert_eq!((results.hits.len(), results.missing()), (3, 1));
        assert_eq!(list.copies(), 2);
        assert!(!list.results().unwrap().expanded);
        list.toggle_hits();
        assert!(list.results().unwrap().expanded);
    }

    #[test]
    fn the_export_asks_first_and_only_the_go_hands_over_the_files() {
        let mut list = ran_list();
        assert!(list.begin_export().is_none(), "no step, no export");

        list.start_export(PathBuf::from("target"));
        list.set_export_folder(Path::new("elsewhere"));
        assert_eq!(
            list.export_step().unwrap().folder,
            PathBuf::from("elsewhere")
        );
        assert!(list.cancel_step());
        assert!(list.begin_export().is_none());

        list.start_export(PathBuf::from("target"));
        let request = list.begin_export().unwrap();
        assert_eq!(request.folder, PathBuf::from("target"));
        assert_eq!(request.hits.len(), 3);
        assert!(list.is_busy());
    }

    #[test]
    fn nothing_to_copy_means_no_export_step() {
        let mut list = loaded_list();
        list.select(0);
        list.begin_run().unwrap();
        list.ran(vec![hit("gone.pdf", true)]);

        list.start_export(PathBuf::from("target"));

        assert!(list.export_step().is_none());
    }

    #[test]
    fn a_failure_ends_busy_and_is_shown() {
        let mut list = loaded_list();
        list.select(0);
        list.begin_run().unwrap();

        list.failed("the index is not open".to_owned());

        assert!(!list.is_busy());
        assert_eq!(
            list.problem(),
            Some(&Problem::Failed("the index is not open".to_owned()))
        );
    }
}
