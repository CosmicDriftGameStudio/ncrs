use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use ncrs_config::config::TargetSetting;
use ncrs_fs::transfer::Counting;
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_index::{
    Condition, ConditionKind, Document, DocumentFields, FieldOrigin, Index, RuleConditions, RuleId,
    RuleOrigin, RuleSpec, TargetId,
};
use ncrs_intake::{
    canonical_path, LocationTransfer, OnNoText, OnSeveralTargets, PlainText, SortSource,
    SortTarget, TextChain,
};
use ncrs_pipeline::{Load, OwnDetails};
use tempfile::TempDir;

use super::watched::Volume;
use super::*;
use crate::reading_support::ReadProgress;

mod filing;
mod planning;
mod suggest;
mod watching;

struct Setup {
    root: PathBuf,
    access: SortingAccess,
    _dir: TempDir,
}

fn id(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}

impl Setup {
    /// Targets `t1` and `t2` in folders `T1` and `T2`, both on the system disk.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical_path(dir.path()).unwrap();
        let index = Index::create_plain(&root.join("index.db")).unwrap();
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let mut setup = Self {
            access: SortingAccess {
                slot: Arc::new(Mutex::new(Some(index))),
                sources: Vec::new(),
                targets: Vec::new(),
                text_chain: Arc::new(TextChain::new().with_source(PlainText)),
                own: OwnDetails::default(),
                load: Load::default(),
                trash: Arc::new(JournaledTrash::new(
                    root.join("trash.toml"),
                    FolderBackend(bin),
                )),
                snapshots: Default::default(),
                index_work: Default::default(),
            },
            root,
            _dir: dir,
        };
        setup.add_target("t1", "T1", Some(false));
        setup.add_target("t2", "T2", Some(false));
        setup
    }

    fn add_target(&mut self, name: &str, folder: &str, own_volume: Option<bool>) {
        let path = self.path(folder);
        fs::create_dir_all(&path).unwrap();
        self.access.targets.push(TargetSetting {
            target: SortTarget {
                id: id(name),
                name: name.to_owned(),
                path,
                own_volume,
            },
            paused: None,
        });
    }

    /// Joined part by part: a `/` kept inside a Windows path would fall
    /// outside the index's subtree range, which separates with `\`.
    fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// A file nobody has written to for a while.
    fn write(&self, relative: &str, content: &str) -> PathBuf {
        self.write_at(relative, content, 600)
    }

    fn write_at(&self, relative: &str, content: &str, seconds_ago: u64) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
            .unwrap();
        path
    }

    fn source(&self, folder: &str, targets: &[&str]) -> SortSource {
        let path = self.path(folder);
        fs::create_dir_all(&path).unwrap();
        SortSource {
            path,
            include: String::new(),
            exclude: String::new(),
            targets: targets.iter().map(|name| id(name)).collect(),
            watch: true,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: None,
        }
    }

    /// Files whatever mentions `word` by the number after "Ref".
    fn rule(&self, target: &str, word: &str, pattern: &str) -> RuleId {
        self.rule_on(target, ConditionKind::Substring, word, pattern)
    }

    fn rule_on(&self, target: &str, kind: ConditionKind, value: &str, pattern: &str) -> RuleId {
        self.index_mut(|index| {
            index
                .create_rule_in(
                    &rule_spec(kind, value, pattern),
                    RuleOrigin::default(),
                    &id(target),
                )
                .unwrap()
        })
    }

    fn index<T>(&self, work: impl FnOnce(&Index) -> T) -> T {
        self.access.with_index(|index| Ok(work(index))).unwrap()
    }

    fn index_mut<T>(&self, work: impl FnOnce(&mut Index) -> T) -> T {
        self.access.with_index_mut(|index| Ok(work(index))).unwrap()
    }

    fn document(&self, path: &Path) -> Document {
        self.index(|index| index.document_by_path(path).unwrap().unwrap())
    }

    fn set_fields(&self, path: &Path, fields: &DocumentFields, origin: FieldOrigin) {
        let document = self.document(path).id;
        self.index_mut(|index| {
            index
                .set_document_fields(document, fields, |_| origin.clone())
                .unwrap();
        });
    }

    fn plan(&self, source: &SortSource) -> SortPlan {
        self.plan_with(source, ReadUnknown::Yes)
    }

    fn plan_with(&self, source: &SortSource, read: ReadUnknown) -> SortPlan {
        let request = PlanRequest {
            source: source.clone(),
            files: PlanFiles::All,
            read,
        };
        plan(&self.access, &request, &ReadProgress::default(), None).unwrap()
    }

    /// Everything the plan has a place for.
    fn run(&self, source: &SortSource) -> Result<SortReport, RunError> {
        let plan = self.plan(source);
        run(&self.access, &request_of(source, &plan), &mut Counting)
    }

    fn watch(&self, source: &SortSource) -> WatchedReport {
        run_watched(
            &self.access,
            source,
            &WatchedFiles::All,
            None,
            "watched",
            &mut Counting,
        )
        .unwrap()
    }

    /// `watch` of a source whose drive is there or not.
    fn watch_drive(&self, source: &SortSource, is_mounted: impl Fn() -> bool) -> WatchedReport {
        self.watch_volume(
            source,
            Volume {
                is_mounted: |_: &Path| is_mounted(),
                is_own: super::ready::is_on_own_volume,
            },
        )
    }

    fn watch_volume(
        &self,
        source: &SortSource,
        volume: Volume<impl Fn(&Path) -> bool, impl Fn(&Path) -> Option<bool>>,
    ) -> WatchedReport {
        super::watched::watch_with(
            &self.access,
            source,
            &WatchedFiles::All,
            None,
            "watched",
            &mut Counting,
            volume,
        )
        .unwrap()
    }

    /// The first look at the source, then what the pipeline does with the
    /// files written since.
    fn watch_after_reading(&self, source: &SortSource) -> WatchedReport {
        self.plan(source);
        self.watch(source)
    }

    fn runs(&self) -> usize {
        self.access.recent_runs(100).unwrap().len()
    }
}

