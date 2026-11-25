//! Step A of the filing eval's test base, the only one that needs the key: the
//! listed documents of an encrypted index go into a plain file. Steps before
//! and after it are in `eval_base` of `ncrs-pipeline`.
//!
//! ```text
//! NCRS_SEED_SOURCE_INDEX=<encrypted index, only copied> NCRS_SEED_HASHES=<hash list>
//! NCRS_SEED_RAW=<new file> cargo test -p ncrs-index --lib
//! -- --ignored --exact eval_seed::seed_extract --nocapture
//! ```
//!
//! Everything that can be checked without the key is checked before the key is
//! asked for. Only the documents with their text, fields, origins and reading
//! versions are taken: rules, evidence, aliases, own tags and every path stay
//! behind, because what the index learned already knows the right places.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use tempfile::TempDir;

use crate::test_support::{new_document, rule_spec};
use crate::{
    ContentHash, DocumentFields, DocumentStatus, Field, FieldOrigin, FileStamp, InMemoryKeyStore,
    Index, IndexError, KeyStore, KeyStoreError, KeyringKeyStore, NewDocument, ReviewReason,
};

const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];
/// SQLite's smallest page; a shorter file is no index.
const SMALLEST_INDEX: u64 = 512;
/// The tables a raw file may have rows in. The tags among them are the ones
/// that follow from the fields.
const RAW_TABLES: [&str; 12] = [
    "document",
    "document_text",
    "document_text_data",
    "document_text_idx",
    "document_text_content",
    "document_text_docsize",
    "document_text_config",
    "document_field_origin",
    "document_tag",
    "document_kind",
    "sender",
    "tag",
];

#[derive(Debug, Default, PartialEq, Eq)]
struct Extracted {
    listed: usize,
    found: usize,
    with_text: usize,
    unreadable: usize,
}

/// What is ready before the key is asked for.
struct Prepared {
    hashes: BTreeSet<[u8; 32]>,
    copy: PathBuf,
    _copy_folder: TempDir,
    staged: PathBuf,
    _staging: TempDir,
    target: Index,
}

fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_hashes(list: &str) -> Result<BTreeSet<[u8; 32]>, String> {
    let mut hashes = BTreeSet::new();
    for (at, line) in list.lines().enumerate() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let wrong = || format!("line {} of the hash list is no hash", at + 1);
        if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(wrong());
        }
        let mut hash: [u8; 32] = [0; 32];
        for (place, byte) in hash.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[place * 2..place * 2 + 2], 16)
                .map_err(|_not_hex| wrong())?;
        }
        hashes.insert(hash);
    }
    if hashes.is_empty() {
        return Err("the hash list is empty".to_owned());
    }
    Ok(hashes)
}

fn refuse_sidecars(source: &Path) -> Result<(), String> {
    for suffix in SIDECARS {
        let mut name = OsString::from(source);
        name.push(suffix);
        if fs::symlink_metadata(&name).is_ok() {
            return Err(format!(
                "the source index has a {suffix} file beside it: quit the app first"
            ));
        }
    }
    Ok(())
}

fn size_and_time(source: &Path) -> Result<(u64, SystemTime), String> {
    let metadata = fs::metadata(source).map_err(|err| format!("source index: {err}"))?;
    if !metadata.is_file() || metadata.len() < SMALLEST_INDEX {
        return Err("the source index is no index file".to_owned());
    }
    let changed = metadata
        .modified()
        .map_err(|err| format!("source index: {err}"))?;
    Ok((metadata.len(), changed))
}

