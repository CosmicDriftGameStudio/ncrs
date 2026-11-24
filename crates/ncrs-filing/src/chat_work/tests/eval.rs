//! The hit rate of the trial run against known target folders. Two ignored
//! tests, run by hand: `brother_eval_read` in the `ncrs` crate reads an archive
//! and a source folder with the real local model and keeps the folder it
//! worked in; `brother_eval_trial` here plans the source into the archive from
//! that folder and compares with the expected folders.
//!
//! ```text
//! NCRS_EVAL_SCRATCH=<folder printed by brother_eval_read, target/eval/<stamp>>
//! NCRS_EVAL_EXPECTED=<tsv: file name, tab, folder or file below the archive;
//!                     several targets separated by " | ", one is enough>
//! NCRS_EVAL_REPORT=<markdown file to write> NCRS_PDFIUM_DIR=<pdfium>
//! cargo test --release -p ncrs-filing --features local-ai brother_eval_trial \
//!     -- --ignored --nocapture
//! ```
//!
//! `NCRS_EVAL_SOURCE` of the reading is the folder `run/downloads` of the kept
//! folder; the trial reads that copy, so neither the real archive nor the real
//! source is touched. The trial only plans: the test checks that the file
//! lists of both copies are the same before and after.

#![allow(
    clippy::print_stdout,
    clippy::too_many_lines,
    reason = "the test reports on stdout; one test walks one long scenario"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ncrs_index::MAX_FREE_KIND_CHARS;

use super::diagnosis::{access_over, read};
use super::*;
use crate::chat_work::change::segments;
use crate::chat_work::{TrialOutcome, TrialPlan};

type FileList = BTreeMap<PathBuf, u64>;

fn file_list(root: &Path) -> FileList {
    let mut found = FileList::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else {
                let size = entry.metadata().unwrap().len();
                found.insert(path.strip_prefix(root).unwrap().to_path_buf(), size);
            }
        }
    }
    found
}

/// Where a file is expected: every folder that counts as right.
fn expected_folders(target: &str) -> Vec<Vec<String>> {
    target
        .split(" | ")
        .map(|alternative| {
            let path = PathBuf::from(alternative.trim());
            let folder = if path.extension().is_some() {
                path.parent().map(Path::to_path_buf).unwrap_or_default()
            } else {
                path
            };
            segments(&folder)
        })
        .collect()
}

fn expected_by_name(tsv: &str) -> BTreeMap<String, Vec<Vec<String>>> {
    tsv.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, target)| (name.trim().to_owned(), expected_folders(target)))
        .collect()
}

fn is_year(segment: &str) -> bool {
    segment.len() == 4
        && segment.chars().all(|c| c.is_ascii_digit())
        && segment.starts_with(['1', '2'])
}

fn shown(folder: &[String]) -> String {
    folder.join("/")
}

struct Scan {
    name: String,
    fields: String,
    outcome: &'static str,
    neighbour: String,
    proposal: Option<Vec<String>>,
    candidates: Vec<Vec<String>>,
    proposed_name: String,
    expected: Option<Vec<Vec<String>>>,
}