fn rule_spec(kind: ConditionKind, value: &str, pattern: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(kind, value)]),
        extract_number: Some(r"Ref (\w+)".to_owned()),
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

fn request_of(source: &SortSource, plan: &SortPlan) -> RunRequest {
    let entries = plan
        .rows
        .iter()
        .filter_map(|row| {
            let places = match &row.state {
                RowState::Place(place) => std::slice::from_ref(place),
                RowState::Several(places) => places.as_slice(),
                _ => return None,
            };
            Some(RunEntry {
                path: row.path.clone(),
                hash: row.hash?,
                document: row.document,
                places: places
                    .iter()
                    .map(|place| (place.target.clone(), place.relative.clone()))
                    .collect(),
            })
        })
        .collect();
    RunRequest {
        name: "sorted".to_owned(),
        source: source.clone(),
        entries,
        transfer: source.transfer,
    }
}

fn state_of<'a>(plan: &'a SortPlan, name: &str) -> &'a RowState {
    let row = plan.rows.iter().find(|row| row.relative.ends_with(name));
    &row.unwrap_or_else(|| panic!("no row for {name} in {plan:?}"))
        .state
}

fn place_of<'a>(plan: &'a SortPlan, name: &str) -> &'a RowPlace {
    match state_of(plan, name) {
        RowState::Place(place) => place,
        other => panic!("expected a place, got {other:?}"),
    }
}

fn filed(report: &WatchedReport) -> usize {
    match report {
        WatchedReport::Done { run, .. } => run.done(),
        other => panic!("expected a run, got {other:?}"),
    }
}

fn asked(report: &WatchedReport) -> usize {
    match report {
        WatchedReport::Done { asked, .. } => *asked,
        other => panic!("expected a run, got {other:?}"),
    }
}
