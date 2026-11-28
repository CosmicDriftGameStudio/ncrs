//! The words of rows, checks and results, shared by the list, the detail pane and the result.

use std::time::{Duration, SystemTime};

use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_space::decide::DecideKind;
use ncrs_space::{cargo_project_name, catalog_msgs, CatalogId, Group, Skip};
use ncrs_ui_kit::format::size_shown;
use ncrs_ui_kit::size_row::CheckTone;

use crate::rows::{DisplayRow, RowKind};

pub fn fill_size(msg: Msg, bytes: u64, lang: Language) -> String {
    fill_template(lang.text(msg), &[("{size}", &size_shown(bytes, lang))])
}

pub fn fill_count(msg: Msg, count: usize, lang: Language) -> String {
    fill_template(lang.text(msg), &[("{count}", &count.to_string())])
}

pub fn fill_count_size(msg: Msg, count: usize, bytes: u64, lang: Language) -> String {
    fill_template(
        lang.text(msg),
        &[
            ("{count}", &count.to_string()),
            ("{size}", &size_shown(bytes, lang)),
        ],
    )
}

/// `one` for exactly one thing, else `other`; both take `{count}` and `{size}`.
pub fn fill_plural(one: Msg, other: Msg, count: usize, bytes: u64, lang: Language) -> String {
    fill_count_size(if count == 1 { one } else { other }, count, bytes, lang)
}

/// What the row is called: the tool for a catalog folder, the branch for a worktree, the folder otherwise.
pub fn row_name(row: &DisplayRow, lang: Language) -> String {
    match &row.kind {
        RowKind::Catalog(id) => {
            let (name, _) = catalog_msgs(*id);
            let shown = match id {
                CatalogId::AppCache(app) => (*app).to_owned(),
                CatalogId::CargoTarget => {
                    cargo_project_name(&row.path).unwrap_or_else(|| row.name())
                }
                _ => row.name(),
            };
            fill_template(lang.text(name), &[("{name}", &shown)])
        }
        RowKind::Downloads => lang.text(Msg::SpaceDecideDownloads).to_owned(),
        RowKind::Worktree(_) | RowKind::BigFolder | RowKind::Place(_) => row.name(),
    }
}

/// The reason and the check of a folder of a known kind.
fn place_msgs(kind: &DecideKind) -> (Msg, Msg) {
    match kind {
        DecideKind::SessionLogs => (
            Msg::SpaceDecideSessionLogsWhy,
            Msg::SpaceDecidePlaceSessionLogsCheck,
        ),
        DecideKind::Simulators => (
            Msg::SpaceDecideSimulatorsWhy,
            Msg::SpaceDecidePlaceSimulatorsCheck,
        ),
        DecideKind::Backups => (
            Msg::SpaceDecideBackupsWhy,
            Msg::SpaceDecidePlaceBackupsCheck,
        ),
        DecideKind::ModelCache => (
            Msg::SpaceDecideModelCacheWhy,
            Msg::SpaceDecidePlaceModelCacheCheck,
        ),
        _ => (Msg::SpaceDecideOtherWhy, Msg::SpaceDecideOtherCheck),
    }
}

/// The short reason in the row. A folder in use says who holds it.
pub fn row_reason(row: &DisplayRow, lang: Language) -> String {
    match &row.kind {
        RowKind::Catalog(id) => lang.text(catalog_msgs(*id).1).to_owned(),
        RowKind::Worktree(_) => lang.text(Msg::SpaceWorktreeRemoval).to_owned(),
        RowKind::Downloads => lang.text(Msg::SpaceDecideDownloadsWhy).to_owned(),
        RowKind::Place(kind) => lang.text(place_msgs(kind).0).to_owned(),
        RowKind::BigFolder => fill_template(
            lang.text(Msg::SpaceDecideOtherWhy),
            &[("{name}", &row.name())],
        ),
    }
}

/// The check column of the list: what was or was not found out about the row.
pub fn row_check(
    row: &DisplayRow,
    processes_known: bool,
    lang: Language,
) -> Option<(CheckTone, String)> {
    if row.group == Group::InUse {
        return Some((CheckTone::Blocked, in_use_reason(row, lang)));
    }
    match &row.kind {
        RowKind::Catalog(_) | RowKind::Worktree(_) if !processes_known => None,
        RowKind::Catalog(_) | RowKind::Worktree(_) => row_checks(row, true, lang)
            .into_iter()
            .next()
            .map(|check| (CheckTone::Ok, check)),
        RowKind::Downloads | RowKind::BigFolder | RowKind::Place(_) => row_checks(row, true, lang)
            .into_iter()
            .next()
            .map(|check| (CheckTone::Decide, check)),
    }
}

fn in_use_reason(row: &DisplayRow, lang: Language) -> String {
    match row.held_by.first() {
        Some(holder) => fill_template(
            lang.text(Msg::SpaceCheckToolRunning),
            &[("{tool}", &holder.name)],
        ),
        None => lang.text(Msg::SpaceCheckUnknownUse).to_owned(),
    }
}