fn scan_of(
    entry: &crate::chat_work::TrialEntry,
    expected: &BTreeMap<String, Vec<Vec<String>>>,
) -> Scan {
    let (outcome, proposal, candidates, proposed_name) = match &entry.outcome {
        TrialOutcome::Filed { to } => (
            "sicher",
            to.parent().map(segments),
            Vec::new(),
            to.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        TrialOutcome::Unsure { choices, .. } => (
            "unsicher",
            choices.first().map(|folder| segments(folder)),
            choices.iter().map(|folder| segments(folder)).collect(),
            String::new(),
        ),
        TrialOutcome::AlreadyThere { at } => (
            "schon da",
            Some(at.parent().map(segments).unwrap_or_default()),
            Vec::new(),
            String::new(),
        ),
        TrialOutcome::Stays(_) => ("bleibt", None, Vec::new(), String::new()),
    };
    let fields = &entry.fields;
    Scan {
        name: entry.name.clone(),
        fields: format!(
            "{} / {} / {}",
            fields.sender.as_deref().unwrap_or("-"),
            fields.kind.as_deref().unwrap_or("-"),
            fields
                .date
                .map_or_else(|| "-".to_owned(), |date| date.to_string())
        ),
        outcome,
        neighbour: entry
            .neighbour
            .as_ref()
            .map_or_else(String::new, |folder| folder.display().to_string()),
        proposal,
        candidates,
        proposed_name,
        expected: expected.get(&entry.name).cloned(),
    }
}

impl Scan {
    fn hit(&self) -> Option<bool> {
        let proposal = self.proposal.as_ref()?;
        Some(self.expected.as_ref()?.iter().any(|soll| soll == proposal))
    }
}

#[derive(Default)]
struct Tally {
    judged: usize,
    top1: usize,
    area: usize,
    with_year: usize,
    year: usize,
    in_candidates: usize,
    unsure: usize,
    sure: usize,
    sure_hits: usize,
    stays: usize,
}

fn tally_of(scans: &[Scan]) -> Tally {
    let mut tally = Tally::default();
    for scan in scans.iter().filter(|scan| scan.outcome != "schon da") {
        let Some(expected) = &scan.expected else {
            continue;
        };
        tally.judged += 1;
        let proposal = scan.proposal.as_deref().unwrap_or_default();
        tally.top1 += usize::from(scan.hit() == Some(true));
        tally.area += usize::from(
            expected
                .iter()
                .any(|soll| !soll.is_empty() && soll.first() == proposal.first()),
        );
        if expected
            .iter()
            .any(|soll| soll.iter().any(|part| is_year(part)))
        {
            tally.with_year += 1;
            tally.year += usize::from(expected.iter().any(|soll| {
                soll.iter()
                    .filter(|part| is_year(part))
                    .all(|year| proposal.contains(year))
            }));
        }
        match scan.outcome {
            "sicher" => {
                tally.sure += 1;
                tally.sure_hits += usize::from(scan.hit() == Some(true));
            }
            "unsicher" => {
                tally.unsure += 1;
                tally.in_candidates += usize::from(
                    scan.candidates
                        .iter()
                        .any(|candidate| expected.iter().any(|soll| soll == candidate)),
                );
            }
            _ => tally.stays += 1,
        }
    }
    tally
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        "n/a".to_owned()
    } else {
        format!(
            "{part}/{whole} = {:.1} %",
            part as f64 * 100.0 / whole as f64
        )
    }
}

fn report_of(
    plan: &TrialPlan,
    scans: &[Scan],
    kinds_outside: usize,
    kinds_long: usize,
    with_model: bool,
) -> String {
    let mut text = String::from(
        "| Scan | Sender / Art / Datum | Nachbar | Ergebnis | Modell | Vorschlag | Name | Soll | Treffer |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    let cell = |value: &str| value.replace('|', "/").replace('\n', " ");
    for scan in scans {
        let proposal = scan
            .proposal
            .as_ref()
            .map_or_else(|| "-".to_owned(), |folder| shown(folder));
        let soll = scan.expected.as_ref().map_or_else(
            || "?".to_owned(),
            |all| {
                all.iter()
                    .map(|folder| shown(folder))
                    .collect::<Vec<_>>()
                    .join(" ; ")
            },
        );
        let modelled = if with_model && scan.outcome == "unsicher" {
            scan.candidates
                .first()
                .map_or_else(String::new, |folder| shown(folder))
        } else {
            String::new()
        };
        let hit = match scan.hit() {
            Some(true) => "ja",
            Some(false) => "nein",
            None => "-",
        };
        writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {hit} |",
            cell(&scan.name),
            cell(&scan.fields),
            cell(&scan.neighbour),
            scan.outcome,
            cell(&modelled),
            cell(&proposal),
            cell(&scan.proposed_name),
            cell(&soll),
        )
        .unwrap();
    }
    let tally = tally_of(scans);
    let judged = tally.judged;
    writeln!(
        text,
        "\n## Summe ({judged} Scans mit Soll, ohne schon vorhandene)\n"
    )
    .unwrap();
    for (label, value) in [
        ("Top1-Ordner exakt", share(tally.top1, judged)),
        (
            "Bereich (erstes Segment) korrekt",
            share(tally.area, judged),
        ),
        (
            "Jahr korrekt (Soll mit Jahr)",
            share(tally.year, tally.with_year),
        ),
        (
            "Soll unter den Unsicher-Kandidaten",
            share(tally.in_candidates, tally.unsure),
        ),
        ("sicher", share(tally.sure, judged)),
        ("unsicher", share(tally.unsure, judged)),
        ("bleibt", share(tally.stays, judged)),
        (
            "Praezision der sicheren",
            share(tally.sure_hits, tally.sure),
        ),
        ("Arten ausserhalb des Katalogs", kinds_outside.to_string()),
        (
            "Arten mit mehr als 40 Zeichen",
            format!("{kinds_long} (Grenze {MAX_FREE_KIND_CHARS})"),
        ),
        ("schon im Archiv", plan.already_there.to_string()),
    ] {
        writeln!(text, "- {label}: {value}").unwrap();
    }
    text
}

