//! `NCRS_EVAL_CONTINUOUS=<folder with new files>`: after the chat tidied the
//! source with "Always" and copied it in the tab, new files arrive in the
//! source, the app files the sure ones by itself, and the run is put back.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ncrs_config::config::LocationTransfer;
use ncrs_index::{BatchId, BatchKind, DocumentStatus, InMemoryKeyStore, Index, JournalState};
use ncrs_intake::LocationMode;

use super::quiet_window::QuietWindow;
use super::scratch::copy_tree;
use super::tidy_flows::relative_files;
use super::{all, expect, Check, Protocol, READ_LIMIT};
use crate::app::App;
use crate::messages::{
    CommandPaletteMessage, FilingStatusMessage, MenuMessage, Message, PanelMessage, PanelSide,
    SettingsFeature, SettingsMessage, ShownTab,
};

/// The window Marc looks at the pictures in.
const WIDE: iced::Size = iced::Size::new(1740.0, 1130.0);
/// How long the pipeline may be idle without a background run before the wait
/// ends; with a run it is the time the run's last report gets to be followed.
const IDLE_WITHOUT_RUN: Duration = Duration::from_secs(60);
const IDLE_AFTER_RUN: Duration = Duration::from_secs(5);
const NEW_FOLDER_NAME: &str = "Neue Scans";