fn prepare(source: &Path, hash_list: &Path, raw: &Path) -> Result<Prepared, String> {
    let before = size_and_time(source)?;
    if Index::file_is_plain(source).map_err(|err| format!("source index: {err}"))? {
        return Err("the source index is not encrypted".to_owned());
    }
    refuse_sidecars(source)?;
    let list = fs::read_to_string(hash_list).map_err(|err| format!("hash list: {err}"))?;
    let hashes = parse_hashes(&list)?;
    match fs::symlink_metadata(raw) {
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Ok(_) => return Err("the output exists already".to_owned()),
        Err(err) => return Err(format!("output: {err}")),
    }
    let folder = raw.parent().ok_or("the output has no folder")?;
    // A folder only the user may enter, next to the output: the plain file is
    // never readable by others, not even while it is written.
    let staging = TempDir::new_in(folder).map_err(|err| format!("output folder: {err}"))?;
    let staged = staging.path().join("raw.db");
    let target = Index::create_plain(&staged).map_err(|err| format!("output: {err}"))?;
    #[cfg(unix)]
    fs::set_permissions(&staged, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|err| format!("output: {err}"))?;

    let copy_folder = TempDir::new().map_err(|err| format!("copy folder: {err}"))?;
    let copy = copy_folder.path().join("index.db");
    let copied = fs::copy(source, &copy).map_err(|err| format!("copying the index: {err}"))?;
    refuse_sidecars(source)?;
    if size_and_time(source)? != before || copied != before.0 {
        return Err("the source index changed while it was copied".to_owned());
    }
    Ok(Prepared {
        hashes,
        copy,
        _copy_folder: copy_folder,
        staged,
        _staging: staging,
        target,
    })
}

enum Copied {
    NotThere,
    Document { with_text: bool },
}

fn copy_document(
    source: &Index,
    target: &mut Index,
    hash: &[u8; 32],
) -> Result<Copied, IndexError> {
    let content = ContentHash(*hash);
    let Some(document) = source.document_by_hash(&content)? else {
        return Ok(Copied::NotThere);
    };
    let text = source.document_text(document.id)?;
    let origins = source.field_origins(document.id)?;
    let fields_version = source.fields_read_version(document.id)?;
    let ai_version = source.ai_read_version(document.id)?;

    let id = target.insert_document(&NewDocument {
        hash: content,
        path: PathBuf::from(hex(hash)),
        source: document.source,
        received_at: document.received_at,
        stamp: None,
    })?;
    let fields = DocumentFields {
        tags: BTreeSet::new(),
        ..document.fields
    };
    let mut write = || {
        target.set_document_fields(id, &fields, |field| match origins.of(field) {
            FieldOrigin::Rule(_) => FieldOrigin::Rule(None),
            other => other,
        })?;
        if let Some(stored) = &text {
            target.set_document_text(id, stored)?;
        }
        target.set_fields_read_version(id, fields_version)?;
        target.set_ai_read_version(id, ai_version)
    };
    if let Err(error) = write() {
        target.delete_document(id)?;
        return Err(error);
    }
    Ok(Copied::Document {
        with_text: text.is_some_and(|stored| !stored.trim().is_empty()),
    })
}

/// The raw file holds nothing but listed documents and what hangs on them.
fn check_raw(raw: &Path, hashes: &BTreeSet<[u8; 32]>, found: usize) -> Result<(), String> {
    let index = Index::open(raw, &InMemoryKeyStore::empty()).map_err(|err| err.to_string())?;
    let conn = index.connection();
    let check = || -> rusqlite::Result<Result<(), String>> {
        let mut rows = conn.prepare("SELECT hash FROM document")?;
        let stored = rows
            .query_map([], |row| row.get::<_, [u8; 32]>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        if stored.len() != found || !stored.is_subset(hashes) {
            return Ok(Err(
                "the output holds other documents than the listed".to_owned()
            ));
        }
        let own_tags: i64 = conn.query_row(
            "SELECT count(*) FROM document_tag WHERE automatic = 0",
            [],
            |row| row.get(0),
        )?;
        if own_tags > 0 {
            return Ok(Err("the output holds tags somebody set".to_owned()));
        }
        let mut names = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )?;
        let tables = names
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for table in tables {
            let count: i64 =
                conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?;
            if count > 0 && !RAW_TABLES.contains(&table.as_str()) {
                return Ok(Err(format!("the output has rows in {table}")));
            }
        }
        Ok(Ok(()))
    };
    check().map_err(|err| err.to_string())?
}

fn extract(
    source: &Path,
    hash_list: &Path,
    raw: &Path,
    store: &dyn KeyStore,
) -> Result<Extracted, String> {
    let mut prepared = prepare(source, hash_list, raw)?;
    let index = Index::open(&prepared.copy, store).map_err(|err| format!("opening: {err}"))?;
    let mut counts = Extracted {
        listed: prepared.hashes.len(),
        ..Extracted::default()
    };
    for hash in &prepared.hashes {
        // The key is given once: one row that cannot be read must not cost the
        // whole run.
        match copy_document(&index, &mut prepared.target, hash) {
            Ok(Copied::NotThere) => {}
            Ok(Copied::Document { with_text }) => {
                counts.found += 1;
                counts.with_text += usize::from(with_text);
            }
            Err(_) => counts.unreadable += 1,
        }
    }
    drop(index);
    drop(prepared.target);
    check_raw(&prepared.staged, &prepared.hashes, counts.found)?;
    fs::rename(&prepared.staged, raw).map_err(|err| format!("output: {err}"))?;
    Ok(counts)
}