/// The local model, when `NCRS_AI_MODELS_DIR` names the folder with it and the
/// build has it.
#[cfg(feature = "local-ai")]
fn local_model() -> Option<Arc<dyn ncrs_ai::ChatModel>> {
    let dir = std::env::var_os("NCRS_AI_MODELS_DIR").map(PathBuf::from)?;
    let path = dir.join(ncrs_ai::LOCAL_MODEL.name);
    let threads = std::num::NonZeroU32::new(4)?;
    let context = std::num::NonZeroU32::new(2048)?;
    Some(Arc::new(ncrs_ai::LazyLocalModel::new(
        path,
        ncrs_ai::LOCAL_MODEL,
        threads,
        context,
    )))
}

#[cfg(not(feature = "local-ai"))]
fn local_model() -> Option<Arc<dyn ncrs_ai::ChatModel>> {
    None
}

#[test]
#[ignore = "reads the folder brother_eval_read kept (NCRS_EVAL_SCRATCH); see the module doc"]
fn brother_eval_trial() {
    let (Some(kept), Some(expected_file), Some(report_file)) = (
        std::env::var_os("NCRS_EVAL_SCRATCH").map(PathBuf::from),
        std::env::var_os("NCRS_EVAL_EXPECTED").map(PathBuf::from),
        std::env::var_os("NCRS_EVAL_REPORT").map(PathBuf::from),
    ) else {
        println!("brother_eval_trial: NCRS_EVAL_SCRATCH, NCRS_EVAL_EXPECTED or NCRS_EVAL_REPORT is not set, nothing to do");
        return;
    };
    let archive_folder = kept.join("run/archive");
    let source_folder = kept.join("run/downloads");
    let archive_before = file_list(&archive_folder);
    let source_before = file_list(&source_folder);

    let work = tempfile::tempdir().unwrap();
    let model = local_model();
    let with_model = model.is_some();
    let access = access_over(&kept.join("index.db"), work.path()).with_model(model);
    let archive = read(&access, &archive_folder);
    let source = read(&access, &source_folder);
    // Each round asks a page of files; the picks it stores are not asked again.
    // A model that never finishes the files must fail the test, not hang it.
    const MAX_ROUNDS: usize = 200;
    let mut rounds = 0;
    let mut plan: TrialPlan;
    loop {
        plan = match access.run(ChatWork::Trial {
            source: Arc::new(source.clone()),
            target: Arc::new(archive.clone()),
            answered: AnsweredGroups::default(),
            layout: None,
            scope: crate::chat_work::ModelScope::FirstPage,
        }) {
            ChatResult::Trial(Ok(trial)) => *trial,
            other => panic!("{other:?}"),
        };
        rounds += 1;
        assert!(
            rounds <= MAX_ROUNDS,
            "the model left files after {MAX_ROUNDS} rounds"
        );
        if plan.model_left.is_empty() || plan.model_failure.is_some() {
            break;
        }
    }

    assert_eq!(
        file_list(&archive_folder),
        archive_before,
        "the archive changed"
    );
    assert_eq!(
        file_list(&source_folder),
        source_before,
        "the source changed"
    );

    let expected = expected_by_name(&fs::read_to_string(expected_file).unwrap());
    let mut scans: Vec<Scan> = plan
        .entries
        .iter()
        .map(|entry| scan_of(entry, &expected))
        .collect();
    scans.sort_by(|left, right| left.name.cmp(&right.name));

    let catalog = &source.kinds.catalog;
    let kinds: Vec<&str> = plan
        .entries
        .iter()
        .filter_map(|entry| entry.fields.kind.as_deref())
        .collect();
    let kinds_outside = kinds
        .iter()
        .filter(|kind| catalog.resolve(kind).is_none())
        .count();
    let kinds_long = kinds
        .iter()
        .filter(|kind| kind.chars().count() > MAX_FREE_KIND_CHARS)
        .count();

    let report = report_of(&plan, &scans, kinds_outside, kinds_long, with_model);
    fs::write(&report_file, &report).unwrap();
    println!("{report}");
    println!("report: {}", report_file.display());
}
