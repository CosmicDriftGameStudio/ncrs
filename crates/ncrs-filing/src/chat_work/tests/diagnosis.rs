//! Numbers on a real archive, printed to stdout. Reads the scratch folder a
//! protocol run left behind (`NCRS_PROTOCOL_KEEP=1`), through a copy of its
//! plain index, so the fields are the ones the app indexed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::too_many_lines,
    clippy::use_debug
)]

use std::collections::BTreeMap;

use ncrs_index::InMemoryKeyStore;
use ncrs_intake::{NeighbourIndex, OwnNames, PdfText};

use super::*;
use crate::chat_work::change::segments;
use crate::chat_work::{TrialOutcome, TrialPlan};

fn scratch_access(scratch: &Path, work: &Path) -> ChatAccess {
    let copy = work.join("index.db");
    let index = if std::env::var_os("NCRS_DIAGNOSIS_COLD").is_some() {
        Index::create_plain(&copy).unwrap()
    } else {
        fs::copy(scratch.join("data/index.db"), &copy).unwrap();
        Index::open(&copy, &InMemoryKeyStore::empty()).unwrap()
    };
    let bin = work.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let trash = JournaledTrash::new(work.join("trash.toml"), FolderBackend(bin));
    let mut chain = TextChain::new();
    chain = chain.with_source(PdfText::load_shipped().expect("pdfium is not found"));
    ChatAccess::new(
        Arc::new(Mutex::new(Some(index))),
        Arc::new(chain),
        Arc::new(trash),
    )
}

fn read(access: &ChatAccess, folder: &Path) -> FolderReading {
    let started = std::time::Instant::now();
    let reading = match access.run(ChatWork::Read {
        folder: folder.to_path_buf(),
        progress: Arc::default(),
    }) {
        ChatResult::Read(Ok(reading)) => reading,
        other => panic!("{other:?}"),
    };
    println!(
        "read {}: {} files in {:.2}s",
        folder.display(),
        reading.files.len(),
        started.elapsed().as_secs_f64()
    );
    Arc::unwrap_or_clone(reading)
}

#[derive(Default)]
struct Counts {
    files: usize,
    sender: usize,
    kind: usize,
    date: usize,
    text: usize,
}

fn counts_of<'a>(files: impl Iterator<Item = &'a PlanFile>) -> Counts {
    let mut counts = Counts::default();
    for file in files {
        counts.files += 1;
        counts.sender += usize::from(file.fields.sender.is_some());
        counts.kind += usize::from(file.fields.kind.is_some());
        counts.date += usize::from(file.fields.date.is_some());
        counts.text += usize::from(file.text.is_some());
    }
    counts
}

fn print_counts(label: &str, counts: &Counts) {
    println!(
        "  {label:<28} files {:>5}  sender {:>5}  kind {:>5}  date {:>5}  text {:>5}",
        counts.files, counts.sender, counts.kind, counts.date, counts.text
    );
}

fn fields_report(archive: &FolderReading) {
    println!("== D1.1 fields in the archive");
    print_counts("all", &counts_of(archive.files.iter()));
    let mut by_top: BTreeMap<String, Vec<&PlanFile>> = BTreeMap::new();
    for file in &archive.files {
        let top = segments(&file.relative_path)
            .into_iter()
            .next()
            .unwrap_or_default();
        by_top.entry(top).or_default().push(file);
    }
    for (top, files) in &by_top {
        print_counts(top, &counts_of(files.iter().copied()));
    }
    let mut senders: BTreeMap<&str, usize> = BTreeMap::new();
    for sender in archive
        .files
        .iter()
        .filter_map(|file| file.fields.sender.as_deref())
    {
        *senders.entry(sender).or_default() += 1;
    }
    let mut ranked: Vec<_> = senders.into_iter().collect();
    ranked.sort_by_key(|(name, count)| (std::cmp::Reverse(*count), *name));
    println!("  top senders: {:?}", &ranked[..ranked.len().min(10)]);
}

fn trial_report(access: &ChatAccess, downloads: &FolderReading, archive: &FolderReading) {
    println!("== D1.2 the downloads against the archive");
    let plan: TrialPlan = match access.run(ChatWork::Trial {
        source: Arc::new(downloads.clone()),
        target: Arc::new(archive.clone()),
        answered: AnsweredGroups::default(),
        layout: None,
    }) {
        ChatResult::Trial(Ok(plan)) => plan,
        other => panic!("{other:?}"),
    };
    let own = OwnNames {
        names: &[],
        ibans: &[],
        tax_ids: &[],
    };
    let neighbours = NeighbourIndex::new(&archive.files, &own, &archive.structure);
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &plan.entries {
        let in_archive = archive
            .files
            .iter()
            .find(|file| file.hash == entry.hash)
            .map(|file| file.relative_path.display().to_string());
        let file = downloads
            .files
            .iter()
            .find(|file| file.hash == entry.hash)
            .unwrap();
        let placed = neighbours.place(&entry.fields, file.text.as_deref());
        let verdict = match &entry.outcome {
            TrialOutcome::Stays(reason) => format!("Stays({reason:?})"),
            TrialOutcome::Filed { .. } => "Filed".to_owned(),
            TrialOutcome::Unsure { .. } => "Unsure".to_owned(),
            TrialOutcome::AlreadyThere { .. } => "AlreadyThere".to_owned(),
        };
        *tally.entry(verdict.clone()).or_default() += 1;
        println!(
            "  {} | was {:?} | sender {:?} kind {:?} date {:?} | {verdict} | place {}",
            entry.name,
            in_archive,
            entry.fields.sender,
            entry.fields.kind,
            entry.fields.date,
            placed.map_or_else(
                || "None".to_owned(),
                |found| format!(
                    "{:?} folder {} agreeing {} similar {} pattern {:?} missing {:?}",
                    found.signal,
                    found.folder.display(),
                    found.agreeing,
                    found.similar,
                    found.pattern,
                    found.missing
                )
            ),
        );
    }
    println!("  tally: {tally:?}");
}

