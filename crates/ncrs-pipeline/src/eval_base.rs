//! The test base of the filing eval: the folder `brother_eval_read` resumes
//! from, with an index that already knows what a real index read from the same
//! files. `eval_seed` in `ncrs-index` is the step between the two here, and the
//! only one that needs a key.
//!
//! ```text
//! NCRS_EVAL_ARCHIVE=<bed archive> NCRS_EVAL_SOURCE=<bed source> NCRS_SEED_HASHES=<file to write>
//! cargo test -p ncrs-pipeline --lib -- --ignored --exact eval_base::seed_hash_list --nocapture
//!
//! NCRS_SEED_RAW=<output of eval_seed> NCRS_EVAL_ARCHIVE=… NCRS_EVAL_SOURCE=…
//! NCRS_EVAL_CONTINUOUS=<folder with new files> NCRS_SEED_KEPT=<new folder>
//! cargo test -p ncrs-pipeline --lib -- --ignored --exact eval_base::seed_base --nocapture
//! ```
//!
//! A document gets the state a fresh reading leaves it in, at the path of the
//! run: the stored text is kept, the fields the AI read from it are kept, the
//! detection of the look of a letter runs again without learned aliases. Values
//! from rules stay in the archive, as the app has them there. A document with a
//! value from a person or of unknown origin, with an older reading, or a file of
//! the source with a value from a rule is asked again from its stored text.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use ncrs_ai::{FakeProvider, Provider};
use ncrs_index::{
    ContentHash, Document, DocumentFields, DocumentSource, DocumentStatus, Field, FieldOrigin,
    FileStamp, InMemoryKeyStore, Index, NewDocument, SenderAliases,
};
use ncrs_intake::{
    canonical_path, detect_fields, hash_path, Location, LocationMode, OwnNames, TextChain,
    FIELDS_READ_VERSION, STABLE_AFTER,
};
use tempfile::TempDir;

use crate::ai_step::{clear_field, AI_READ_VERSION};
use crate::{intake_steps, IndexAccess, Item, OwnDetails, StepOutcome};

const MASTER: &str = "master";

#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    files: usize,
    adopted: usize,
    /// Of the adopted ones: asked again in the run, for which reason.
    asked_again_older_reading: usize,
    asked_again_not_from_the_text: usize,
    asked_again_detection_differs: usize,
    /// Left to the run.
    not_in_the_index: usize,
    without_text: usize,
    same_content_as_another: usize,
    also_a_new_file: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Built {
    archive: Counts,
    source: Counts,
    new_files: usize,
}

struct Bed<'a> {
    archive: &'a Path,
    source: &'a Path,
    new_files: Option<&'a Path>,
}

/// The files the run copies, relative to `root`: no dot entries, no links.
fn files_below(root: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder)? {
            let file = entry?;
            let kind = file.file_type()?;
            if file.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if kind.is_dir() {
                pending.push(file.path());
            } else if !kind.is_symlink() {
                let path = file.path();
                found.insert(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            }
        }
    }
    Ok(found)
}

fn hashes_below(root: &Path) -> HashSet<ContentHash> {
    files_below(root)
        .unwrap()
        .iter()
        .map(|file| hash_path(&root.join(file)).unwrap())
        .collect()
}

fn hash_list(roots: &[&Path]) -> String {
    let lines: BTreeSet<String> = roots
        .iter()
        .flat_map(|root| hashes_below(root))
        .map(|hash| hash.0.iter().map(|byte| format!("{byte:02x}")).collect())
        .collect();
    lines.into_iter().map(|line| line + "\n").collect()
}

fn stamp_of(file: &Path) -> FileStamp {
    let metadata = fs::metadata(file).unwrap();
    FileStamp {
        size: metadata.len(),
        mtime: metadata.modified().unwrap(),
    }
}

struct Base<'a> {
    raw: &'a Index,
    index: Index,
    new_hashes: HashSet<ContentHash>,
}

