//! The files the user asked Hermes to read again, and how each one ends. The
//! pipeline reports an end as a change of the file after the sequence number
//! it had when the request was made; this keeps the pure bookkeeping out of
//! `App`.

use std::path::{Path, PathBuf};

use ncrs_pipeline::{ChangeOutcome, Snapshot};

use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};

/// How reading a file again ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The rules or the AI placed it, or it waits in the review list: which
    /// one only the index knows.
    Finished,
    Unchanged,
    Failed(Option<String>),
    /// The pipeline was rebuilt or lost track; nothing more will be reported.
    Lost,
}

#[derive(Debug, Default)]
pub struct ReadingAgain {
    /// Oldest first, with the change sequence of the snapshot at the request.
    open: Vec<(PathBuf, u64)>,
}

impl ReadingAgain {
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// `since` is `changed_sequence` of the snapshot the request was made at.
    /// A second request for the same file replaces the first.
    pub fn ask(&mut self, path: PathBuf, since: u64) {
        self.open.retain(|(asked, _)| *asked != path);
        self.open.push((path, since));
    }

    /// The status bar text: the file asked last, and how many others wait.
    pub fn line(&self, lang: Language) -> Option<String> {
        let (path, _) = self.open.last()?;
        let name = file_name(path);
        let others = self.open.len() - 1;
        Some(if others == 0 {
            fill_template(lang.text(Msg::StatusReadingAgain), &[("{name}", &name)])
        } else {
            fill_template(
                lang.text(Msg::StatusReadingAgainMore),
                &[("{name}", &name), ("{count}", &others.to_string())],
            )
        })
    }

    /// Takes the files out whose end the snapshot reports. A step in the
    /// middle of the pipeline is no end. When the snapshot lost track of
    /// changes, everything still open ends as [`Ending::Lost`].
    pub fn settle(&mut self, snapshot: &Snapshot) -> Vec<(PathBuf, Ending)> {
        let mut ended = Vec::new();
        let mut still_open = Vec::new();
        for (path, since) in std::mem::take(&mut self.open) {
            let ending = match snapshot.changes_since(since) {
                None => Some(Ending::Lost),
                Some(mut changes) => changes
                    .find(|change| change.path == path && change.outcome != ChangeOutcome::Advanced)
                    .map(|change| match change.outcome {
                        ChangeOutcome::Unchanged => Ending::Unchanged,
                        ChangeOutcome::Failed => Ending::Failed(reason_of(snapshot, &path)),
                        ChangeOutcome::Advanced | ChangeOutcome::Finished => Ending::Finished,
                    }),
            };
            match ending {
                Some(found) => ended.push((path, found)),
                None => still_open.push((path, since)),
            }
        }
        self.open = still_open;
        ended
    }

    /// The pipeline was replaced: its sequence numbers start over.
    pub fn give_up(&mut self) -> Vec<(PathBuf, Ending)> {
        std::mem::take(&mut self.open)
            .into_iter()
            .map(|(path, _)| (path, Ending::Lost))
            .collect()
    }
}

