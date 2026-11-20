//! Keeping the notes folder in a second folder, off the UI thread.
//!
//! One way copies new and changed note files to the target and deletes
//! nothing there. Both ways lets the newer file win; when both sides changed
//! since the last sync the older one stays next to it as "Name (Conflict).md".
//! Encrypted notes are copied as they are: they stay encrypted. Nothing is
//! ever deleted, on either side.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ncrs_core::notes_sync::NotesSyncDirection;

use crate::crypt::ENCRYPTED_SUFFIX;
use crate::store::{content_hash, FILES_DIR};

const MAX_DEPTH: usize = 16;
/// Two times this close, with the same size, are the same file: some disks
/// (FAT, exFAT) keep only every second.
const SAME_TIME: Duration = Duration::from_secs(2);
const MARKDOWN_SUFFIX: &str = ".md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncJob {
    pub local: PathBuf,
    pub target: PathBuf,
    pub direction: NotesSyncDirection,
    /// The word in "Name (Conflict).md", in the language of the program.
    pub conflict_suffix: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Files written to either side.
    pub copied: usize,
    /// Notes saved as a conflict copy.
    pub conflicts: usize,
    /// Notes in the notes folder after the run.
    pub notes: usize,
    /// When the run ended.
    pub finished: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Meta {
    size: u64,
    modified: SystemTime,
}

fn same_file(first: Meta, second: Meta) -> bool {
    let apart = first
        .modified
        .duration_since(second.modified)
        .or_else(|_| second.modified.duration_since(first.modified))
        .unwrap_or_default();
    first.size == second.size && apart < SAME_TIME
}

fn nanos(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}

fn is_note_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(MARKDOWN_SUFFIX) || lower.ends_with(ENCRYPTED_SUFFIX)
}

fn listed(root: &Path) -> BTreeMap<PathBuf, Meta> {
    let mut found = BTreeMap::new();
    walk(root, Path::new(""), 0, &mut found);
    found
}

fn walk(root: &Path, relative: &Path, depth: usize, found: &mut BTreeMap<PathBuf, Meta>) {
    let Ok(entries) = fs::read_dir(root.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name != FILES_DIR {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let below = relative.join(&name);
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &below, depth + 1, found);
            }
        } else if is_note_name(&name) {
            if let Some(meta) = entry.metadata().ok().filter(|meta| meta.is_file()) {
                if let Ok(modified) = meta.modified() {
                    found.insert(
                        below,
                        Meta {
                            size: meta.len(),
                            modified,
                        },
                    );
                }
            }
        }
    }
}

/// What both sides looked like when they last matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Record {
    local: (u64, u128),
    remote: (u64, u128),
}

fn key_of(meta: Meta) -> (u64, u128) {
    (meta.size, nanos(meta.modified))
}

type Manifest = BTreeMap<PathBuf, Record>;

fn manifest_path(local: &Path, target: &Path) -> PathBuf {
    let id = content_hash(target.to_string_lossy().as_bytes()) >> 32;
    local.join(format!(".sync-{id:08x}"))
}

fn read_manifest(path: &Path) -> Manifest {
    let Ok(text) = fs::read_to_string(path) else {
        return Manifest::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?;
            let mut number = || parts.next()?.parse::<u128>().ok();
            let (local_size, local_time) = (number()?, number()?);
            let (remote_size, remote_time) = (number()?, number()?);
            Some((
                PathBuf::from(name),
                Record {
                    local: (u64::try_from(local_size).ok()?, local_time),
                    remote: (u64::try_from(remote_size).ok()?, remote_time),
                },
            ))
        })
        .collect()
}

fn write_manifest(path: &Path, manifest: &Manifest) -> std::io::Result<()> {
    let mut text = String::new();
    for (name, record) in manifest {
        let shown = name.to_string_lossy();
        if shown.contains(['\t', '\n']) {
            continue;
        }
        text += &format!(
            "{shown}\t{}\t{}\t{}\t{}\n",
            record.local.0, record.local.1, record.remote.0, record.remote.1
        );
    }
    fs::write(path, text)
}