impl Base<'_> {
    fn adopt(&mut self, file: &Path, at: PathBuf, mode: LocationMode, counts: &mut Counts) {
        counts.files += 1;
        let hash = hash_path(file).unwrap();
        if self.new_hashes.contains(&hash) {
            counts.also_a_new_file += 1;
            return;
        }
        if self.index.document_by_hash(&hash).unwrap().is_some() {
            counts.same_content_as_another += 1;
            return;
        }
        let Some(known) = self.raw.document_by_hash(&hash).unwrap() else {
            counts.not_in_the_index += 1;
            return;
        };
        // An empty text may be one that could not be read then.
        let Some(text) = self
            .raw
            .document_text(known.id)
            .unwrap()
            .filter(|text| !text.trim().is_empty())
        else {
            counts.without_text += 1;
            return;
        };
        counts.adopted += 1;
        let stamp = stamp_of(file);
        let (source, received_at) = match mode {
            LocationMode::Archive => (DocumentSource::Archive, stamp.mtime),
            LocationMode::Inbox => (DocumentSource::Inbox, SystemTime::now()),
        };
        let id = self
            .index
            .insert_document(&NewDocument {
                hash,
                path: at,
                source,
                received_at: DateTime::<Utc>::from(received_at),
                stamp: Some(stamp),
            })
            .unwrap();
        self.index.set_document_text(id, &text).unwrap();
        let (fields, ai_version) = self.fresh_fields(&known, &text, mode, counts);
        let origins = self.raw.field_origins(known.id).unwrap();
        self.index
            .set_document_fields(id, &fields, |field| match origins.of(field) {
                ai @ FieldOrigin::Ai { .. } if ai_version == AI_READ_VERSION => ai,
                _ => match field {
                    Field::Recipient => FieldOrigin::AddressBlock,
                    Field::Kind => FieldOrigin::Heading,
                    _ => FieldOrigin::Detected,
                },
            })
            .unwrap();
        self.index.set_ai_read_version(id, ai_version).unwrap();
        self.index
            .set_fields_read_version(id, FIELDS_READ_VERSION)
            .unwrap();
        if mode == LocationMode::Archive {
            self.index
                .set_document_status(id, DocumentStatus::Filed)
                .unwrap();
        }
    }

    /// The fields a fresh reading of the text has, and the version of the AI
    /// reading they are from: 0 when the AI has to be asked again.
    fn fresh_fields(
        &self,
        known: &Document,
        text: &str,
        mode: LocationMode,
        counts: &mut Counts,
    ) -> (DocumentFields, u32) {
        let detected = detect_fields(
            text,
            &DocumentFields::default(),
            &SenderAliases::default(),
            &OwnNames {
                names: &[],
                ibans: &[],
                tax_ids: &[],
            },
            self.index.kinds(),
        );
        let origins = self.raw.field_origins(known.id).unwrap();
        let mut stored = known.fields.clone();
        stored.tags.clear();
        let mut from_ai = stored.clone();
        let mut not_from_the_text = false;
        for field in Field::ALL {
            let origin = origins.of(field);
            // What rules filled in the archive is what the app knows there too. In
            // the source it would give away the place the eval asks for.
            let known_in_the_archive =
                matches!(origin, FieldOrigin::Rule(_)) && mode == LocationMode::Archive;
            if known_in_the_archive || matches!(origin, FieldOrigin::Ai { .. }) {
                continue;
            }
            let before = from_ai.clone();
            clear_field(&mut from_ai, field);
            not_from_the_text |= from_ai != before && !origin.is_heuristic();
        }
        let merged = detected.fill(&from_ai);
        let reason = if self.raw.ai_read_version(known.id).unwrap() != AI_READ_VERSION {
            &mut counts.asked_again_older_reading
        } else if not_from_the_text {
            &mut counts.asked_again_not_from_the_text
        } else if merged != stored {
            &mut counts.asked_again_detection_differs
        } else {
            return (merged, AI_READ_VERSION);
        };
        *reason += 1;
        (detected.fill(&DocumentFields::default()), 0)
    }
}