fn reason_of(snapshot: &Snapshot, path: &Path) -> Option<String> {
    snapshot
        .errors
        .iter()
        .rev()
        .find(|error| error.file == path)
        .map(|error| error.message.clone())
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// The notice for an end. `waits_for_review` is what the index says about a
/// [`Ending::Finished`] file; `None` when it could not be asked.
pub fn notice(
    lang: Language,
    path: &Path,
    ending: &Ending,
    waits_for_review: Option<bool>,
) -> String {
    let name = file_name(path);
    let fill = |msg: Msg| fill_template(lang.text(msg), &[("{name}", &name)]);
    match ending {
        Ending::Finished => match waits_for_review {
            Some(true) => fill(Msg::StatusAgainReview),
            Some(false) => fill(Msg::StatusAgainFiled),
            None => fill(Msg::StatusAgainDone),
        },
        Ending::Unchanged => fill(Msg::StatusAgainUnchanged),
        Ending::Lost => fill(Msg::StatusAgainDone),
        Ending::Failed(reason) => fill_template(
            lang.text(Msg::StatusAgainFailed),
            &[
                ("{name}", &name),
                ("{reason}", reason.as_deref().unwrap_or("?")),
            ],
        ),
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use ncrs_pipeline::{ChangedPath, ErrorEntry, Task};

    use super::*;

    fn change(sequence: u64, path: &str, outcome: ChangeOutcome) -> ChangedPath {
        ChangedPath {
            sequence,
            path: PathBuf::from(path),
            outcome,
        }
    }

    fn snapshot(changes: Vec<ChangedPath>) -> Snapshot {
        Snapshot {
            changed_sequence: changes.last().map_or(0, |last| last.sequence),
            changed_paths: changes,
            ..Snapshot::off()
        }
    }

    #[test]
    fn the_status_bar_names_the_file_asked_last_and_counts_the_others() {
        let mut open = ReadingAgain::default();
        assert_eq!(open.line(Language::English), None);
        open.ask(PathBuf::from("/in/a.pdf"), 0);
        assert_eq!(
            open.line(Language::English).as_deref(),
            Some("Reading a.pdf again …")
        );
        open.ask(PathBuf::from("/in/b.pdf"), 0);
        assert_eq!(
            open.line(Language::German).as_deref(),
            Some("liest b.pdf neu … (+1)")
        );
    }

    #[test]
    fn a_step_in_the_middle_does_not_end_the_reading() {
        let mut open = ReadingAgain::default();
        open.ask(PathBuf::from("/in/a.pdf"), 3);

        let ended = open.settle(&snapshot(vec![
            change(3, "/in/a.pdf", ChangeOutcome::Finished),
            change(4, "/in/a.pdf", ChangeOutcome::Advanced),
        ]));
        assert!(ended.is_empty(), "the change at 3 came before the request");
        assert!(!open.is_empty());

        let ended_now = open.settle(&snapshot(vec![
            change(4, "/in/a.pdf", ChangeOutcome::Advanced),
            change(5, "/in/a.pdf", ChangeOutcome::Finished),
        ]));
        assert_eq!(
            ended_now,
            vec![(PathBuf::from("/in/a.pdf"), Ending::Finished)]
        );
        assert!(open.is_empty());
    }

    #[test]
    fn a_failed_file_carries_the_reason_of_its_error() {
        let mut open = ReadingAgain::default();
        open.ask(PathBuf::from("/in/a.pdf"), 0);
        let mut seen = snapshot(vec![change(1, "/in/a.pdf", ChangeOutcome::Failed)]);
        seen.errors.push(ErrorEntry {
            file: PathBuf::from("/in/a.pdf"),
            task: Task::Scan,
            message: "no text".to_owned(),
            at: SystemTime::UNIX_EPOCH,
            sequence: 1,
        });

        let ended = open.settle(&seen);

        assert_eq!(
            ended,
            vec![(
                PathBuf::from("/in/a.pdf"),
                Ending::Failed(Some("no text".to_owned()))
            )]
        );
        assert_eq!(
            notice(Language::German, Path::new("/in/a.pdf"), &ended[0].1, None),
            "a.pdf: nicht gelesen: no text"
        );
    }

    #[test]
    fn the_notice_tells_whether_a_finished_file_waits_for_review() {
        let path = Path::new("/in/a.pdf");
        assert_eq!(
            notice(Language::English, path, &Ending::Finished, Some(true)),
            "a.pdf: waiting for your review"
        );
        assert_eq!(
            notice(Language::English, path, &Ending::Finished, Some(false)),
            "a.pdf: filed"
        );
        assert_eq!(
            notice(Language::German, path, &Ending::Unchanged, None),
            "a.pdf: unverändert, der Inhalt ergibt nichts Neues"
        );
    }

    #[test]
    fn changes_the_snapshot_lost_end_every_open_reading() {
        let mut open = ReadingAgain::default();
        open.ask(PathBuf::from("/in/a.pdf"), 7);
        let restarted = snapshot(vec![change(1, "/in/b.pdf", ChangeOutcome::Finished)]);

        let ended = open.settle(&restarted);

        assert_eq!(ended, vec![(PathBuf::from("/in/a.pdf"), Ending::Lost)]);
    }
}
