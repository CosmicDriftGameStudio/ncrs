//! The flows that file, change and answer: 5 tidy, 6 levels, 7 unknown.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_config::config::LocationTransfer;
use ncrs_filing::sorting::{adopt, suggest_scheme, ReadProblem, RowState};
use ncrs_hermes_chat::{ChipReply, Entry, Part};
use ncrs_index::{BatchId, TargetId};

use super::thread::{chat, chip_labels, has_chip, last_hermes_has_chips, last_text};
use super::{all, env_path, expect, Check, Protocol, READ_LIMIT};
use crate::app::App;
use crate::e2e_tests::Session;
use crate::messages::Message;

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    /// The chat answers the sentence with the way to the dialog; the plan of the
    /// source is then made by the rules. Nothing is filed before `NCRS_EVAL_FILE`.
    pub fn flow_5_tidy(&mut self) {
        self.back_to_start();
        let before_downloads = self.files_below(&self.downloads);
        let before_archive = self.files_below(&self.archive);
        let sentence = self.say("Downloads aufräumen", "Tidy up Downloads");
        let started = Instant::now();
        self.type_sentence(sentence);
        let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
        let chat_image = self.shot("tidy-answer");
        let offers_the_dialog = has_chip(self.app(), |reply| {
            matches!(reply, ChipReply::OpenSort(_) | ChipReply::NamedFolder(_))
        });
        let labels = chip_labels(self.app());

        let rules = self.adopt_suggested_rules();
        let opened = self.open_plan_of_downloads();
        let mid_image = self.plan_in_tab_until_done();
        let done_image = self.shot("filing-done");
        let tally = self.filing_tally();
        println!("[{}] 5 tidy filing tally: {tally}", self.language.code());

        let planned = self.filing_is_planned();
        let moved = before_downloads.saturating_sub(self.files_below(&self.downloads));
        let arrived = self
            .files_below(&self.archive)
            .saturating_sub(before_archive);
        let result = all([
            limit,
            expect(
                offers_the_dialog,
                format!("the chat offers no way to the dialog: {labels}"),
            ),
            rules.as_ref().map(|_| ()).map_err(Clone::clone),
            opened,
            expect(planned, "no finished plan in the tab"),
            expect(
                mid_image.is_some(),
                "no screenshot of the tab while it reads",
            ),
            expect(
                moved == 0 && arrived == 0,
                format!(
                    "{moved} left downloads, {arrived} reached the archive: nothing may be filed"
                ),
            ),
        ]);
        self.record(
            "5 tidy",
            &format!(
                "buttons: {labels} ;; rules: {} ;; tally: {tally}",
                rules.unwrap_or_else(|error| error)
            ),
            started.elapsed().as_secs_f64(),
            &format!(
                "{chat_image}, {}, {done_image}",
                mid_image.unwrap_or_else(|| "no mid shot".to_owned())
            ),
            "the chat offers the dialog; the tab plans every file by the rules; nothing is filed",
            result,
        );
        match std::env::var("NCRS_EVAL_FILE").as_deref() {
            Ok("copy") => {
                self.copy_planned_files();
                if let Some(new_files) = env_path("NCRS_EVAL_CONTINUOUS") {
                    self.continuous_after_tab_run(&new_files, LocationTransfer::Copy);
                }
            }
            Ok("move") => {
                self.move_planned_files();
                if let Some(new_files) = env_path("NCRS_EVAL_CONTINUOUS") {
                    self.continuous_after_tab_run(&new_files, LocationTransfer::Move);
                }
            }
            _ => {}
        }
        if env_path("NCRS_EVAL_CONTINUOUS").is_some()
            && !matches!(
                std::env::var("NCRS_EVAL_FILE").as_deref(),
                Ok("copy" | "move")
            )
        {
            println!("NCRS_EVAL_CONTINUOUS needs NCRS_EVAL_FILE=copy or move, nothing ran");
        }
    }

    /// An index without rules plans nothing: the ones suggested from the archive
    /// are taken, without the model so every run takes the same.
    fn adopt_suggested_rules(&self) -> Result<String, String> {
        let index = self.open_index().ok_or("the index did not open")?;
        let there = index.rules().map_err(|error| error.to_string())?.len();
        drop(index);
        if there > 0 {
            return Ok(format!("{there} of the index"));
        }
        let trash: Arc<dyn crate::fs::Trash> = Arc::new(crate::fs::JournaledTrash::new(
            self.out.join("journal.toml"),
            crate::fs::FolderBackend(self.out.clone()),
        ));
        let access = self
            .app()
            .filing_module_for_test()
            .and_then(|module| module.sorting_access(trash))
            .ok_or("sorting is not ready")?;
        let found = suggest_scheme(&access, None, &TargetId::main(), None, 0, None)
            .map_err(|error| format!("{error:?}"))?;
        let made = adopt(&access, &TargetId::main(), &found.items)
            .map_err(|error| format!("{error:?}"))?;
        Ok(format!("{} adopted", made.len()))
    }

    fn open_plan_of_downloads(&mut self) -> Check {
        let source = self
            .app()
            .sources_for_test()
            .iter()
            .map(|setting| setting.source.clone())
            .find(|source| source.path == self.downloads)
            .ok_or("the downloads are no source")?;
        let Session { app, runtime, .. } = &mut self.session;
        let task = runtime.enter(|| app.open_filing_plan(source, true));
        self.session.drive(task);
        Ok(())
    }

    fn filing_is_planned(&self) -> bool {
        self.app()
            .filing_session_for_test()
            .is_some_and(|session| session.planned && !session.planning)
    }

    /// Where the plan puts its files.
    fn planned_places(&self) -> Option<Vec<PathBuf>> {
        let session = self.app().filing_session_for_test()?;
        Some(
            session
                .rows
                .iter()
                .flat_map(|row| session.places(row).into_owned())
                .filter_map(|(target, relative)| {
                    let index = session.target_index(&target)?;
                    Some(session.targets.get(index)?.path.join(relative))
                })
                .collect(),
        )
    }

    /// `NCRS_EVAL_FILE=copy`: presses copy in the tab and checks the archive and
    /// the source, then undoes the run.
    fn copy_planned_files(&mut self) {
        let started = Instant::now();
        let Some(rows) = self.planned_places() else {
            self.record(
                "5 tidy copy",
                "no plan in the tab",
                started.elapsed().as_secs_f64(),
                "",
                "the plan is copied",
                Err("no plan in the tab".to_owned()),
            );
            return;
        };
        let filed = rows.len();
        let downloads_before = self.files_below(&self.downloads);
        let archive_before = self.files_below(&self.archive);

        let batches = self.hermes_batches();
        let copied = self.press_and_wait(LocationTransfer::Copy);
        let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
        let archive_after = self.files_below(&self.archive);
        let downloads_after = self.files_below(&self.downloads);
        let missing = Self::missing_samples(&rows);
        let copied_image = self.shot("filing-copied");
        let copy_result = all([
            idle,
            expect(copied, "the run in the tab was not over after 300 s"),
            expect(
                archive_after == archive_before + filed,
                format!("the archive went from {archive_before} to {archive_after}, planned {filed}"),
            ),
            expect(
                downloads_after == downloads_before,
                format!(
                    "downloads went from {downloads_before} to {downloads_after}: copy leaves the source"
                ),
            ),
            expect(
                missing.is_empty(),
                format!("sample rows missing in the archive: {}", missing.join(", ")),
            ),
        ]);

        let undo = self.undo_runs_since(&batches);
        let archive_undone = self.files_below(&self.archive);
        let undone_image = self.shot("filing-undone");
        let undo_result = all([
            undo,
            expect(
                archive_undone == archive_before,
                format!(
                    "after undo the archive has {archive_undone}, before it had {archive_before}"
                ),
            ),
        ]);
        let summary = format!(
            "{filed} planned, archive {archive_before} -> {archive_after} -> {archive_undone}, downloads {downloads_before} -> {downloads_after}"
        );
        println!("[{}] 5 tidy copy: {summary}", self.language.code());
        self.record(
            "5 tidy copy",
            &summary,
            started.elapsed().as_secs_f64(),
            &format!("{copied_image}, {undone_image}"),
            "the files are copied; undo removes the copies",
            all([copy_result, undo_result]),
        );
    }

    /// Presses `transfer` in the tab and ticks until the run is over; false
    /// when there was nothing to run or 300 s were not enough.
    fn press_and_wait(&mut self, transfer: LocationTransfer) -> bool {
        self.deliver(Message::FilingView(ncrs_filing_view::Message::File(
            transfer,
        )));
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(300)
            && self
                .app()
                .filing_session_for_test()
                .is_some_and(|session| session.is_filing())
        {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
        }
        self.app()
            .filing_session_for_test()
            .is_some_and(|session| session.outcome.is_some())
    }

    /// The first, middle and last of the planned rows that are not in the
    /// archive.
    fn missing_samples(rows: &[PathBuf]) -> Vec<String> {
        [
            rows.first(),
            rows.get(rows.len().div_euclid(2)),
            rows.last(),
        ]
        .into_iter()
        .flatten()
        .filter(|row| !row.is_file())
        .map(|row| row.display().to_string())
        .collect()
    }

    /// Takes back every run that is not among `before`.
    fn undo_runs_since(&mut self, before: &[BatchId]) -> Check {
        let runs: Vec<BatchId> = self
            .hermes_batches()
            .into_iter()
            .filter(|batch| !before.contains(batch))
            .collect();
        let mut checks = vec![expect(!runs.is_empty(), "no run in the journal to undo")];
        for batch in runs {
            self.deliver(Message::Sorting(ncrs_hermes_ui::SortingMessage::Undo(
                batch.number(),
            )));
            checks.push(self.wait_idle(READ_LIMIT, None).2);
        }
        all(checks)
    }

    /// `NCRS_EVAL_FILE=move`: moves the planned files with the duplicates in
    /// the source trashed, checks archive and source, then undoes the run
    /// and records what came back.
    fn move_planned_files(&mut self) {
        let label = "5 tidy move";
        let started = Instant::now();
        let Some(rows) = self.planned_places() else {
            self.record(
                label,
                "no plan in the tab",
                started.elapsed().as_secs_f64(),
                "",
                "the plan is moved",
                Err("no plan in the tab".to_owned()),
            );
            return;
        };
        let mut moved_sources = BTreeSet::new();
        let mut duplicates = Vec::new();
        if let Some(session) = self.app().filing_session_for_test() {
            for row in &session.rows {
                if let RowState::AlreadyThere { path, .. } = &row.state {
                    moved_sources.insert(row.relative.clone());
                    duplicates.push((row.relative.clone(), path.clone(), file_stamp(path)));
                } else if !session.places(row).is_empty() {
                    moved_sources.insert(row.relative.clone());
                }
            }
        }
        let filed = rows.len();
        let downloads_before = self.files_below(&self.downloads);
        let archive_before = self.files_below(&self.archive);
        let source_before = relative_files(&self.downloads);
        let staying: BTreeSet<_> = source_before.difference(&moved_sources).cloned().collect();

        self.deliver(Message::FilingView(
            ncrs_filing_view::Message::ToggleTrashDupes,
        ));
        let batches = self.hermes_batches();
        let done = self.press_and_wait(LocationTransfer::Move);
        let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
        let archive_after = self.files_below(&self.archive);
        let downloads_after = self.files_below(&self.downloads);
        let source_after = relative_files(&self.downloads);
        let changed: Vec<String> = duplicates
            .iter()
            .filter(|(_, keeper, stamp)| file_stamp(keeper) != *stamp || stamp.is_none())
            .map(|(_, keeper, _)| keeper.display().to_string())
            .collect();
        let missing = Self::missing_samples(&rows);
        let moved_image = self.shot("filing-moved");
        let move_result = all([
            idle,
            expect(done, "the run in the tab was not over after 300 s"),
            expect(
                archive_after == archive_before + filed,
                format!(
                    "the archive went from {archive_before} to {archive_after}, planned {filed}"
                ),
            ),
            expect(
                source_after == staying,
                format!(
                    "downloads hold {:?}, only {:?} may stay",
                    source_after, staying
                ),
            ),
            expect(
                changed.is_empty(),
                format!(
                    "archive files of already-there rows changed or are gone: {}",
                    changed.join(", ")
                ),
            ),
            expect(
                missing.is_empty(),
                format!("sample rows missing in the archive: {}", missing.join(", ")),
            ),
        ]);

        let undo = self.undo_runs_since(&batches);
        let archive_undone = self.files_below(&self.archive);
        let downloads_undone = self.files_below(&self.downloads);
        let back: Vec<String> = duplicates
            .iter()
            .filter(|(from, _, _)| self.downloads.join(from).is_file())
            .map(|(from, _, _)| from.display().to_string())
            .collect();
        let undone_image = self.shot("filing-move-undone");
        let undo_result = all([
            undo,
            expect(
                archive_undone == archive_before,
                format!(
                    "after undo the archive has {archive_undone}, before it had {archive_before}"
                ),
            ),
        ]);
        let summary = format!(
            "{filed} planned, {} already there; archive {archive_before} -> {archive_after} -> {archive_undone}, downloads {downloads_before} -> {downloads_after} -> {downloads_undone}, staying {}; already-there sources back after undo: {} of {} ({})",
            duplicates.len(),
            staying.len(),
            back.len(),
            duplicates.len(),
            back.join(", ")
        );
        println!("[{}] {label}: {summary}", self.language.code());
        self.record(
            label,
            &summary,
            started.elapsed().as_secs_f64(),
            &format!("{moved_image}, {undone_image}"),
            "the planned files move, duplicates are trashed, the archive keeps its files; undo empties the archive of them",
            all([move_result, undo_result]),
        );
    }

    /// Feeds the tab its ticks until the plan is done. The mid shot is the first
    /// plan with files that are still read; without one it is the done plan.
    fn plan_in_tab_until_done(&mut self) -> Option<String> {
        self.app().filing_session_for_test()?;
        let started = Instant::now();
        let mut mid = None;
        while started.elapsed() < READ_LIMIT && !self.filing_is_planned() {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
            let Some(session) = self.app().filing_session_for_test() else {
                break;
            };
            let partial =
                session.planned && session.rows.iter().any(|row| session.is_being_read(row));
            if partial && mid.is_none() {
                mid = Some(self.shot("filing-mid"));
            }
        }
        mid.or_else(|| Some(self.shot("filing-mid-done")))
    }

    /// One line: how many files, and where the plan puts them.
    fn filing_tally(&self) -> String {
        let Some(session) = self.app().filing_session_for_test() else {
            return "no plan".to_owned();
        };
        let count =
            |is: fn(&RowState) -> bool| session.rows.iter().filter(|row| is(&row.state)).count();
        let staying: Vec<String> = session
            .rows
            .iter()
            .filter(|row| {
                !matches!(
                    row.state,
                    RowState::Place(_) | RowState::AlreadyThere { .. }
                )
            })
            .map(|row| format!("{} {:?}", row.relative.display(), row.state))
            .collect();
        format!(
            "{} files, {} with a place ({}), {} already there, {} without a rule, {} without text, {} not read; staying: {}",
            session.rows.len(),
            count(|state| matches!(state, RowState::Place(_))),
            self.hits(),
            count(|state| matches!(state, RowState::AlreadyThere { .. })),
            count(|state| matches!(state, RowState::NoRule)),
            count(|state| matches!(state, RowState::NoText)),
            count(|state| matches!(state, RowState::NotRead(ReadProblem::Unknown))),
            staying.join(", "),
        )
    }

    /// The planned places against the original archive: a file of the same name
    /// and size lies there in the folder it belongs to.
    fn hits(&self) -> String {
        let mut folders: HashMap<(OsString, u64), BTreeSet<PathBuf>> = HashMap::new();
        for relative in relative_files(&self.original) {
            let size = std::fs::metadata(self.original.join(&relative)).map(|meta| meta.len());
            if let (Some(name), Ok(bytes), Some(folder)) =
                (relative.file_name(), size, relative.parent())
            {
                folders
                    .entry((name.to_owned(), bytes))
                    .or_default()
                    .insert(folder.to_path_buf());
            }
        }
        let Some(session) = self.app().filing_session_for_test() else {
            return "no plan".to_owned();
        };
        let (mut right, mut wrong, mut unknown) = (0, 0, 0);
        for row in &session.rows {
            let RowState::Place(place) = &row.state else {
                continue;
            };
            let size = std::fs::metadata(&row.path).map_or(0, |meta| meta.len());
            let belongs = row
                .path
                .file_name()
                .and_then(|name| folders.get(&(name.to_owned(), size)));
            match (belongs, place.relative.parent()) {
                (Some(original_folders), Some(folder)) if original_folders.contains(folder) => {
                    right += 1
                }
                (Some(_), _) => wrong += 1,
                (None, _) => unknown += 1,
            }
        }
        format!("{right} right, {wrong} wrong, {unknown} without a comparison")
    }

    /// The order of the archive is read, changed in words and kept; no file moves.
    pub fn flow_6_levels(&mut self) {
        self.ask_for_folder();
        let before = self.files_below(&self.archive);
        let archive = self.archive.to_string_lossy().into_owned();
        let steps = [
            ("6 read", archive, true),
            (
                "6 year first",
                self.say(
                    "erst ein Ordner für das Jahr und der Dateiname mit Jahr_Monat_Tag",
                    "first a folder for the year and the file name with year_month_day",
                )
                .to_owned(),
                true,
            ),
            (
                "6 by month",
                self.say("ne, lieber nach Monat", "no, rather by month")
                    .to_owned(),
                true,
            ),
            ("6 done", self.say("fertig", "done").to_owned(), false),
        ];
        for (label, sentence, shows_levels) in steps {
            let (seconds, _, limit) = self.say_and_wait(&sentence, None);
            let image = self.shot(&label.replace(' ', "-"));
            let levels = matches!(
                chat(self.app()).thread().last(),
                Some(Entry::Hermes { parts, .. })
                    if parts.iter().any(|part| matches!(part, Part::Levels(_)))
            );
            let met = all([
                limit,
                expect(
                    levels || !shows_levels,
                    format!("no levels are shown; last: {}", last_text(self.app())),
                ),
                expect(
                    last_hermes_has_chips(self.app()),
                    "the answer has no buttons",
                ),
                expect(self.files_below(&self.archive) == before, "files changed"),
            ]);
            self.record(
                label,
                &format!("typed '{sentence}'; buttons: {}", chip_labels(self.app())),
                seconds,
                &image,
                if shows_levels {
                    "the levels as changed, with buttons"
                } else {
                    "the levels are taken, the rule suggestions are offered"
                },
                met,
            );
        }
    }

    pub fn flow_7_unknown(&mut self) {
        for (label, sentence) in [
            (
                "7 suggestion",
                self.say(
                    "Mach mal einen Vorschlag, im Root sollten die Themen stehen",
                    "Make a suggestion, the topics should be at the root",
                ),
            ),
            (
                "7 abilities",
                self.say("Was kannst du?", "What can you do?"),
            ),
        ] {
            self.back_to_start();
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
            let image = self.shot(&label.replace(' ', "-"));
            let met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app()),
                    format!(
                        "no honest answer with buttons; last: {}",
                        last_text(self.app())
                    ),
                ),
            ]);
            self.record(
                label,
                &format!("typed '{sentence}'"),
                started.elapsed().as_secs_f64(),
                &image,
                "an honest answer with buttons",
                met,
            );
        }
    }
}

/// Size and modification time, to tell an untouched file from a changed one.
fn file_stamp(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// The files below `root`, relative to it.
pub(in crate::hermes_protocol) fn relative_files(root: &std::path::Path) -> BTreeSet<PathBuf> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, found: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, found);
            } else if let Ok(relative) = path.strip_prefix(root) {
                found.insert(relative.to_path_buf());
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(root, root, &mut found);
    found
}