fn build_base(raw: &Path, bed: &Bed<'_>, kept: &Path) -> Built {
    assert!(
        Index::file_is_plain(raw).unwrap(),
        "NCRS_SEED_RAW is no plain index"
    );
    assert!(
        fs::symlink_metadata(kept).is_err(),
        "NCRS_SEED_KEPT exists already"
    );
    fs::create_dir_all(kept).unwrap();
    // Texts and files lie here unencrypted.
    #[cfg(unix)]
    fs::set_permissions(kept, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let master = kept.join(MASTER);
    for file in files_below(bed.archive).unwrap() {
        let copy = master.join(&file);
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        fs::copy(bed.archive.join(&file), copy).unwrap();
    }
    let raw_index = Index::open(raw, &InMemoryKeyStore::empty()).unwrap();
    let mut base = Base {
        raw: &raw_index,
        index: Index::create_plain(&kept.join("index.db")).unwrap(),
        new_hashes: bed.new_files.map(hashes_below).unwrap_or_default(),
    };
    let mut built = Built {
        new_files: base.new_hashes.len(),
        ..Built::default()
    };
    // The pipeline knows files by their canonical path.
    let run = canonical_path(kept).unwrap().join("run");
    for file in files_below(&master).unwrap() {
        base.adopt(
            &master.join(&file),
            run.join("archive").join(&file),
            LocationMode::Archive,
            &mut built.archive,
        );
    }
    for file in files_below(bed.source).unwrap() {
        base.adopt(
            &bed.source.join(&file),
            run.join("downloads").join(&file),
            LocationMode::Inbox,
            &mut built.source,
        );
    }
    built
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[test]
#[ignore = "hashes NCRS_EVAL_ARCHIVE and NCRS_EVAL_SOURCE; see the module doc"]
fn seed_hash_list() {
    let (Some(archive), Some(source), Some(target)) = (
        env_path("NCRS_EVAL_ARCHIVE"),
        env_path("NCRS_EVAL_SOURCE"),
        env_path("NCRS_SEED_HASHES"),
    ) else {
        println!("seed_hash_list: NCRS_EVAL_ARCHIVE, NCRS_EVAL_SOURCE or NCRS_SEED_HASHES is not set, nothing to do");
        return;
    };
    let list = hash_list(&[&archive, &source]);
    fs::write(target, &list).unwrap();
    println!("seed_hash_list: {} hashes", list.lines().count());
}

#[test]
#[ignore = "builds the folder brother_eval_read resumes from; see the module doc"]
fn seed_base() {
    let (Some(raw), Some(archive), Some(source), Some(kept)) = (
        env_path("NCRS_SEED_RAW"),
        env_path("NCRS_EVAL_ARCHIVE"),
        env_path("NCRS_EVAL_SOURCE"),
        env_path("NCRS_SEED_KEPT"),
    ) else {
        println!("seed_base: NCRS_SEED_RAW, NCRS_EVAL_ARCHIVE, NCRS_EVAL_SOURCE or NCRS_SEED_KEPT is not set, nothing to do");
        return;
    };
    let new_files = env_path("NCRS_EVAL_CONTINUOUS");
    let bed = Bed {
        archive: &archive,
        source: &source,
        new_files: new_files.as_deref(),
    };
    let built = build_base(&raw, &bed, &kept);
    for (part, counts) in [("archive", &built.archive), ("source", &built.source)] {
        println!(
            "{part}: {} files, {} adopted; of these asked again: {} older reading, {} value not from the text, {} detection differs",
            counts.files,
            counts.adopted,
            counts.asked_again_older_reading,
            counts.asked_again_not_from_the_text,
            counts.asked_again_detection_differs
        );
        println!(
            "{part}, left to the run: {} not in the index, {} without text, {} same content, {} also a new file",
            counts.not_in_the_index,
            counts.without_text,
            counts.same_content_as_another,
            counts.also_a_new_file
        );
    }
    println!("new files, not in the base: {}", built.new_files);
}

#[derive(Debug)]
struct SharedIndex(Mutex<Index>);

impl IndexAccess for SharedIndex {
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
        work(&mut self.0.lock().unwrap());
        true
    }
}

const LETTER: &str = "Muster GmbH\nHauptstr. 1, 09111 Chemnitz\n\nMarc Beispiel\nMusterweg 1\n00000 Musterstadt\n\nChemnitz, 28.01.2019\n\nRechnung\n\nBetrag 10 EUR\n";

struct Scene {
    dir: TempDir,
    kept: PathBuf,
    built: Built,
}

impl Scene {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// The archive holds a document the AI read, one with a value set by hand,
    /// one with a value from a rule, one with an older reading, a copy of the first
    /// and a file the index does not know; the source one the AI read, one with an empty text, one set by
    /// hand and one that is also a new file.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        for (file, content) in [
            ("archive/2019/read.txt", "read"),
            ("archive/2019/by-hand.txt", "by hand"),
            ("archive/2019/by-rule.txt", "by rule"),
            ("archive/older.txt", "older"),
            ("archive/z-copy.txt", "read"),
            ("archive/unknown.txt", "unknown"),
            ("archive/.hidden.txt", "hidden"),
            ("source/scan.txt", "scan"),
            ("source/empty.txt", "empty"),
            ("source/scan-by-hand.txt", "scan by hand"),
            ("source/also-new.txt", "new"),
            ("new/new.txt", "new"),
        ] {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let mut raw = Index::create_plain(&dir.path().join("raw.db")).unwrap();
        let ai = FieldOrigin::Ai {
            confidence_percent: 95,
        };
        let mut add = |file: &str, text: &str, number: FieldOrigin, ai_version: u32| {
            let id = raw
                .insert_document(&NewDocument {
                    hash: hash_path(&dir.path().join(file)).unwrap(),
                    path: PathBuf::from(file),
                    source: DocumentSource::Archive,
                    received_at: Utc::now(),
                    stamp: None,
                })
                .unwrap();
            raw.set_document_text(id, text).unwrap();
            let fields = if text.is_empty() {
                DocumentFields::default()
            } else {
                let detected = detect_fields(
                    text,
                    &DocumentFields::default(),
                    &SenderAliases::default(),
                    &OwnNames {
                        names: &[],
                        ibans: &[],
                        tax_ids: &[],
                    },
                    raw.kinds(),
                );
                DocumentFields {
                    sender: Some("Sender of the AI".to_owned()),
                    number: Some("4711".to_owned()),
                    ..detected.fill(&DocumentFields::default())
                }
            };
            raw.set_document_fields(id, &fields, |field| match field {
                Field::Sender => ai.clone(),
                Field::Number => number.clone(),
                _ => FieldOrigin::Detected,
            })
            .unwrap();
            raw.set_ai_read_version(id, ai_version).unwrap();
        };
        add("archive/2019/read.txt", LETTER, ai.clone(), AI_READ_VERSION);
        add(
            "archive/2019/by-hand.txt",
            LETTER,
            FieldOrigin::Manual,
            AI_READ_VERSION,
        );
        add(
            "archive/2019/by-rule.txt",
            LETTER,
            FieldOrigin::Rule(None),
            AI_READ_VERSION,
        );
        add("archive/older.txt", LETTER, ai.clone(), AI_READ_VERSION - 1);
        add("source/scan.txt", LETTER, ai.clone(), AI_READ_VERSION);
        add("source/empty.txt", "", ai.clone(), AI_READ_VERSION);
        add(
            "source/scan-by-hand.txt",
            LETTER,
            FieldOrigin::Rule(None),
            AI_READ_VERSION,
        );
        add("new/new.txt", LETTER, ai.clone(), AI_READ_VERSION);
        drop(raw);

        let kept = dir.path().join("kept");
        let built = build_base(
            &dir.path().join("raw.db"),
            &Bed {
                archive: &dir.path().join("archive"),
                source: &dir.path().join("source"),
                new_files: Some(&dir.path().join("new")),
            },
            &kept,
        );
        Self { dir, kept, built }
    }

    /// `run` as `start_protocol` makes it, with the new files in the source.
    fn start(&self) -> Started {
        let archive = self.kept.join("run/archive");
        let downloads = self.kept.join("run/downloads");
        for file in files_below(&self.kept.join(MASTER)).unwrap() {
            let link = archive.join(&file);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            fs::hard_link(self.kept.join(MASTER).join(&file), link).unwrap();
        }
        fs::create_dir_all(&downloads).unwrap();
        for folder in ["source", "new"] {
            for file in files_below(&self.path(folder)).unwrap() {
                fs::copy(self.path(folder).join(&file), downloads.join(&file)).unwrap();
            }
        }
        let index = Index::open(&self.kept.join("index.db"), &InMemoryKeyStore::empty()).unwrap();
        Started {
            archive: Location::new(&archive, LocationMode::Archive, false).unwrap(),
            downloads: Location::new(&downloads, LocationMode::Inbox, false).unwrap(),
            index: Arc::new(SharedIndex(Mutex::new(index))),
            provider: Arc::new(FakeProvider::replying("{}")),
        }
    }
}