#[test]
#[ignore = "asks the credential store for the key of a real index; see the module doc"]
fn seed_extract() {
    let path = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let (Some(source), Some(hash_list), Some(raw)) = (
        path("NCRS_SEED_SOURCE_INDEX"),
        path("NCRS_SEED_HASHES"),
        path("NCRS_SEED_RAW"),
    ) else {
        println!("seed_extract: NCRS_SEED_SOURCE_INDEX, NCRS_SEED_HASHES or NCRS_SEED_RAW is not set, nothing to do");
        return;
    };
    let counts = extract(&source, &hash_list, &raw, &KeyringKeyStore::new()).unwrap();
    println!(
        "seed_extract: {} hashes listed, {} documents found, {} with text, {} unreadable",
        counts.listed, counts.found, counts.with_text, counts.unreadable
    );
}

/// Counts every touch, writing included.
#[derive(Debug)]
struct CountingStore {
    inner: InMemoryKeyStore,
    touches: AtomicUsize,
}

impl CountingStore {
    fn new(inner: InMemoryKeyStore) -> Self {
        Self {
            inner,
            touches: AtomicUsize::new(0),
        }
    }

    fn touches(&self) -> usize {
        self.touches.load(Ordering::SeqCst)
    }
}

impl KeyStore for CountingStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        self.touches.fetch_add(1, Ordering::SeqCst);
        self.inner.load()
    }

    fn store_new(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.touches.fetch_add(1, Ordering::SeqCst);
        self.inner.store_new(key)
    }

    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.touches.fetch_add(1, Ordering::SeqCst);
        self.inner.replace(key)
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        self.touches.fetch_add(1, Ordering::SeqCst);
        self.inner.delete()
    }
}

struct Scene {
    dir: TempDir,
    store: CountingStore,
}

impl Scene {
    /// An encrypted index with the documents 1, 2 and 3 and everything the
    /// extraction has to leave behind; 1, 2 and the unknown 9 are listed.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let store = InMemoryKeyStore::empty();
        let (mut index, _) = Index::create_encrypted(&dir.path().join("index.db"), &store).unwrap();
        let rule = index.create_rule(&rule_spec()).unwrap();
        for seed in 1..=3 {
            let id = index.insert_document(&new_document(seed)).unwrap();
            index
                .set_document_text(id, &format!("text of {seed}"))
                .unwrap();
            let fields = DocumentFields {
                sender: Some(format!("Sender {seed}")),
                recipient: Some("Erika".to_owned()),
                kind: Some("invoice".to_owned()),
                number: Some("7".to_owned()),
                tags: BTreeSet::from(["own".to_owned()]),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &fields, |field| match field {
                    Field::Sender => FieldOrigin::Ai {
                        confidence_percent: 90,
                    },
                    Field::Kind => FieldOrigin::Rule(Some(rule)),
                    Field::Number => FieldOrigin::Manual,
                    _ => FieldOrigin::AddressBlock,
                })
                .unwrap();
            index.set_ai_read_version(id, 2).unwrap();
            index.set_fields_read_version(id, 1).unwrap();
            index.add_review(id, ReviewReason::NoMatchingRule).unwrap();
            index
                .record_duplicate_path(
                    id,
                    Path::new("copy"),
                    FileStamp {
                        size: 1,
                        mtime: SystemTime::UNIX_EPOCH,
                    },
                )
                .unwrap();
            index
                .set_document_status(id, DocumentStatus::Filed)
                .unwrap();
        }
        drop(index);
        let scene = Self {
            dir,
            store: CountingStore::new(store),
        };
        scene.list(&[1, 2, 9]);
        scene
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn list(&self, seeds: &[u8]) {
        let lines: Vec<String> = seeds.iter().map(|seed| hex(&[*seed; 32])).collect();
        fs::write(self.path("hashes"), lines.join("\n")).unwrap();
    }