/// What the index says about the runs the wait started.
struct BackgroundRun {
    name: String,
    /// Source and target of each file that was carried out.
    filed: Vec<(PathBuf, PathBuf)>,
}

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    /// A picture in the size of the acceptance, numbered like the others.
    fn shot_wide(&mut self, name: &str) -> String {
        self.images += 1;
        let file = format!("{:02}-{name}.png", self.images);
        crate::shot_file::write_screenshot_to(
            &self.session.app,
            &self.out.to_string_lossy(),
            WIDE,
            &file,
        );
        file
    }

    fn open_index(&self) -> Option<Index> {
        Index::open(&self.index_path, &InMemoryKeyStore::empty()).ok()
    }

    fn open_review_count(&self) -> Option<usize> {
        Some(self.open_index()?.open_reviews().ok()?.len())
    }

    fn hermes_batches(&self) -> Vec<BatchId> {
        self.open_index()
            .map(|index| {
                [BatchKind::Hermes, BatchKind::Filing]
                    .into_iter()
                    .flat_map(|kind| index.undoable_batches(kind, 50).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn background_runs_since(&self, before: &[BatchId]) -> Vec<(BatchId, BackgroundRun)> {
        let Some(index) = self.open_index() else {
            return Vec::new();
        };
        self.hermes_batches()
            .into_iter()
            .filter(|batch| !before.contains(batch))
            .filter_map(|batch| {
                let name = index.batch_info(batch).ok().flatten()?.name?;
                let filed = index
                    .journal_items(batch)
                    .ok()?
                    .into_iter()
                    .filter(|item| item.state == JournalState::Moved)
                    .map(|item| (item.from, item.to))
                    .collect();
                Some((batch, BackgroundRun { name, filed }))
            })
            .collect()
    }

    /// Pumps until `done` holds or `limit` ran out.
    fn wait_until(&mut self, limit: Duration, done: impl Fn(&App) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit && !done(self.app()) {
            self.pump(Duration::from_millis(100));
        }
        done(self.app())
    }

    fn show_panels(&mut self) {
        for _ in 0..8 {
            if matches!(self.app().shown_tab_for_test(), ShownTab::Panels) {
                return;
            }
            self.deliver(Message::NextTab);
        }
    }

    /// Feeds the app its once-a-second look at the pipeline until the
    /// pipeline is idle for `quiet_for` and no job runs. Returns the
    /// quiet time that ended it, or `None` when `limit` did.
    fn tick_until_quiet(
        &mut self,
        limit: Duration,
        quiet_for: impl Fn(&Self) -> Duration,
    ) -> Option<Instant> {
        let started = Instant::now();
        let mut last_tick = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        let mut window = QuietWindow::default();
        let mut status_shot = false;
        let mut last_status = Instant::now();
        let mut reports_seen = 0;
        while started.elapsed() < limit {
            if self.background_reports.len() > reports_seen {
                reports_seen = self.background_reports.len();
                println!(
                    "[{}] background report {reports_seen} after {:.1} s",
                    self.language.code(),
                    started.elapsed().as_secs_f64()
                );
            }
            if last_status.elapsed() >= Duration::from_secs(60) {
                last_status = Instant::now();
                println!(
                    "[{}] waiting: {}, in flight {}",
                    self.language.code(),
                    self.pipeline_line(),
                    self.session.in_flight
                );
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                // Looked at before the next tick: what the last one started,
                // such as the counts read every fifth tick, has had a second
                // to report back.
                let quiet = self.app().pipeline_is_idle_for_test() && self.session.in_flight == 0;
                if window.look(Instant::now(), quiet, quiet_for(self)) {
                    return window.since();
                }
                self.deliver(Message::FilingStatus(FilingStatusMessage::Tick));
                last_tick = Instant::now();
            }
            self.pump(Duration::from_millis(100));
            if !status_shot && !self.background_reports.is_empty() {
                status_shot = true;
                self.shot_wide("continuous-status");
            }
        }
        None
    }

    /// Runner state, the queue per step, waiting candidates and whether a job runs.
    fn pipeline_line(&self) -> String {
        let (state, queues, candidates) = self.app().pipeline_status_for_test();
        let (tools, tab_blocks) = self.app().continuous_gates_for_test();
        format!(
            "runner {state}, queued per step {queues}, {candidates} candidates, jobs busy {}, tools {}, tab blocks {}",
            self.app().jobs_busy_for_test(),
            if tools { "Some" } else { "None" },
            if tab_blocks { "yes" } else { "no" }
        )
    }

    /// The panel on the right shows the folder that holds the archive and the
    /// source, with one more plain folder; returns the rows of the plain
    /// folder and of the source.
    fn show_run_folder_on_the_right(&mut self) -> (Option<usize>, Option<usize>) {
        let run = self
            .downloads
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        std::fs::create_dir_all(run.join(NEW_FOLDER_NAME)).unwrap();
        self.deliver(Message::Panel(PanelMessage::ActivatePanel(
            PanelSide::Right,
        )));
        self.deliver(Message::Panel(PanelMessage::GoUp));
        let shown = run.clone();
        self.wait_until(Duration::from_secs(30), move |app| {
            app.panel_for_test(PanelSide::Right).path == shown
                && !app.panel_for_test(PanelSide::Right).loading
        });
        let panel = self.session.app.panel_mut_for_test(PanelSide::Right);
        let row_of = |name: &str| panel.entries.iter().position(|entry| entry.name == name);
        (row_of(NEW_FOLDER_NAME), row_of("downloads"))
    }

    fn folder_menu_shot(&mut self, row: usize, name: &str) -> String {
        self.session
            .app
            .panel_mut_for_test(PanelSide::Right)
            .selected = row;
        self.deliver(Message::Files(MenuMessage::ContextMenuAt {
            side: PanelSide::Right,
            index: row,
        }));
        self.deliver(Message::Files(MenuMessage::ContextMenuLast));
        self.deliver(Message::Files(MenuMessage::ContextMenuOpenSub));
        let image = self.shot_wide(name);
        self.deliver(Message::Files(MenuMessage::ContextMenuClose));
        image
    }

    /// The pictures of the entries at the folders, the folder list, the
    /// history and F1.
    fn continuous_pictures(&mut self) -> Vec<String> {
        let mut images = Vec::new();
        self.show_panels();
        let (plain, source) = self.show_run_folder_on_the_right();
        if let Some(row) = plain {
            images.push(self.folder_menu_shot(row, "menu-folder"));
        }
        if let Some(row) = source {
            images.push(self.folder_menu_shot(row, "menu-folder-continuous"));
        }
        if let Some(row) = plain {
            self.session
                .app
                .panel_mut_for_test(PanelSide::Right)
                .selected = row;
            self.deliver(Message::CommandPalette(CommandPaletteMessage::Toggle));
            self.deliver(Message::CommandPalette(
                CommandPaletteMessage::QueryChanged("einsortieren".to_owned()),
            ));
            images.push(self.shot_wide("palette-filing"));
            self.deliver(Message::CommandPalette(CommandPaletteMessage::Close));
        }
        assert!(
            self.app().command_palette_for_test().is_none(),
            "the command palette is still open before the settings"
        );
        if self.app().settings_for_test().is_none() {
            self.deliver(Message::Settings(SettingsMessage::Toggle));
        }
        self.deliver(Message::Settings(SettingsMessage::Select(
            SettingsFeature::Hermes,
        )));
        assert!(
            self.app().settings_for_test().is_some(),
            "the settings page did not open"
        );
        self.pump(Duration::from_secs(1));
        images.push(self.shot_wide("settings-hermes"));
        self.deliver(Message::Settings(SettingsMessage::Close));
        images
    }

    /// Runs after the copy or move of `NCRS_EVAL_FILE=copy|move`: the source files by
    /// itself.
    pub fn continuous_after_tab_run(&mut self, new_files: &Path, expected: LocationTransfer) {
        let started = Instant::now();
        let downloads = self.downloads.clone();
        let same_folder = |path: &Path| {
            path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
                == downloads
                    .canonicalize()
                    .unwrap_or_else(|_| downloads.clone())
        };
        let switched_on = self.wait_until(Duration::from_secs(60), |app| {
            app.filing_locations_for_test().iter().any(|location| {
                same_folder(&location.path)
                    && location.continuous
                    && location.mode == LocationMode::Inbox
                    && location.transfer == expected
            })
        });
        let on_the_source = self
            .app()
            .filing_locations_for_test()
            .iter()
            .filter(|location| same_folder(&location.path))
            .map(|location| {
                format!(
                    "{} / continuous {} / {}",
                    if location.mode == LocationMode::Inbox {
                        "Inbox"
                    } else {
                        "Archive"
                    },
                    location.continuous,
                    if location.transfer == LocationTransfer::Copy {
                        "Copy"
                    } else {
                        "Move"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let setting = format!("after the tab run: {on_the_source}");
        println!("[{}] continuous setting: {setting}", self.language.code());
        let setting_check = expect(
            switched_on,
            format!("the source is no continuous Inbox with {expected:?}: {setting}"),
        );

        let new_names = relative_files(new_files);
        let new_total = new_names.len();
        let new_emails = count_named(&new_names, |name| {
            Path::new(name).extension().is_some_and(|ext| ext == "eml")
        });
        let new_bodies = count_named(&new_names, |name| name == "body.txt");
        let reviews_before = self.open_review_count();
        let batches_before = self.hermes_batches();

        copy_tree(new_files, &self.downloads).unwrap();
        let source_with_new = relative_files(&self.downloads);
        self.show_panels();
        self.background_reports.clear();
        self.longest_deliver = Duration::ZERO;
        self.longest_deliver_message.clear();
        let wait_began = Instant::now();
        let ended = self.tick_until_quiet(super::waiting::ai_wait_limit(), |protocol| {
            if protocol.background_reports.is_empty() {
                IDLE_WITHOUT_RUN
            } else {
                IDLE_AFTER_RUN
            }
        });
        let longest = self.longest_deliver;
        let longest_message = self.longest_deliver_message.clone();
        let waited = wait_began.elapsed();
        let report_count = self.background_reports.len();
        let reviews_after = self.open_review_count();
        let runs = self.background_runs_since(&batches_before);

        let mut carried = BTreeSet::new();
        println!(
            "[{}] continuous: {new_total} new files ({new_emails} .eml, {new_bodies} body.txt), waited {:.1} s, {report_count} reports, longest deliver {:.3} s ({longest_message})",
            self.language.code(),
            waited.as_secs_f64(),
            longest.as_secs_f64()
        );
        for (_, run) in &runs {
            println!(
                "[{}] continuous run \"{}\": {} filed",
                self.language.code(),
                run.name,
                run.filed.len()
            );
            for (from, to) in &run.filed {
                println!("    {} -> {}", from.display(), to.display());
                if let Ok(relative) = from.strip_prefix(&self.downloads) {
                    carried.insert(relative.to_path_buf());
                }
            }
        }
        let filed_new = carried.intersection(&new_names).count();
        println!(
            "[{}] continuous: {filed_new} filed, {} stayed; open reviews {} -> {}",
            self.language.code(),
            new_total.saturating_sub(filed_new),
            count_text(reviews_before),
            count_text(reviews_after)
        );
        println!(
            "[{}] continuous: longest update during the wait {:.3} s (the whole deliver, tasks it starts included)",
            self.language.code(),
            longest.as_secs_f64()
        );
        let reviews_ok = match (reviews_before, reviews_after) {
            (Some(before), Some(after)) => expect(
                after <= before,
                format!("open review entries went from {before} to {after}"),
            ),
            _ => Err("the index did not open to count the review entries".to_owned()),
        };
        let wait_check = all([
            expect(
                ended.is_some(),
                format!(
                    "the pipeline was not idle for good after {:.0} s",
                    waited.as_secs_f64()
                ),
            ),
            expect(report_count > 0, "no ContinuousFiled report came"),
            expect(!runs.is_empty(), "no new run in the journal"),
            reviews_ok,
        ]);

        let images = self.continuous_pictures();
        let shown_runs = runs
            .iter()
            .map(|(_, run)| run.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let summary = format!(
            "{new_total} new ({new_emails} .eml, {new_bodies} body.txt), {filed_new} filed, {} stayed, runs: {shown_runs}",
            new_total.saturating_sub(filed_new)
        );
        self.record(
            "5 tidy continuous",
            &summary,
            started.elapsed().as_secs_f64(),
            &images.join(", "),
            "the sure new files are filed by themselves in one run; the unsure ones stay; no review entry is added",
            all([setting_check, wait_check]),
        );

        let undo_started = Instant::now();
        let mut undo_checks: Vec<Check> = Vec::new();
        for (batch, run) in &runs {
            self.deliver(Message::Settings(SettingsMessage::UndoRun(batch.number())));
            let (_, _, idle) = self.wait_idle(READ_LIMIT, None);
            undo_checks.push(idle);
            let left: Vec<String> = run
                .filed
                .iter()
                .filter(|(_, to)| to.exists())
                .map(|(_, to)| to.display().to_string())
                .collect();
            undo_checks.push(expect(
                left.is_empty(),
                format!(
                    "after undo the targets are still there: {}",
                    left.join(", ")
                ),
            ));
        }
        let still_filed: Vec<String> = self
            .open_index()
            .map(|index| {
                runs.iter()
                    .flat_map(|(_, run)| run.filed.iter())
                    .filter(|(from, _)| {
                        index
                            .document_by_path(from)
                            .ok()
                            .flatten()
                            .is_none_or(|document| document.status == DocumentStatus::Filed)
                    })
                    .map(|(from, _)| from.display().to_string())
                    .collect()
            })
            .unwrap_or_default();
        undo_checks.push(expect(
            still_filed.is_empty(),
            format!(
                "after undo the index has no document, or a filed one, for: {}",
                still_filed.join(", ")
            ),
        ));
        let source_after = relative_files(&self.downloads);
        undo_checks.push(expect(
            source_after == source_with_new,
            format!(
                "the source changed: {} files before, {} after the undo",
                source_with_new.len(),
                source_after.len()
            ),
        ));
        println!(
            "[{}] continuous undo: {} runs put back, source {} -> {} files",
            self.language.code(),
            runs.len(),
            source_with_new.len(),
            source_after.len()
        );
        let undone_image = self.shot_wide("continuous-undone");
        self.record(
            "5 tidy continuous undo",
            &format!("{} runs put back", runs.len()),
            undo_started.elapsed().as_secs_f64(),
            &undone_image,
            "the filed copies are gone and the source is unchanged",
            all(undo_checks),
        );
    }
}

fn count_named(names: &BTreeSet<PathBuf>, wanted: impl Fn(&str) -> bool) -> usize {
    names
        .iter()
        .filter_map(|path| path.file_name())
        .filter(|name| wanted(&name.to_string_lossy()))
        .count()
}

fn count_text(count: Option<usize>) -> String {
    count.map_or_else(|| "?".to_owned(), |number| number.to_string())
}