struct Started {
    archive: Location,
    downloads: Location,
    index: Arc<SharedIndex>,
    provider: Arc<FakeProvider>,
}

/// How far a file got in the pipeline.
#[derive(Debug, PartialEq, Eq)]
enum Reading {
    Nothing,
    /// Asked the AI from the stored text.
    AiOnly,
    New,
}

impl Started {
    fn read(&self, location: &Location, file: &str) -> Reading {
        let steps = intake_steps(
            Arc::clone(&self.index) as Arc<dyn IndexAccess>,
            Arc::new(TextChain::new()),
            Some(Arc::clone(&self.provider) as Arc<dyn Provider>),
            OwnDetails::default(),
        );
        let mut item = Item::at_first_step(location, &location.path.join(file), false);
        let known = {
            let index = self.index.0.lock().unwrap();
            index.document_by_path(&item.path).unwrap().is_some()
        };
        let asked_before = self.provider.requests().len();
        let start = SystemTime::now();
        let mut outcome = steps[0].run(&mut item, start).unwrap();
        if outcome == StepOutcome::NotReady {
            let later = start + STABLE_AFTER + Duration::from_secs(1);
            outcome = steps[0].run(&mut item, later).unwrap();
        }
        if outcome == StepOutcome::Unchanged {
            return Reading::Nothing;
        }
        assert_eq!(outcome, StepOutcome::Continue);
        if !known {
            return Reading::New;
        }
        assert!(item.stored_text_is_current, "{file} is read again");
        for step in &steps[1..] {
            if step.run(&mut item, start).unwrap() != StepOutcome::Continue {
                break;
            }
        }
        let asked = self.provider.requests();
        assert_eq!(asked.len(), asked_before + 1);
        assert_eq!(asked[asked_before].text, LETTER);
        Reading::AiOnly
    }

