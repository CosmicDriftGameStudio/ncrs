//! The work behind the reorganisation plan view. It blocks, so `App` runs it
//! on a blocking thread, never in `update`.
//!
//! Making a plan reads the index and the folder tree and stores the plan; the
//! files of the location are not touched. The index is the one the intake
//! shares, so it is held for as long as one location is planned.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{BatchKind, Finding, Index, IndexError, PlanItem, PlanOutcome, RuleId};
use ncrs_intake::{plan_location, FINDING_PRIORITY};

use crate::execute::{self, ExecutionItem, UndoReport};
use crate::fs::transfer::Progress;
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// One entry of a plan, with the location it belongs to and what the view
/// needs to know about the rule behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedItem {
    /// The archive location, as the intake sees it; `item` paths are below it.
    pub location: PathBuf,
    pub item: PlanItem,
    /// The finding that decides the action rests on a rule the user has not
    /// confirmed.
    pub unconfirmed_rule: bool,
}

/// The finding the outcome of `item` follows: the first of the priority.
pub fn deciding_finding(item: &PlanItem) -> Option<&Finding> {
    FINDING_PRIORITY
        .iter()
        .find_map(|&kind| item.findings.iter().find(|finding| finding.kind() == kind))
}

/// What the view may do with the index: plan the archive locations.
#[derive(Clone)]
pub struct PlanAccess {
    slot: IndexSlot,
    archive_roots: Vec<PathBuf>,
}

impl PlanAccess {
    pub fn new(slot: IndexSlot, archive_roots: Vec<PathBuf>) -> Self {
        Self {
            slot,
            archive_roots,
        }
    }

    /// Plans every archive location and returns all entries, location by
    /// location, each sorted by source.
    pub fn plan(&self) -> Result<Vec<PlannedItem>, String> {
        let mut planned = Vec::new();
        for root in &self.archive_roots {
            let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
            let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
            let plan = plan_location(index, root).map_err(|err| err.to_string())?;
            let confirmed: HashSet<RuleId> = index
                .rules()
                .map_err(|err| err.to_string())?
                .into_iter()
                .filter(|rule| rule.spec.confirmed)
                .map(|rule| rule.id)
                .collect();
            planned.extend(plan.items.into_iter().map(|item| PlannedItem {
                location: root.clone(),
                unconfirmed_rule: matches!(
                    deciding_finding(&item),
                    Some(Finding::Misplaced { rule }) if !confirmed.contains(rule)
                ),
                item,
            }));
        }
        Ok(planned)
    }
}

/// What a run of the plan did, over all locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub moved: usize,
    pub skipped: usize,
}

/// Counts over the whole selection, although the executor runs one location at
/// a time.
struct Overall<'a, P> {
    inner: &'a mut P,
    done_before: usize,
    total: usize,
}

impl<P: Progress> Progress for Overall<'_, P> {
    fn advanced(&mut self, done: usize, _total: usize) -> std::io::Result<()> {
        self.inner.advanced(self.done_before + done, self.total)
    }
}

/// The entries of the plan that move something, with the document the index
/// knows the file as.
fn execution_items(
    index: &Index,
    entries: &[&PlannedItem],
) -> Result<Vec<ExecutionItem>, IndexError> {
    let mut items = Vec::new();
    for entry in entries {
        let (PlanOutcome::Move { target } | PlanOutcome::Rename { target }) = &entry.item.outcome
        else {
            continue;
        };
        let from = entry.item.source.clone();
        let (document_id, duplicate) = match index.document_by_path(&from)? {
            Some(document) => (Some(document.id), false),
            None => match index.document_by_duplicate_path(&from)? {
                Some((id, _stamp)) => (Some(id), true),
                None => (None, false),
            },
        };
        items.push(ExecutionItem {
            from,
            to: target.clone(),
            expected_hash: entry.item.hash,
            document_id,
            duplicate,
        });
    }
    Ok(items)
}