/// The checks the detail pane lists for a row; what could not be checked says so.
pub fn row_checks(row: &DisplayRow, processes_known: bool, lang: Language) -> Vec<String> {
    if row.group == Group::InUse {
        return vec![in_use_reason(row, lang)];
    }
    match &row.kind {
        RowKind::Catalog(id) => {
            if !processes_known {
                return vec![lang.text(Msg::SpaceCheckUnknownUse).to_owned()];
            }
            let mut checks = Vec::new();
            if *id == CatalogId::DerivedData {
                checks.push(lang.text(Msg::SpaceCheckNoBuild).to_owned());
            } else if !id.tools().is_empty() {
                checks.push(fill_template(
                    lang.text(Msg::SpaceCheckNoTool),
                    &[("{tool}", &id.tools().join("/"))],
                ));
            }
            if *id == CatalogId::UnavailableSimulators {
                checks.push(lang.text(Msg::SpaceCheckReportedUnavailable).to_owned());
            } else {
                checks.push(lang.text(Msg::SpaceCheckNoOpenFolder).to_owned());
            }
            checks
        }
        RowKind::Worktree(_) => vec![lang.text(Msg::SpaceWorktreeCheck).to_owned()],
        RowKind::Downloads => vec![lang.text(Msg::SpaceDecideDownloadsCheck).to_owned()],
        RowKind::Place(kind) => vec![lang.text(place_msgs(kind).1).to_owned()],
        RowKind::BigFolder => vec![lang.text(Msg::SpaceDecideOtherCheck).to_owned()],
    }
}

/// The tools that write a catalog folder, for "made by".
pub fn made_by(row: &DisplayRow) -> String {
    match &row.kind {
        RowKind::Catalog(id) => id.regenerated_by().to_owned(),
        RowKind::Worktree(_) | RowKind::Downloads | RowKind::BigFolder | RowKind::Place(_) => {
            String::new()
        }
    }
}

/// What happens to the row afterwards.
pub fn afterwards(row: &DisplayRow, lang: Language) -> String {
    match (&row.kind, row.group) {
        (RowKind::Catalog(_), Group::Safe) => lang.text(Msg::SpaceDetailPermanent).to_owned(),
        (RowKind::Worktree(_), Group::Worktrees) => lang.text(Msg::SpaceWorktreeRemoval).to_owned(),
        _ => String::new(),
    }
}

pub fn group_title(group: Group, lang: Language) -> String {
    lang.text(match group {
        Group::Safe => Msg::SpaceGroupSafe,
        Group::Worktrees => Msg::SpaceGroupWorktrees,
        Group::Decide => Msg::SpaceGroupDecide,
        Group::InUse => Msg::SpaceGroupInUse,
    })
    .to_owned()
}

pub fn skip_text(skip: &Skip, lang: Language) -> String {
    match skip {
        Skip::ToolRunning(holder) => fill_template(
            lang.text(Msg::SpaceSkipToolRunning),
            &[("{tool}", &holder.name)],
        ),
        Skip::FolderOpen(holder) => fill_template(
            lang.text(Msg::SpaceSkipFolderOpen),
            &[("{holder}", &holder.name)],
        ),
        Skip::UseUnknown => lang.text(Msg::SpaceSkipUseUnknown).to_owned(),
        Skip::Symlink => lang.text(Msg::SpaceSkipSymlink).to_owned(),
        Skip::OtherDrive => lang.text(Msg::SpaceSkipOtherDrive).to_owned(),
        Skip::Changed => lang.text(Msg::SpaceSkipChanged).to_owned(),
        Skip::Gone => lang.text(Msg::SpaceSkipGone).to_owned(),
        Skip::NotGranted => lang.text(Msg::SpaceSkipNotGranted).to_owned(),
        Skip::Failed(reason) => {
            fill_template(lang.text(Msg::SpaceSkipFailed), &[("{reason}", reason)])
        }
    }
}

/// How long ago, in the short units every language shares: `12 s`, `5 min`, `3 h`, `2 d`.
pub fn ago_text(now: SystemTime, then: SystemTime) -> String {
    let seconds = now.duration_since(then).map_or(0, |ago| ago.as_secs());
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3599 => format!("{} min", seconds / 60),
        3600..=86_399 => format!("{} h", seconds / 3600),
        _ => format!("{} d", seconds / 86_400),
    }
}

pub fn took_text(took: Duration) -> String {
    if took.as_secs() >= 60 {
        format!("{} min {} s", took.as_secs() / 60, took.as_secs() % 60)
    } else {
        format!("{:.1} s", took.as_secs_f64())
    }
}

/// The share of the drive a size is, in whole percent, at least 1 for anything there.
pub fn percent_of(bytes: u64, total: u64) -> u64 {
    if total == 0 || bytes == 0 {
        return 0;
    }
    (bytes.saturating_mul(100) / total).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_at_least_one_percent_unless_nothing_is_there() {
        assert_eq!(percent_of(0, 100), 0);
        assert_eq!(percent_of(1, 1000), 1);
        assert_eq!(percent_of(50, 100), 50);
        assert_eq!(percent_of(5, 0), 0);
    }

    #[test]
    fn the_age_steps_through_the_units() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |seconds| ago_text(now, now - Duration::from_secs(seconds));
        assert_eq!(ago(5), "5 s");
        assert_eq!(ago(300), "5 min");
        assert_eq!(ago(7200), "2 h");
        assert_eq!(ago(172_800), "2 d");
    }
}