    fn document(&self, location: &Location, file: &str) -> Option<Document> {
        let index = self.index.0.lock().unwrap();
        index.document_by_path(&location.path.join(file)).unwrap()
    }
}

#[test]
fn the_base_holds_the_files_of_the_bed_the_index_had_read() {
    let scene = Scene::new();
    assert_eq!(
        scene.built,
        Built {
            archive: Counts {
                files: 6,
                adopted: 4,
                asked_again_older_reading: 1,
                asked_again_not_from_the_text: 1,
                not_in_the_index: 1,
                same_content_as_another: 1,
                ..Counts::default()
            },
            source: Counts {
                files: 4,
                adopted: 2,
                asked_again_not_from_the_text: 1,
                without_text: 1,
                also_a_new_file: 1,
                ..Counts::default()
            },
            new_files: 1,
        }
    );
    assert!(scene.kept.join("master/2019/read.txt").is_file());
    assert!(!scene.kept.join("master/.hidden.txt").exists());
    assert!(!scene.kept.join("run").exists());
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&scene.kept).unwrap().permissions())
            & 0o777,
        0o700
    );
}

#[test]
fn a_document_of_the_base_is_not_read_again_and_a_new_file_is() {
    let scene = Scene::new();
    let run = scene.start();
    let (archive, downloads) = (&run.archive, &run.downloads);

    assert_eq!(run.read(archive, "2019/read.txt"), Reading::Nothing);
    assert_eq!(run.read(archive, "2019/by-rule.txt"), Reading::Nothing);
    assert_eq!(run.read(archive, "z-copy.txt"), Reading::Nothing);
    assert_eq!(run.read(downloads, "scan.txt"), Reading::Nothing);
    assert!(run.provider.requests().is_empty());
    assert_eq!(run.read(archive, "2019/by-hand.txt"), Reading::AiOnly);
    assert_eq!(run.read(archive, "older.txt"), Reading::AiOnly);
    assert_eq!(run.read(downloads, "scan-by-hand.txt"), Reading::AiOnly);
    assert_eq!(run.read(archive, "unknown.txt"), Reading::New);
    assert_eq!(run.read(downloads, "empty.txt"), Reading::New);
    assert!(run.document(downloads, "new.txt").is_none());
    assert_eq!(run.read(downloads, "new.txt"), Reading::New);
}