#[test]
#[ignore = "reads NCRS_DIAGNOSIS_SCRATCH, the scratch folder of a protocol run"]
fn diagnosis() {
    let Some(scratch) = std::env::var_os("NCRS_DIAGNOSIS_SCRATCH").map(PathBuf::from) else {
        println!("diagnosis: NCRS_DIAGNOSIS_SCRATCH is not set, nothing to do");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let access = scratch_access(&scratch, work.path());
    let archive = read(&access, &scratch.join("archive"));
    let downloads = read(&access, &scratch.join("downloads"));
    println!(
        "areas: {} (unordered: {:?})",
        archive.structure.areas.len(),
        archive.structure.unordered
    );
    for area in &archive.structure.areas {
        println!("  area {} branches {:#?}", area.folder, area.branches);
    }
    fields_report(&archive);
    trial_report(&access, &downloads, &archive);
}

fn pdfs_below(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut listed: Vec<_> = entries.flatten().collect();
    listed.sort_by_key(fs::DirEntry::path);
    for entry in listed {
        let path = entry.path();
        if path.is_dir() {
            pdfs_below(&path, found);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            found.push(path);
        }
    }
}

/// Every `step`-th PDF below `folder`, `count` of them: a spread over the years.
fn spread_of_pdfs(folder: &Path, count: usize) -> Vec<PathBuf> {
    let mut all = Vec::new();
    pdfs_below(folder, &mut all);
    let step = all.len().div_euclid(count).max(1);
    all.into_iter().step_by(step).take(count).collect()
}

#[cfg(feature = "local-ai")]
#[test]
#[ignore = "reads NCRS_PROTOCOL_ARCHIVE and runs the model from NCRS_AI_MODELS_DIR"]
fn ai_fields_on_archive_pdfs() {
    use ncrs_ai::{ExtractionRequest, Provider as _, CONFIDENT, LOCAL_MODEL};
    use ncrs_index::{Field, KindCatalog};

    let archive = PathBuf::from(std::env::var_os("NCRS_PROTOCOL_ARCHIVE").unwrap());
    let models = PathBuf::from(std::env::var_os("NCRS_AI_MODELS_DIR").unwrap());
    let model = ncrs_ai::LazyLocalModel::shared(
        models.join(LOCAL_MODEL.name),
        LOCAL_MODEL,
        std::num::NonZeroU32::new(4).unwrap(),
        std::num::NonZeroU32::new(4096).unwrap(),
    );
    let mut chain = TextChain::new();
    chain = chain.with_source(PdfText::load_shipped().expect("pdfium is not found"));
    let mut files = spread_of_pdfs(&archive.join("Bank"), 10);
    files.extend(spread_of_pdfs(&archive.join("Versicherung"), 10));
    let kinds = Arc::new(KindCatalog::built_in());
    let (mut with_text, mut sender_hit, mut kind_hit) = (0, 0, 0);
    let mut total_secs: f64 = 0.0;
    let mut asked = 0;
    for (position, path) in files.iter().enumerate() {
        let Some(text) = chain.extract(path).ok().flatten() else {
            println!("doc {position}: no text {}", path.display());
            continue;
        };
        with_text += 1;
        let started = std::time::Instant::now();
        let answer = model.extract(&ExtractionRequest {
            text,
            open_fields: vec![Field::Sender, Field::Kind, Field::Date],
            known: DocumentFields::default(),
            source: archive.clone(),
            kinds: Arc::clone(&kinds),
        });
        let secs = started.elapsed().as_secs_f64();
        asked += 1;
        total_secs += secs;
        let confident = |field: Field| {
            let extraction = answer.as_ref().ok()?;
            extraction
                .fields
                .iter()
                .find(|found| found.field == field && found.confidence >= CONFIDENT)
                .map(|found| found.value.clone())
        };
        let (sender, kind) = (confident(Field::Sender), confident(Field::Kind));
        sender_hit += usize::from(sender.is_some());
        kind_hit += usize::from(kind.is_some());
        println!("doc {position}: {secs:.1}s sender {sender:?} kind {kind:?}");
    }
    println!(
        "p0: documents {} with text {with_text} asked {asked} mean {:.1}s sender {sender_hit} kind {kind_hit}",
        files.len(),
        total_secs / f64::from(u32::try_from(asked.max(1)).unwrap())
    );
}