    fn extract(&self) -> Result<Extracted, String> {
        extract(
            &self.path("index.db"),
            &self.path("hashes"),
            &self.path("raw.db"),
            &self.store,
        )
    }

    /// The run stops before the key, and leaves no output.
    fn refused(&self, because: &str) {
        let raw_before = fs::read(self.path("raw.db")).ok();
        let error = self.extract().unwrap_err();
        assert!(error.contains(because), "{error}");
        assert_eq!(self.store.touches(), 0, "the key was asked for: {error}");
        assert_eq!(fs::read(self.path("raw.db")).ok(), raw_before);
    }
}

#[test]
fn only_the_listed_documents_are_extracted_with_one_look_at_the_key() {
    let scene = Scene::new();
    let before = fs::read(scene.path("index.db")).unwrap();

    let counts = scene.extract().unwrap();

    assert_eq!(
        counts,
        Extracted {
            listed: 3,
            found: 2,
            with_text: 2,
            unreadable: 0
        }
    );
    assert_eq!(scene.store.touches(), 1);
    assert!(before == fs::read(scene.path("index.db")).unwrap());
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &fs::metadata(scene.path("raw.db")).unwrap().permissions()
        ) & 0o777,
        0o600
    );
    let mut left: Vec<_> = fs::read_dir(scene.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(left, ["hashes", "index.db", "raw.db"]);

    let raw = Index::open(&scene.path("raw.db"), &InMemoryKeyStore::empty()).unwrap();
    assert!(!raw.is_encrypted());
    assert!(raw
        .document_by_hash(&ContentHash([3; 32]))
        .unwrap()
        .is_none());
    let document = raw
        .document_by_hash(&ContentHash([1; 32]))
        .unwrap()
        .unwrap();
    assert_eq!(document.path, PathBuf::from(hex(&[1; 32])));
    assert_eq!(document.fields.sender.as_deref(), Some("Sender 1"));
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
    assert!(document.fields.tags.is_empty());
    assert_eq!(
        raw.document_text(document.id).unwrap().as_deref(),
        Some("text of 1")
    );
    assert_eq!(raw.ai_read_version(document.id).unwrap(), 2);
    assert_eq!(raw.fields_read_version(document.id).unwrap(), 1);
    let origins = raw.field_origins(document.id).unwrap();
    assert_eq!(
        origins.of(Field::Sender),
        FieldOrigin::Ai {
            confidence_percent: 90
        }
    );
    assert_eq!(origins.of(Field::Kind), FieldOrigin::Rule(None));
    assert_eq!(origins.of(Field::Number), FieldOrigin::Manual);
    assert!(raw.rules().unwrap().is_empty());
    assert_eq!(raw.statistics().unwrap().open_reviews, 0);
}

#[test]
fn a_problem_seen_without_the_key_stops_before_the_key() {
    let scene = Scene::new();

    for (list, because) in [("", "empty"), ("abc", "line 1"), ("\nzz", "line 2")] {
        fs::write(scene.path("hashes"), list).unwrap();
        scene.refused(because);
    }
    fs::remove_file(scene.path("hashes")).unwrap();
    scene.refused("hash list");
    scene.list(&[1]);

    for suffix in SIDECARS {
        let sidecar = scene.path(&format!("index.db{suffix}"));
        fs::write(&sidecar, b"").unwrap();
        scene.refused(suffix);
        fs::remove_file(sidecar).unwrap();
    }

    fs::write(scene.path("raw.db"), b"there").unwrap();
    scene.refused("exists already");
    fs::remove_file(scene.path("raw.db")).unwrap();

    let encrypted = fs::read(scene.path("index.db")).unwrap();
    fs::write(scene.path("index.db"), b"").unwrap();
    scene.refused("no index file");
    fs::remove_file(scene.path("index.db")).unwrap();
    scene.refused("source index");
    drop(Index::create_plain(&scene.path("index.db")).unwrap());
    scene.refused("not encrypted");

    fs::write(scene.path("index.db"), encrypted).unwrap();
    let nowhere = scene.path("no-folder").join("raw.db");
    let error = extract(
        &scene.path("index.db"),
        &scene.path("hashes"),
        &nowhere,
        &scene.store,
    )
    .unwrap_err();
    assert!(error.contains("output folder"), "{error}");
    assert_eq!(scene.store.touches(), 0);
}
