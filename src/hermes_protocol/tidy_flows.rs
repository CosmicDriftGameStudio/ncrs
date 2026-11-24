//! The flows that file, change and answer: 5 tidy, 6 structure, 7 unknown.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ncrs_hermes_chat::{ChipReply, Step};
use ncrs_i18n::Msg;

use super::thread::{chat, chip_labels, has_chip, last_hermes_has_chips, last_text};
use super::{all, env_path, expect, Check, Protocol, READ_LIMIT};
use crate::app::App;
use crate::messages::Message;

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    /// Whether the newest bubble ends like the text of `msg`: the words after
    /// its last placeholder.
    fn last_says(&self, msg: Msg) -> bool {
        let template = self.language.text(msg);
        let fixed = template.rsplit('}').next().unwrap_or(template);
        last_text(self.app()).contains(fixed)
    }

    /// The chat asks for the folders and hands over to the tab; the tab plans
    /// live. Nothing is filed: the run never presses copy or move.
    pub fn flow_5_tidy(&mut self) {
        self.back_to_start();
        let before_downloads = self.files_below(&self.downloads);
        let before_archive = self.files_below(&self.archive);
        let sentence = self.say("Downloads aufräumen", "Tidy up Downloads");
        let started = Instant::now();
        self.type_sentence(sentence);
        let (_, _, mut limit) = self.wait_idle(READ_LIMIT, None);
        let mut questions = Vec::new();
        let archive = self.archive.clone();
        let continuous = env_path("NCRS_EVAL_CONTINUOUS").is_some();
        for round in 1..=20 {
            let labels = chip_labels(self.app());
            questions.push(format!(
                "round {round}: {} => {labels}",
                last_text(self.app()).chars().take(80).collect::<String>()
            ));
            if self.app().filing_session_for_test().is_some() {
                break;
            }
            let first_press = self.press_chip(|reply| match reply {
                ChipReply::Folder(path) | ChipReply::NamedFolder(path) => *path == archive,
                _ => false,
            });
            let pressed = first_press
                .or_else(|failed| {
                    // "Always" files the source once now and then by itself.
                    if continuous {
                        self.press_chip(|reply| *reply == ChipReply::Always)
                    } else {
                        Err(failed)
                    }
                })
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::Once))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::ConfirmTidy))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::PhotosNo))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::KeepYes));
            if pressed.is_err() {
                break;
            }
            let (_, _, again) = self.wait_idle(READ_LIMIT, None);
            limit = again;
            if limit.is_err() {
                break;
            }
        }
        let handed_over = self.last_says(Msg::HermesChatHandover);
        // The tab is the shown page now; the chat is the next tab after it.
        self.deliver(Message::NextTab);
        let chat_image = self.shot("tidy-handover");
        self.deliver(Message::PreviousTab);

        let mid_image = self.plan_in_tab_until_done();
        let done_image = self.shot("filing-done");
        let tally = self.filing_tally();
        println!("[{}] 5 tidy filing tally: {tally}", self.language.code());

        let planned = self
            .app()
            .filing_session_for_test()
            .is_some_and(|session| session.plan_is_done());
        let moved = before_downloads.saturating_sub(self.files_below(&self.downloads));
        let arrived = self
            .files_below(&self.archive)
            .saturating_sub(before_archive);
        let result = all([
            limit,
            expect(handed_over, "the chat did not say it hands over to the tab"),
            expect(planned, "the tab did not finish planning"),
            expect(
                mid_image.is_some(),
                "no screenshot of the tab at about half decided",
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
            &format!("questions: {} ;; tally: {tally}", questions.join(" ;; ")),
            started.elapsed().as_secs_f64(),
            &format!(
                "{chat_image}, {}, {done_image}",
                mid_image.unwrap_or_else(|| "no mid shot".to_owned())
            ),
            "the chat hands over; the tab plans every file; nothing is filed",
            result,
        );
        match std::env::var("NCRS_EVAL_FILE").as_deref() {
            Ok("copy") => {
                self.copy_planned_files();
                if let Some(new_files) = env_path("NCRS_EVAL_CONTINUOUS") {
                    self.continuous_after_copy(&new_files);
                }
            }
            Ok("move") => self.move_planned_files(false),
            Ok("move-sure") => self.move_planned_files(true),
            _ => {}
        }
        if continuous && std::env::var("NCRS_EVAL_FILE").as_deref() != Ok("copy") {
            println!("NCRS_EVAL_CONTINUOUS needs NCRS_EVAL_FILE=copy, nothing ran");
        }
    }

    /// `NCRS_EVAL_FILE=copy`: presses copy in the tab and checks the archive,
    /// the source, the index and the chat, then undoes the run in the chat.
    fn copy_planned_files(&mut self) {
        use ncrs_filing::chat_work::LiveState;
        let started = Instant::now();
        let Some(plan) = self
            .app()
            .filing_session_for_test()
            .and_then(|session| session.shown_plan())
        else {
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
        let rows: Vec<std::path::PathBuf> = plan
            .files
            .iter()
            .filter_map(|file| match &file.state {
                LiveState::Filed { folder, name, .. } => Some(folder.join(name)),
                _ => None,
            })
            .collect();
        let unreadable: Vec<ncrs_index::ContentHash> = plan
            .files
            .iter()
            .filter(|file| file.state == LiveState::Unreadable)
            .filter_map(|file| file.hash)
            .collect();
        let filed = rows.len();
        let downloads_before = self.files_below(&self.downloads);
        let archive_before = self.files_below(&self.archive);

        let copied = self.press_and_wait(ncrs_index::Transfer::Copy);
        let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
        let archive_after = self.files_below(&self.archive);
        let downloads_after = self.files_below(&self.downloads);
        let missing = self.missing_samples(&rows);
        let tag = self.language.text(Msg::FilingViewTagUnreadable);
        let tagged = self.own_tagged_count(&unreadable, tag);
        let says_filed = self.last_says(Msg::HermesChatFiledInTabCopy);
        let copied_image = self.shot("filing-copied");
        let copy_result = all([
            idle,
            expect(copied, "the tab still had its session after 300 s"),
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
            expect(
                tagged == Some(unreadable.len()),
                format!(
                    "{tagged:?} of {} unreadable files carry the tag (own)",
                    unreadable.len()
                ),
            ),
            expect(
                says_filed,
                format!(
                    "the chat's last message is not the filed text: {}",
                    last_text(self.app())
                ),
            ),
        ]);

        let (undo, undo_idle) = self.undo_in_chat();
        let archive_undone = self.files_below(&self.archive);
        let undone_image = self.shot("filing-undone");
        let undo_result = all([
            undo,
            undo_idle,
            expect(
                archive_undone == archive_before,
                format!(
                    "after undo the archive has {archive_undone}, before it had {archive_before}"
                ),
            ),
        ]);
        let summary = format!(
            "{filed} planned, archive {archive_before} -> {archive_after} -> {archive_undone}, downloads {downloads_before} -> {downloads_after}, {} of {} unreadable tagged",
            tagged.map_or_else(|| "?".to_owned(), |count| count.to_string()),
            unreadable.len()
        );
        println!("[{}] 5 tidy copy: {summary}", self.language.code());
        self.record(
            "5 tidy copy",
            &summary,
            started.elapsed().as_secs_f64(),
            &format!("{copied_image}, {undone_image}"),
            "the files are copied, tagged and announced in the chat; undo removes the copies",
            all([copy_result, undo_result]),
        );
    }

    /// Presses `transfer` in the tab and ticks until its session is gone;
    /// false when 300 s were not enough.
    fn press_and_wait(&mut self, transfer: ncrs_index::Transfer) -> bool {
        self.press_message_and_wait(ncrs_filing_view::Message::File(transfer))
    }

    fn press_message_and_wait(&mut self, press: ncrs_filing_view::Message) -> bool {
        self.deliver(Message::FilingView(press));
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(300)
            && self.app().filing_session_for_test().is_some()
        {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
        }
        self.app().filing_session_for_test().is_none()
    }

    /// The first, middle and last of the planned rows that are not in the
    /// archive.
    fn missing_samples(&self, rows: &[std::path::PathBuf]) -> Vec<String> {
        [
            rows.first(),
            rows.get(rows.len().div_euclid(2)),
            rows.last(),
        ]
        .into_iter()
        .flatten()
        .filter(|row| !self.archive.join(row).is_file())
        .map(|row| row.display().to_string())
        .collect()
    }

    /// Presses the chat's undo chip and waits for the chat.
    fn undo_in_chat(&mut self) -> (Check, Check) {
        let undo = self.press_chip(|reply| *reply == ChipReply::PutBack);
        let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
        (undo, idle)
    }

    /// `NCRS_EVAL_FILE=move`: moves the planned files with the duplicates in
    /// the source trashed, checks archive and source, then undoes the run in
    /// the chat and records what came back.
    /// `sure_only`: presses "Move only the sure ones"; the unsure files stay.
    fn move_planned_files(&mut self, sure_only: bool) {
        use ncrs_filing::chat_work::LiveState;
        let label = if sure_only {
            "5 tidy move sure"
        } else {
            "5 tidy move"
        };
        let started = Instant::now();
        let Some(plan) = self
            .app()
            .filing_session_for_test()
            .and_then(|session| session.shown_plan())
        else {
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
        let mut rows = Vec::new();
        let mut moved_sources = BTreeSet::new();
        let mut duplicates = Vec::new();
        for file in &plan.files {
            match &file.state {
                LiveState::Filed { unsure: true, .. } if sure_only => {}
                LiveState::Filed { folder, name, .. } => {
                    rows.push(folder.join(name));
                    moved_sources.insert(file.from.clone());
                }
                LiveState::AlreadyThere { at } => {
                    moved_sources.insert(file.from.clone());
                    let keeper = self.archive.join(at);
                    duplicates.push((file.from.clone(), keeper.clone(), file_stamp(&keeper)));
                }
                _ => {}
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
        let done = if sure_only {
            self.shot("filing-unsure-marked");
            self.press_message_and_wait(ncrs_filing_view::Message::FileSure)
        } else {
            self.press_and_wait(ncrs_index::Transfer::Move)
        };
        let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
        let archive_after = self.files_below(&self.archive);
        let downloads_after = self.files_below(&self.downloads);
        let source_after = relative_files(&self.downloads);
        let changed: Vec<String> = duplicates
            .iter()
            .filter(|(_, keeper, stamp)| file_stamp(keeper) != *stamp || stamp.is_none())
            .map(|(_, keeper, _)| keeper.display().to_string())
            .collect();
        let missing = self.missing_samples(&rows);
        let moved_image = self.shot("filing-moved");
        let move_result = all([
            idle,
            expect(done, "the tab still had its session after 300 s"),
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

        let (undo, undo_idle) = self.undo_in_chat();
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
            undo_idle,
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

    /// How many of the files with these hashes carry the tag as the user's
    /// own; `None` when the index does not open.
    fn own_tagged_count(&self, hashes: &[ncrs_index::ContentHash], tag: &str) -> Option<usize> {
        let index =
            ncrs_index::Index::open(&self.index_path, &ncrs_index::InMemoryKeyStore::empty())
                .ok()?;
        Some(
            hashes
                .iter()
                .filter(|hash| {
                    index
                        .document_by_hash(hash)
                        .ok()
                        .flatten()
                        .is_some_and(|document| document.fields.tags.contains(tag))
                })
                .count(),
        )
    }

    /// Feeds the tab its ticks and reads its snapshot between pumps until the
    /// plan is done. The mid shot is the first snapshot with 30 % or more of
    /// the files decided (neither waiting nor looked at) and not all of them;
    /// when the plan jumps past that, the first partial snapshot stands in,
    /// and with none the shot is taken once the plan is done. The case is
    /// printed.
    fn plan_in_tab_until_done(&mut self) -> Option<String> {
        use ncrs_filing::chat_work::LiveState;
        let started = Instant::now();
        let mut mid = None;
        let mut first_partial = None;
        while started.elapsed() < READ_LIMIT {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
            let Some(session) = self.app().filing_session_for_test() else {
                break;
            };
            let done = session.plan_is_done();
            let (decided, all_files) = session.shown_plan().map_or((0, 0), |plan| {
                let decided = plan
                    .files
                    .iter()
                    .filter(|file| !matches!(file.state, LiveState::Waiting | LiveState::Looking))
                    .count();
                (decided, plan.files.len())
            });
            if done {
                break;
            }
            let partial = decided > 0 && decided < all_files;
            if partial && decided * 10 >= all_files * 3 {
                mid = Some(self.shot("filing-mid"));
                println!(
                    "[{}] 5 tidy mid shot: {decided} of {all_files} decided",
                    self.language.code()
                );
                break;
            }
            if partial && first_partial.is_none() {
                first_partial = Some(self.shot("filing-mid-early"));
            }
        }
        self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
        if mid.is_none() {
            let case = if first_partial.is_some() {
                "the plan jumped past 30 %: the first partial snapshot stands in"
            } else {
                "the plan jumped from nothing to done: the shot is of the done plan"
            };
            println!("[{}] 5 tidy mid shot: {case}", self.language.code());
            mid = first_partial.or_else(|| Some(self.shot("filing-mid-done")));
        }
        while started.elapsed() < READ_LIMIT
            && self
                .app()
                .filing_session_for_test()
                .is_some_and(|session| !session.plan_is_done())
        {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
        }
        mid
    }

    /// One line: how many files, and where the plan puts them.
    fn filing_tally(&self) -> String {
        use ncrs_filing::chat_work::LiveState;
        let Some(plan) = self
            .app()
            .filing_session_for_test()
            .and_then(|session| session.shown_plan())
        else {
            return "no plan".to_owned();
        };
        let count = |is: fn(&LiveState) -> bool| plan.files.iter().filter(|f| is(&f.state)).count();
        let staying: Vec<String> = plan
            .files
            .iter()
            .filter(|file| matches!(file.state, LiveState::NoFolder | LiveState::Unreadable))
            .map(|file| format!("{} {:?}", file.from.display(), file.state))
            .collect();
        format!(
            "{} files, {} filed ({} unsure), {} already there, {} without folder, {} unreadable; staying: {}",
            plan.files.len(),
            count(|state| matches!(state, LiveState::Filed { .. })),
            count(|state| matches!(state, LiveState::Filed { unsure: true, .. })),
            count(|state| matches!(state, LiveState::AlreadyThere { .. })),
            count(|state| matches!(state, LiveState::NoFolder)),
            count(|state| matches!(state, LiveState::Unreadable)),
            staying.join(", "),
        )
    }

    pub fn flow_6_structure(&mut self) {
        for (label, sentence) in [
            (
                "6 bank",
                self.say(
                    "Kontoauszüge der Sparkasse kommen nach Bank, dann Jahr und Monat",
                    "Statements from the Sparkasse go to Bank, then year and month",
                ),
            ),
            (
                "6 insurance",
                self.say(
                    "Versicherungspost nach Versicherung und Absender",
                    "Insurance mail to Versicherung and sender",
                ),
            ),
        ] {
            self.back_to_start();
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
            let image = self.shot(&format!("{}-proposal", label.replace(' ', "-")));
            let proposed = has_chip(self.app(), |reply| *reply == ChipReply::RuleCreate);
            let asked = has_chip(self.app(), |reply| matches!(reply, ChipReply::RuleWhat(_)));
            let met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app()),
                    "no proposal or question with buttons",
                ),
                expect(
                    proposed || asked,
                    format!(
                        "neither a proposed rule nor the question what goes in; last: {}",
                        last_text(self.app())
                    ),
                ),
            ]);
            // The rule is only proposed here: nothing is saved without a click
            // on "Create rule", and the protocol never makes it.
            if has_chip(self.app(), |reply| *reply == ChipReply::Cancel) {
                let _ = self.press_chip(|reply| *reply == ChipReply::Cancel);
            }
            self.record(
                label,
                &format!("typed '{sentence}'; labels: {}", chip_labels(self.app())),
                started.elapsed().as_secs_f64(),
                &image,
                "a rule proposal with levels and buttons, or the question what goes in",
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
                    last_hermes_has_chips(self.app())
                        && !matches!(chat(self.app()).step, Step::Role)
                        && !self.last_says(Msg::HermesChatChangeAreaAsk),
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
pub(in crate::hermes_protocol) fn relative_files(
    root: &std::path::Path,
) -> BTreeSet<std::path::PathBuf> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        found: &mut BTreeSet<std::path::PathBuf>,
    ) {
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
