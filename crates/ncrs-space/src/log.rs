use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::checks::Skip;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    pub path: PathBuf,
    pub size: u64,
    /// The bytes freed, or why the entry was skipped.
    pub outcome: Result<u64, Skip>,
    /// When the folder was last used, as the scan saw it.
    pub last_used: Option<SystemTime>,
    /// The in-use check got a real answer: no tool of the folder ran and nothing had it open.
    pub use_checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLog {
    pub started: SystemTime,
    pub finished: SystemTime,
    pub rows: Vec<LogRow>,
}

impl RunLog {
    pub(crate) fn new(started: SystemTime) -> Self {
        Self {
            started,
            finished: started,
            rows: Vec::new(),
        }
    }

    /// The bytes the rows that went freed.
    pub fn freed_bytes(&self) -> u64 {
        self.rows
            .iter()
            .filter_map(|row| row.outcome.as_ref().ok())
            .fold(0_u64, |sum, bytes| sum.saturating_add(*bytes))
    }
}

/// Appends one header line and one tab-separated row per entry to the file of the day
/// (`space-YYYY-MM-DD.log`, UTC) in `dir`.
pub fn append_log(dir: &Path, log: &RunLog) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let seconds = log
        .started
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil_date(seconds / 86_400);
    let file = dir.join(format!("space-{year:04}-{month:02}-{day:02}.log"));
    let mut out = OpenOptions::new().create(true).append(true).open(file)?;
    let finished = log
        .finished
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    writeln!(
        out,
        "# run\tstarted={seconds}\tfinished={finished}\tfreed={}",
        log.freed_bytes()
    )?;
    for row in &log.rows {
        let outcome = match &row.outcome {
            Ok(freed) => format!("freed\t{freed}"),
            Err(skip) => format!("skipped\t{}", describe(skip)),
        };
        writeln!(
            out,
            "{seconds}\t{}\t{}\t{outcome}",
            row.path.display(),
            row.size
        )?;
        if row.outcome.is_ok() {
            let last_used = row
                .last_used
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or_else(|| "-".to_owned(), |since| since.as_secs().to_string());
            let checked = if row.use_checked {
                "use-checked"
            } else {
                "use-unchecked"
            };
            writeln!(
                out,
                "# detail\t{}\t{checked}\t{last_used}",
                row.path.display()
            )?;
        }
    }
    Ok(())
}

/// One run read back from a log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedRun {
    pub started: SystemTime,
    pub finished: SystemTime,
    pub freed: u64,
    pub rows: Vec<LoggedRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedRow {
    pub when: SystemTime,
    pub path: PathBuf,
    pub size: u64,
    /// The bytes freed, or why it was skipped; a reason the log words do not match is `Failed`.
    pub outcome: Result<u64, Skip>,
    pub use_checked: bool,
    pub last_used: Option<SystemTime>,
}

/// The runs of a log file in file order, each with its rows in the order they were written.
/// Lines of another shape are ignored; rows before the first run header make a run with
/// unknown times.
pub fn parse_log(text: &str) -> Vec<LoggedRun> {
    let from_seconds = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
    let mut runs: Vec<LoggedRun> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let first = parts.next().unwrap_or_default();
        if first == "# run" {
            let value = |key: &str, field: Option<&str>| -> u64 {
                field
                    .and_then(|field| field.strip_prefix(key))
                    .and_then(|number| number.parse().ok())
                    .unwrap_or(0)
            };
            let started = value("started=", parts.next());
            let finished = value("finished=", parts.next());
            let freed = value("freed=", parts.next());
            runs.push(LoggedRun {
                started: from_seconds(started),
                finished: from_seconds(finished),
                freed,
                rows: Vec::new(),
            });
        } else if first == "# detail" {
            let path = parts.next().map(PathBuf::from);
            let checked = parts.next() == Some("use-checked");
            let last_used = parts
                .next()
                .and_then(|seconds| seconds.parse().ok())
                .map(from_seconds);
            let row = runs.last_mut().and_then(|run| {
                run.rows
                    .iter_mut()
                    .rev()
                    .find(|row| Some(&row.path) == path.as_ref())
            });
            if let Some(row) = row {
                row.use_checked = checked;
                row.last_used = last_used;
            }
        } else if let Some(row) = parse_row(line, from_seconds) {
            match runs.last_mut() {
                Some(run) => run.rows.push(row),
                None => runs.push(LoggedRun {
                    started: row.when,
                    finished: row.when,
                    freed: 0,
                    rows: vec![row],
                }),
            }
        }
    }
    runs
}

fn parse_row(line: &str, from_seconds: impl Fn(u64) -> SystemTime) -> Option<LoggedRow> {
    let mut parts = line.splitn(5, '\t');
    let seconds: u64 = parts.next()?.parse().ok()?;
    let path = PathBuf::from(parts.next()?);
    let size: u64 = parts.next()?.parse().ok()?;
    let outcome = match parts.next()? {
        "freed" => Ok(parts.next()?.trim().parse().ok()?),
        "skipped" => {
            let words = parts.next()?;
            Err(Skip::from_log_words(words).unwrap_or_else(|| Skip::Failed(words.to_owned())))
        }
        _ => return None,
    };
    Some(LoggedRow {
        when: from_seconds(seconds),
        path,
        size,
        outcome,
        use_checked: false,
        last_used: None,
    })
}

fn describe(skip: &Skip) -> String {
    match skip {
        Skip::ToolRunning(holder) => format!("tool-running {} {}", holder.name, holder.pid),
        Skip::FolderOpen(holder) => format!("folder-open {} {}", holder.name, holder.pid),
        Skip::UseUnknown => "use-unknown".to_owned(),
        Skip::Symlink => "symlink".to_owned(),
        Skip::OtherDrive => "other-drive".to_owned(),
        Skip::Changed => "changed".to_owned(),
        Skip::Gone => "gone".to_owned(),
        Skip::NotGranted => "not-granted".to_owned(),
        Skip::Failed(why) => format!("failed {}", why.replace(['\t', '\n'], " ")),
    }
}

/// Days since 1970-01-01 as (year, month, day), proleptic Gregorian.
fn civil_date(days: u64) -> (i64, u32, u32) {
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    })
    .unwrap_or(1);
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