/// Copies `from` to `to` with its modification time, through a temporary file
/// so a crash leaves the old file or the new one.
fn copy_file(from: &Path, to: &Path, modified: SystemTime) -> Result<(), String> {
    let describe = |err: std::io::Error| format!("{}: {err}", to.display());
    let bytes = fs::read(from).map_err(|err| format!("{}: {err}", from.display()))?;
    write_file(to, &bytes, modified).map_err(describe)
}

fn write_file(to: &Path, bytes: &[u8], modified: SystemTime) -> std::io::Result<()> {
    let folder = to
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder"))?;
    fs::create_dir_all(folder)?;
    let mut file = tempfile::NamedTempFile::new_in(folder)?;
    file.write_all(bytes)?;
    file.as_file().set_modified(modified)?;
    file.as_file().sync_all()?;
    file.persist(to).map(drop).map_err(|err| err.error)
}

/// `Name.md` as `Name (Conflict).md`, or with a number when that is taken on
/// either side. `.md.enc` keeps its ending.
fn conflict_name(relative: &Path, suffix: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = name.to_lowercase();
    let ending = if lower.ends_with(ENCRYPTED_SUFFIX) {
        ENCRYPTED_SUFFIX.len()
    } else if lower.ends_with(MARKDOWN_SUFFIX) {
        MARKDOWN_SUFFIX.len()
    } else {
        0
    };
    let split = name.len().saturating_sub(ending);
    let (stem, tail) = name.split_at_checked(split).unwrap_or((name.as_str(), ""));
    let folder = relative.parent().unwrap_or(Path::new(""));
    let mut number: u32 = 1;
    loop {
        let label = if number == 1 {
            suffix.to_owned()
        } else {
            format!("{suffix} {number}")
        };
        let candidate = folder.join(format!("{stem} ({label}){tail}"));
        if !taken(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

/// Runs the sync. `now` is when it ends, as the report says.
pub fn run(job: &SyncJob, now: SystemTime) -> Result<SyncReport, String> {
    if !job.target.is_dir() {
        let parent_exists = job.target.parent().is_some_and(Path::is_dir);
        if !parent_exists {
            return Err(format!("{} was not found", job.target.display()));
        }
        fs::create_dir(&job.target).map_err(|err| format!("{}: {err}", job.target.display()))?;
    }
    if folders_overlap(&job.local, &job.target) {
        return Err(
            "the sync folder and the notes folder must not be the same or inside one another"
                .to_owned(),
        );
    }
    match job.direction {
        NotesSyncDirection::OneWay => one_way(job, now),
        NotesSyncDirection::BothWays => both_ways(job, now),
    }
}

fn folders_overlap(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(left), Ok(right)) => left.starts_with(&right) || right.starts_with(&left),
        _ => false,
    }
}

fn finish(
    mut report: SyncReport,
    job: &SyncJob,
    errors: &[String],
    now: SystemTime,
) -> Result<SyncReport, String> {
    report.notes = listed(&job.local).len();
    report.finished = Some(now);
    match errors.first() {
        Some(first) if errors.len() > 1 => Err(format!("{first} (+{} more)", errors.len() - 1)),
        Some(first) => Err(first.clone()),
        None => Ok(report),
    }
}

fn one_way(job: &SyncJob, now: SystemTime) -> Result<SyncReport, String> {
    let local = listed(&job.local);
    let remote = listed(&job.target);
    let mut report = SyncReport::default();
    let mut errors = Vec::new();
    for (name, mine) in &local {
        let current = remote
            .get(name)
            .is_some_and(|theirs| same_file(*mine, *theirs));
        if current {
            continue;
        }
        match copy_file(&job.local.join(name), &job.target.join(name), mine.modified) {
            Ok(()) => report.copied += 1,
            Err(reason) => errors.push(reason),
        }
    }
    finish(report, job, &errors, now)
}

fn both_ways(job: &SyncJob, now: SystemTime) -> Result<SyncReport, String> {
    let manifest_file = manifest_path(&job.local, &job.target);
    let known = read_manifest(&manifest_file);
    let local = listed(&job.local);
    let remote = listed(&job.target);
    let mut report = SyncReport::default();
    let mut errors = Vec::new();
    let mut names: Vec<&PathBuf> = local.keys().chain(remote.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let step = match (local.get(name).copied(), remote.get(name).copied()) {
            (Some(mine), None) => {
                copy_file(&job.local.join(name), &job.target.join(name), mine.modified)
                    .map(|()| report.copied += 1)
            }
            (None, Some(theirs)) => copy_file(
                &job.target.join(name),
                &job.local.join(name),
                theirs.modified,
            )
            .map(|()| report.copied += 1),
            (Some(mine), Some(theirs)) if same_file(mine, theirs) => Ok(()),
            (Some(mine), Some(theirs)) => {
                let record = known.get(name);
                let mine_changed = record.is_none_or(|rec| rec.local != key_of(mine));
                let theirs_changed = record.is_none_or(|rec| rec.remote != key_of(theirs));
                match (mine_changed, theirs_changed) {
                    (true, false) => {
                        copy_file(&job.local.join(name), &job.target.join(name), mine.modified)
                            .map(|()| report.copied += 1)
                    }
                    (false, true) => copy_file(
                        &job.target.join(name),
                        &job.local.join(name),
                        theirs.modified,
                    )
                    .map(|()| report.copied += 1),
                    (false, false) => Ok(()),
                    (true, true) => conflicted(job, name, mine, theirs, &mut report),
                }
            }
            (None, None) => Ok(()),
        };
        if let Err(reason) = step {
            errors.push(reason);
        }
    }
    let after_local = listed(&job.local);
    let after_remote = listed(&job.target);
    let matched: Manifest = after_local
        .iter()
        .filter_map(|(name, mine)| {
            let theirs = after_remote.get(name)?;
            same_file(*mine, *theirs).then(|| {
                (
                    name.clone(),
                    Record {
                        local: key_of(*mine),
                        remote: key_of(*theirs),
                    },
                )
            })
        })
        .collect();
    if let Err(err) = write_manifest(&manifest_file, &matched) {
        errors.push(format!("{}: {err}", manifest_file.display()));
    }
    finish(report, job, &errors, now)
}

/// Both sides changed: the newer file takes the name on both sides, the older
/// one is kept next to it under the conflict name, also on both sides. The
/// same bytes on both sides are no conflict.
fn conflicted(
    job: &SyncJob,
    name: &Path,
    mine: Meta,
    theirs: Meta,
    report: &mut SyncReport,
) -> Result<(), String> {
    let describe = |path: &Path, err: std::io::Error| format!("{}: {err}", path.display());
    let local_path = job.local.join(name);
    let remote_path = job.target.join(name);
    let local_bytes = fs::read(&local_path).map_err(|err| describe(&local_path, err))?;
    let remote_bytes = fs::read(&remote_path).map_err(|err| describe(&remote_path, err))?;
    if local_bytes == remote_bytes {
        let newest = mine.modified.max(theirs.modified);
        write_file(&local_path, &local_bytes, newest).map_err(|err| describe(&local_path, err))?;
        write_file(&remote_path, &remote_bytes, newest)
            .map_err(|err| describe(&remote_path, err))?;
        return Ok(());
    }
    let local_is_newer = mine.modified >= theirs.modified;
    let (older_bytes, older_time, newer_bytes, newer_time) = if local_is_newer {
        (&remote_bytes, theirs.modified, &local_bytes, mine.modified)
    } else {
        (&local_bytes, mine.modified, &remote_bytes, theirs.modified)
    };
    let copy_name = conflict_name(name, &job.conflict_suffix, |candidate| {
        job.local.join(candidate).exists() || job.target.join(candidate).exists()
    });
    for side in [&job.local, &job.target] {
        let path = side.join(&copy_name);
        write_file(&path, older_bytes, older_time).map_err(|err| describe(&path, err))?;
        let named = side.join(name);
        write_file(&named, newer_bytes, newer_time).map_err(|err| describe(&named, err))?;
    }
    report.conflicts += 1;
    report.copied += 4;
    Ok(())
}