#[test]
fn the_documents_are_in_the_state_a_fresh_reading_leaves_them_in() {
    let scene = Scene::new();
    let run = scene.start();

    let read = run.document(&run.archive, "2019/read.txt").unwrap();
    assert_eq!(read.status, DocumentStatus::Filed);
    assert_eq!(read.source, DocumentSource::Archive);
    assert_eq!(read.fields.sender.as_deref(), Some("Sender of the AI"));
    assert_eq!(read.fields.number.as_deref(), Some("4711"));
    let by_rule = run.document(&run.archive, "2019/by-rule.txt").unwrap();
    assert_eq!(by_rule.fields.number.as_deref(), Some("4711"));
    let scan = run.document(&run.downloads, "scan.txt").unwrap();
    assert_eq!(scan.status, DocumentStatus::New);
    assert_eq!(scan.source, DocumentSource::Inbox);
    assert_eq!(scan.fields.sender.as_deref(), Some("Sender of the AI"));
    for (location, file) in [
        (&run.archive, "2019/by-hand.txt"),
        (&run.downloads, "scan-by-hand.txt"),
    ] {
        let document = run.document(location, file).unwrap();
        assert_eq!(document.fields.number, None);
        assert_eq!(document.fields.sender.as_deref(), Some("Muster GmbH"));
        let index = run.index.0.lock().unwrap();
        let origins = index.field_origins(document.id).unwrap();
        assert_eq!(origins.of(Field::Sender), FieldOrigin::Detected);
        assert!(index.has_document_text_row(document.id).unwrap());
    }
    let index = run.index.0.lock().unwrap();
    assert_eq!(index.statistics().unwrap().open_reviews, 0);
    assert_eq!(
        index.fields_read_version(scan.id).unwrap(),
        FIELDS_READ_VERSION
    );
    assert_eq!(index.ai_read_version(scan.id).unwrap(), AI_READ_VERSION);
    assert_eq!(
        index.fields_read_version(read.id).unwrap(),
        FIELDS_READ_VERSION
    );
}

#[test]
fn the_hash_list_names_every_content_once_in_the_form_the_extraction_reads() {
    let scene = Scene::new();
    let list = hash_list(&[&scene.path("archive"), &scene.path("source")]);
    let mut lines: Vec<&str> = list.lines().collect();
    assert_eq!(lines.len(), 9);
    assert!(lines.iter().all(|line| line.len() == 64));
    lines.dedup();
    assert_eq!(lines.len(), 9);
}
