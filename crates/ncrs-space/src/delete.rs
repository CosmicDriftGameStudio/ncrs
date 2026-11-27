use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::catalog::CatalogId;
use crate::checks::{check_before_delete, ProcessSource, Skip, UnknownUse};
use crate::log::{LogRow, RunLog};
use crate::scan::{Entry, EntryKind, Group};

/// Blocking. Checks each entry again right before it goes, then deletes it for good.
/// Only entries of the safe group are touched; `stop` is read between entries.
pub fn delete_safe(
    entries: &[Entry],
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
    stop: &AtomicBool,
    mut on_row: impl FnMut(&LogRow),
) -> RunLog {
    let mut log = RunLog::new(SystemTime::now());
    for entry in entries {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let outcome = delete_one(entry, probes, unknown);
        let row = LogRow { path: entry.path.clone(), size: entry.size.bytes, outcome };
        on_row(&row);
        log.rows.push(row);
    }
    log.finished = SystemTime::now();
    log
}

fn delete_one(entry: &Entry, probes: &dyn ProcessSource, unknown: UnknownUse) -> Result<u64, Skip> {
    if entry.group != Group::Safe {
        return Err(Skip::Failed("not a safe entry".to_owned()));
    }
    check_before_delete(entry, probes, unknown)?;
    match &entry.kind {
        EntryKind::Catalog(CatalogId::UnavailableSimulators) => delete_unavailable_simulators()?,
        EntryKind::Catalog(_) => {
            ncrs_fs::remove_permanently(&entry.path).map_err(|error| Skip::from_io(&error))?;
        }
        EntryKind::Worktree(_) | EntryKind::Decide(_) => {
            return Err(Skip::Failed("not a safe entry".to_owned()));
        }
    }
    Ok(entry.size.bytes)
}

#[cfg(all(target_os = "macos", not(feature = "sandbox")))]
fn delete_unavailable_simulators() -> Result<(), Skip> {
    let status = std::process::Command::new("xcrun")
        .args(["simctl", "delete", "unavailable"])
        .status()
        .map_err(|error| Skip::from_io(&error))?;
    if status.success() {
        Ok(())
    } else {
        Err(Skip::Failed(format!("simctl: {status}")))
    }
}

#[cfg(not(all(target_os = "macos", not(feature = "sandbox"))))]
fn delete_unavailable_simulators() -> Result<(), Skip> {
    Err(Skip::Failed("simulators cannot be removed here".to_owned()))
}
