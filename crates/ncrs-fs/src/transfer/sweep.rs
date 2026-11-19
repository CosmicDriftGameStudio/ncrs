//! Removing what an interrupted transfer left under a hidden name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const PREFIX: &str = ".ncrs-partial-";

const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

/// A leftover is only removed once it is this old: a second running instance
/// keeps its own staging fresh, so age is what tells a leftover from a live one.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether `name` is one of the hidden names this program gives staging
/// entries: `<prefix><target name>-<digits>-<digits>` for a staged file, the
/// random suffix of a temporary folder (6 letters/digits) or of a server
/// download (16 hex digits). Anything else that merely starts alike is the
/// user's.
pub fn is_staging_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(PREFIX) else {
        return false;
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let counted = rest
        .rsplit_once('-')
        .and_then(|(front, counter)| front.rsplit_once('-').map(|(_, id)| (id, counter)))
        .is_some_and(|(id, counter)| digits(id) && digits(counter));
    let random = (rest.len() == 6 && rest.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        || (rest.len() == 16 && rest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    counted || random
}

/// Removes the staging leftovers in `dir` that are older than a day. Names of
/// the form `.name.ncrs-PID-N` are never touched. Best effort: a folder that
/// cannot be read or an entry that cannot be removed is left alone.
///
/// A folder is read at most once an hour per process: a leftover is a day old
/// before it goes, and a batch of files into one folder must not list it again
/// for every file.
pub fn sweep_partials(dir: &Path) {
    if due_for_sweep(dir) {
        sweep_older_than(dir, STALE_AFTER);
    }
}

/// Whether `dir` was not swept within the last hour; says yes once, then no.
fn due_for_sweep(dir: &Path) -> bool {
    static LAST_SWEEP: std::sync::Mutex<Option<HashMap<PathBuf, Instant>>> =
        std::sync::Mutex::new(None);
    let mut guard = LAST_SWEEP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let swept = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let recent = swept
        .get(dir)
        .is_some_and(|last| now.duration_since(*last) < SWEEP_EVERY);
    if !recent {
        swept.insert(dir.to_path_buf(), now);
    }
    !recent
}

/// The later of the modification time and the time the entry was changed. A
/// staged copy is given its source's old modification time before it is placed,
/// so the modification time alone would make a live copy look stale.
fn last_touched(meta: &std::fs::Metadata) -> Option<SystemTime> {
    let modified = meta.modified().ok();
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt as _;
        u64::try_from(meta.ctime())
            .ok()
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    };
    #[cfg(not(unix))]
    let changed = meta.created().ok();
    modified.max(changed)
}

#[cfg(not(any(test, feature = "test-support")))]
fn clock_now() -> SystemTime {
    SystemTime::now()
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static CLOCK_SKEW: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
}

#[cfg(any(test, feature = "test-support"))]
fn clock_now() -> SystemTime {
    SystemTime::now() + CLOCK_SKEW.with(std::cell::Cell::get)
}

/// The change time of a file cannot be set back, so tests move the sweep's
/// clock of this thread forward instead.
#[cfg(any(test, feature = "test-support"))]
pub fn let_time_pass_for_sweep(by: Duration) {
    CLOCK_SKEW.with(|skew| skew.set(by));
}

fn sweep_older_than(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = clock_now();
    for entry in entries.filter_map(Result::ok) {
        if !is_staging_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let old_enough = last_touched(&meta)
            .and_then(|touched| now.duration_since(touched).ok())
            .is_some_and(|passed| passed >= age);
        if !old_enough {
            continue;
        }
        let _best_effort = if meta.is_dir() {
            std::fs::remove_dir_all(&path)
        } else if meta.is_file() || meta.file_type().is_symlink() {
            std::fs::remove_file(&path)
        } else {
            continue;
        };
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    const THREE_DAYS: Duration = Duration::from_secs(3 * 24 * 60 * 60);

    fn backdate(path: &Path, age: Duration) {
        date(path, SystemTime::now() - age);
    }

    // Windows sets the time only through a handle opened for writing.
    fn date(path: &Path, modified: SystemTime) {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    /// Makes everything written so far three days old for the sweep, except
    /// what is dated ahead of the skewed clock.
    fn let_three_days_pass() {
        let_time_pass_for_sweep(THREE_DAYS);
    }

    #[test]
    fn only_old_staging_names_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old_file = dir.path().join(".ncrs-partial-a.pdf-1234-7");
        let fresh_file = dir.path().join(".ncrs-partial-b.pdf-1234-8");
        let old_folder = dir.path().join(".ncrs-partial-Ab3dE9");
        let old_remote = dir.path().join(".ncrs-partial-00ff00ff00ff00ff");
        let users_own = dir.path().join(".ncrs-partial-notes");
        let other_scheme = dir.path().join(".a.pdf.ncrs-1234-1");
        for file in [&old_file, &fresh_file, &users_own, &other_scheme] {
            std::fs::write(file, "x").unwrap();
        }
        std::fs::create_dir(&old_folder).unwrap();
        std::fs::write(old_folder.join("half"), "x").unwrap();
        std::fs::write(&old_remote, "x").unwrap();
        let_three_days_pass();
        let ahead = SystemTime::now() + THREE_DAYS;
        date(&fresh_file, ahead);

        sweep_partials(dir.path());

        assert!(!old_file.exists());
        assert!(!old_folder.exists());
        assert!(!old_remote.exists());
        assert!(fresh_file.exists(), "a live transfer keeps a fresh name");
        assert!(users_own.exists(), "not one of our names");
        assert!(other_scheme.exists(), "never the other scheme");
    }

    #[test]
    fn a_staged_copy_with_an_old_modification_time_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join(".ncrs-partial-a.pdf-1234-9");
        std::fs::write(&live, "x").unwrap();
        backdate(&live, THREE_DAYS);

        sweep_partials(dir.path());

        assert!(live.exists(), "its change time is now");

        let_three_days_pass();
        sweep_older_than(dir.path(), STALE_AFTER);

        assert!(!live.exists(), "and a day later it is a leftover");
    }

    #[test]
    fn a_folder_is_not_read_again_within_the_hour() {
        let dir = tempfile::tempdir().unwrap();
        sweep_partials(dir.path());
        let leftover = dir.path().join(".ncrs-partial-a.pdf-1234-5");
        std::fs::write(&leftover, "x").unwrap();
        let_three_days_pass();

        sweep_partials(dir.path());

        assert!(leftover.exists(), "the second call listed the folder again");
    }

    #[test]
    fn a_transfer_sweeps_the_leftover_of_a_crashed_run() {
        let dir = tempfile::tempdir().unwrap();
        let leftover = dir.path().join(".ncrs-partial-a.txt-99999-1");
        std::fs::write(&leftover, "half").unwrap();
        backdate(&leftover, Duration::from_secs(2 * 24 * 60 * 60));
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("a.txt"), "x").unwrap();
        let target = dir.path().join("dst");
        std::fs::create_dir(&target).unwrap();
        let leftover_there = target.join(".ncrs-partial-a.txt-99999-1");
        std::fs::write(&leftover_there, "half").unwrap();
        backdate(&leftover_there, Duration::from_secs(2 * 24 * 60 * 60));
        let_three_days_pass();

        crate::transfer::run(
            &source.join("a.txt"),
            &target,
            ncrs_core::TransferKind::Copy,
            crate::transfer::OnConflict::Fail,
        )
        .unwrap();

        assert!(!leftover_there.exists());
        assert!(leftover.exists(), "only the target folder is swept");
    }
}