impl PlanAccess {
    /// Carries out the selected entries, location by location, and marks the
    /// plans executed. The index stays locked for the whole run, which keeps
    /// the intake from looking at files while they move.
    pub fn execute<P: Progress>(
        &self,
        selection: &[PlannedItem],
        progress: &mut P,
    ) -> Result<ExecutionSummary, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let mut locations: Vec<&PathBuf> = Vec::new();
        for entry in selection {
            if !locations.contains(&&entry.location) {
                locations.push(&entry.location);
            }
        }
        let total = selection.len();
        let mut summary = ExecutionSummary {
            moved: 0,
            skipped: 0,
        };
        for location in locations {
            let entries: Vec<&PlannedItem> = selection
                .iter()
                .filter(|entry| &entry.location == location)
                .collect();
            let items = execution_items(index, &entries).map_err(|err| err.to_string())?;
            let mut overall = Overall {
                inner: progress,
                done_before: summary.moved + summary.skipped,
                total,
            };
            let report = execute::execute(
                index,
                BatchKind::Reorganisation,
                location,
                &items,
                &mut overall,
            )
            .map_err(|err| err.to_string())?;
            summary.moved += report.moved();
            summary.skipped += report.skipped();
            if let Some(plan) = index
                .reorganisation_plan(location)
                .map_err(|err| err.to_string())?
            {
                index
                    .mark_executed(plan.id)
                    .map_err(|err| err.to_string())?;
            }
        }
        Ok(summary)
    }

    /// Undoes the last carried out reorganisation that is not undone yet;
    /// `None` when there is none.
    pub fn undo_last(&self) -> Result<Option<UndoReport>, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let Some(batch) = index
            .last_undoable_batch(BatchKind::Reorganisation)
            .map_err(|err| err.to_string())?
        else {
            return Ok(None);
        };
        execute::undo(index, batch, &self.archive_roots)
            .map(Some)
            .map_err(|err| err.to_string())
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::SystemTime;

    use chrono::{NaiveDate, TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, ContentHash, DocumentFields, DocumentId, DocumentSource,
        FieldOrigin, FileStamp, NewDocument, PlanOutcome, RuleConditions, RuleEvidence, RuleSpec,
        TaxContext,
    };
    use tempfile::TempDir;

    use super::*;

    /// A real archive on disk with the index that knows it. Company A's
    /// insurance letters are filed by year; a few files break the order.
    pub struct Archive {
        pub _dir: TempDir,
        pub root: PathBuf,
        pub slot: IndexSlot,
        pub next_seed: u8,
    }

    fn rule_spec(phrase: &str, kind: &str, pattern: &str, confirmed: bool) -> RuleSpec {
        RuleSpec {
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                phrase,
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: kind.to_owned(),
            tax_context: TaxContext::None,
            tags: Default::default(),
            filing_pattern: pattern.to_owned(),
            confirmed,
        }
    }

    impl Archive {
        pub fn access(&self) -> PlanAccess {
            PlanAccess::new(Arc::clone(&self.slot), vec![self.root.clone()])
        }

        fn add(&mut self, relative: &str, date: Option<NaiveDate>, text: &str) -> DocumentId {
            self.next_seed += 1;
            let seed = self.next_seed;
            self.write(relative, &format!("content {seed}"));
            let mut guard = self.slot.lock().unwrap();
            let index = guard.as_mut().unwrap();
            let id = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: self.root.join(relative),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: Some(stamp()),
                })
                .unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        date,
                        ..DocumentFields::default()
                    },
                    |_| FieldOrigin::Unknown,
                )
                .unwrap();
            index.set_document_text(id, text).unwrap();
            id
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn add_copy(&self, of: DocumentId, relative: &str, content_of: &str) {
            let original = fs::read_to_string(self.root.join(content_of)).unwrap();
            self.write(relative, &original);
            let mut guard = self.slot.lock().unwrap();
            guard
                .as_mut()
                .unwrap()
                .record_duplicate_path(of, &self.root.join(relative), stamp())
                .unwrap();
        }

        /// Every file below the archive with its content.
        pub fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
            fn collect(root: &Path, folder: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
                for entry in fs::read_dir(folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        collect(root, &path, found);
                    } else {
                        found.insert(
                            path.strip_prefix(root).unwrap().to_path_buf(),
                            fs::read(&path).unwrap(),
                        );
                    }
                }
            }
            let mut found = BTreeMap::new();
            collect(&self.root, &self.root, &mut found);
            found
        }
    }

    fn stamp() -> FileStamp {
        FileStamp {
            size: 1,
            mtime: SystemTime::now(),
        }
    }

    fn day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, month, day)
    }

    /// One entry of every kind: a copy, a file a confirmed rule and one a
    /// suggested rule file elsewhere, a wrong year, an odd name, a target that
    /// exists already, and a file too little is known about.
    pub fn archive() -> Archive {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("Documents");
        fs::create_dir(&root).unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        index
            .create_rule(&rule_spec(
                "tax notice",
                "Tax",
                "Taxes/{year}/{date}_{kind}.pdf",
                true,
            ))
            .unwrap();
        index
            .suggest_rule(
                &rule_spec("late fee", "Fees", "Fees/{year}/{date}_{kind}.pdf", false),
                RuleEvidence {
                    matching_in_folder: 3,
                    folder_documents: 3,
                    matching_elsewhere: 0,
                },
            )
            .unwrap();
        let mut archive = Archive {
            _dir: dir,
            root,
            slot: Arc::new(Mutex::new(Some(index))),
            next_seed: 0,
        };
        let letter = "Company A policy letter";
        for (year, month) in [(2023, 2), (2023, 3), (2024, 1), (2024, 2), (2024, 3)] {
            archive.add(
                &format!("Insurance/{year}/{year}-{month:02}-01_invoice.pdf"),
                day(year, month, 1),
                letter,
            );
        }
        archive.add(
            "Insurance/2024/2024-06-01_invoice.pdf",
            day(2024, 6, 1),
            letter,
        );
        let original = archive.add(
            "Insurance/2024/2024-04-01_invoice.pdf",
            day(2024, 4, 1),
            letter,
        );
        archive.add_copy(
            original,
            "Misc/2024-04-01_invoice.pdf",
            "Insurance/2024/2024-04-01_invoice.pdf",
        );
        archive.add(
            "Insurance/2024/2024-07-01_invoice.pdf",
            day(2024, 7, 1),
            "Agency B Tax Notice",
        );
        archive.add(
            "Insurance/2024/2024-08-01_invoice.pdf",
            day(2024, 8, 1),
            "Company A late fee",
        );
        archive.add(
            "Insurance/2023/2024-05-01_invoice.pdf",
            day(2024, 5, 1),
            letter,
        );
        archive.add("Insurance/2024/scan 0042.pdf", day(2024, 4, 5), letter);
        archive.add(
            "Insurance/2023/2024-06-01_invoice.pdf",
            day(2024, 6, 1),
            letter,
        );
        archive.add("Insurance/2024/IMG_7.pdf", None, letter);
        archive
    }

    /// The plan the real planner makes for `archive()`.
    pub fn sample_items() -> Vec<PlannedItem> {
        archive().access().plan().unwrap()
    }

    pub fn item<'a>(items: &'a [PlannedItem], source: &str) -> &'a PlannedItem {
        items
            .iter()
            .find(|planned| planned.item.source == Path::new(source))
            .unwrap_or_else(|| panic!("no entry for {source}: {items:?}"))
    }

    #[test]
    fn the_plan_has_an_entry_of_every_kind_and_a_target_for_the_ones_that_can_run() {
        let items = sample_items();

        assert!(matches!(
            item(&items, "Misc/2024-04-01_invoice.pdf").item.outcome,
            PlanOutcome::Move { .. }
        ));
        assert!(matches!(
            item(&items, "Insurance/2023/2024-05-01_invoice.pdf")
                .item
                .outcome,
            PlanOutcome::Move { .. }
        ));
        assert!(matches!(
            item(&items, "Insurance/2024/scan 0042.pdf").item.outcome,
            PlanOutcome::Rename { .. }
        ));
        assert!(matches!(
            item(&items, "Insurance/2023/2024-06-01_invoice.pdf")
                .item
                .outcome,
            PlanOutcome::Conflict { .. }
        ));
        assert!(matches!(
            item(&items, "Insurance/2024/IMG_7.pdf").item.outcome,
            PlanOutcome::NoAction(_)
        ));
    }

    #[test]
    fn only_a_rule_the_user_has_not_confirmed_is_marked() {
        let items = sample_items();

        assert!(!item(&items, "Insurance/2024/2024-07-01_invoice.pdf").unconfirmed_rule);
        assert!(item(&items, "Insurance/2024/2024-08-01_invoice.pdf").unconfirmed_rule);
        assert!(!item(&items, "Misc/2024-04-01_invoice.pdf").unconfirmed_rule);
    }

    #[test]
    fn making_the_plan_changes_no_file() {
        let archive = archive();
        let before = archive.snapshot();

        let items = archive.access().plan().unwrap();

        assert!(!items.is_empty());
        assert_eq!(archive.snapshot(), before);
    }

    #[test]
    fn a_closed_index_is_reported() {
        let access = PlanAccess::new(Arc::new(Mutex::new(None)), vec![PathBuf::from("/x")]);

        assert_eq!(access.plan(), Err(INDEX_NOT_OPEN.to_owned()));
    }

    #[test]
    fn executing_and_undoing_with_a_closed_index_is_reported() {
        let access = PlanAccess::new(Arc::new(Mutex::new(None)), vec![PathBuf::from("/x")]);

        assert_eq!(
            access.execute(&[], &mut crate::fs::transfer::Counting),
            Err(INDEX_NOT_OPEN.to_owned())
        );
        assert_eq!(access.undo_last(), Err(INDEX_NOT_OPEN.to_owned()));
    }
}
