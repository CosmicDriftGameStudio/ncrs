//! A real archive and its plan, for the tests of the reorganisation and of the app.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use chrono::{NaiveDate, TimeZone as _, Utc};
use ncrs_index::{
    Condition, ConditionKind, DocumentFields, DocumentId, DocumentSource, FieldOrigin, FileStamp,
    NewDocument, RuleConditions, RuleEvidence, RuleSpec,
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

impl std::fmt::Debug for Archive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

pub fn rule_spec(phrase: &str, kind: &str, pattern: &str, confirmed: bool) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, phrase)]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some(kind.to_owned()),
        tags: Default::default(),
        filing_pattern: pattern.to_owned(),
        confirmed,
        enabled: true,
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
                hash: ncrs_intake::hash_path(&self.root.join(relative)).unwrap(),
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

pub fn day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
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
                subtree: None,
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
