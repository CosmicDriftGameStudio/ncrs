//! What the Index and Hermes pages show that only the app knows: the figures
//! of the index, the order read per folder, the last run, and the folders
//! that cannot be reached.

use std::collections::BTreeMap;
use std::time::SystemTime;

use chrono::{DateTime, Local, TimeZone as _};

use crate::app::*;
use ncrs_ai::Compute;
use ncrs_config::config::FilingLocation;
use ncrs_config::SettingsFeature;
use ncrs_filing::chat_work::{ChatAccess, ChatResult, ChatWork, RecentRun};
use ncrs_filing::filing::AiStatus;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::BatchId;
use ncrs_intake::{area_order_plain, LocationMode};
use ncrs_pipeline::RunnerState;
use ncrs_settings::facts::{
    AiState, AreaFacts, DocumentsFacts, LoadedFacts, ReadFolderCoverage, RunDay, RunFacts,
};
use ncrs_settings::folders::AddAs;

/// How often the Index page may read its facts again while filing runs.
const FACTS_REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// The line under the AI row of the Index page: which model is in memory and
/// on what it computes, or why it failed.
pub(in crate::app) fn ai_detail(status: &AiStatus, lang: Language) -> Option<String> {
    match status {
        AiStatus::Loaded {
            name,
            memory_bytes,
            compute,
        } => {
            let computes_on = match compute {
                Compute::Gpu(device) => device.as_str(),
                Compute::Cpu => lang.text(Msg::FilingStatusAiComputeCpu),
            };
            Some(fill_template(
                lang.text(Msg::FilingStatusAiLoaded),
                &[
                    ("{name}", name),
                    ("{memory}", &ncrs_ui_kit::format::size(*memory_bytes)),
                    ("{compute}", computes_on),
                ],
            ))
        }
        AiStatus::Failed(reason) => Some(fill_template(
            lang.text(Msg::FilingStatusAiFailed),
            &[("{reason}", reason)],
        )),
        AiStatus::Off
        | AiStatus::NotDownloaded
        | AiStatus::NotLoadedYet
        | AiStatus::Http { .. } => None,
    }
}

impl App {
    pub(in crate::app) fn documents_facts(
        &self,
        overview: &OverviewFacts,
        state: &settings_ui::SettingsState,
    ) -> DocumentsFacts {
        let counts = overview.counts.unwrap_or_default();
        let loaded = state.loaded.clone().unwrap_or_default();
        let status = &self.hermes.filing_status;
        let tile = overview.ai_tile();
        let paused = status.snapshot().state == RunnerState::Paused;
        let (documents, with_text, understood) = match loaded.read_folders {
            Some(read) => (read.documents, read.with_text, read.understood),
            None => (
                counts.archive_documents,
                counts.archive_documents_with_text,
                counts.archive_documents_with_text,
            ),
        };
        DocumentsFacts {
            documents,
            understood,
            work_left: !paused && with_text > understood,
            paused,
            unreadable: status.errors_newest_first().count(),
            disconnected: loaded.disconnected,
            areas: state.loaded.as_ref().map(|_| loaded.areas),
            runs: loaded.runs,
            folder_files: loaded.folder_files,
            system_reads_scans: self.system_reads_scans,
            ai_detail: ai_detail(&overview.ai_status, self.lang),
            rules: usize::try_from(counts.confirmed_rules).unwrap_or(usize::MAX),
            ai: match tile {
                // Nothing is sent to a server that was not agreed to, whether or not
                // the module runs.
                _ if tile != crate::hermes_overview::AiTile::Off
                    && ncrs_settings::ai_consent::server_is_not_allowed(
                        &self.hermes.filing_config.ai,
                    ) =>
                {
                    AiState::NotAllowed
                }
                crate::hermes_overview::AiTile::Off => AiState::Off,
                crate::hermes_overview::AiTile::NotLoaded => AiState::Missing,
                crate::hermes_overview::AiTile::Downloading { .. } => AiState::Downloading,
                crate::hermes_overview::AiTile::Ready => AiState::Ready,
                crate::hermes_overview::AiTile::OwnServer {
                    model,
                    last_failure,
                } => AiState::OwnServer {
                    model,
                    failure: last_failure.map(|failure| {
                        crate::hermes_overview::server_failure_text(failure, self.lang)
                    }),
                },
                crate::hermes_overview::AiTile::TakesEffectAtRestart => AiState::NextStart,
                crate::hermes_overview::AiTile::Failed(_) => AiState::Failed,
            },
        }
    }

    /// Reads what the open Index or Hermes page shows in the background, once
    /// per change of the folder lists.
    pub(in crate::app) fn load_documents_facts(&mut self) -> Task<Message> {
        let availability = self.chat_availability();
        let Some(state) = self.settings.page.as_mut() else {
            return Task::none();
        };
        if state.facts_requested
            || !matches!(
                state.feature,
                SettingsFeature::Index | SettingsFeature::Hermes
            )
        {
            return Task::none();
        }
        let access = match availability {
            ncrs_hermes_chat::Availability::Ready(access) => Some(*access),
            ncrs_hermes_chat::Availability::Starting => return Task::none(),
            ncrs_hermes_chat::Availability::Off | ncrs_hermes_chat::Availability::Failed(_) => None,
        };
        let Some(locations) = state.config().map(|config| config.filing.locations.clone()) else {
            return Task::none();
        };
        state.facts_requested = true;
        state.facts_in_flight = true;
        let lang = self.lang;
        let now = self.wall_clock.now();
        self.facts_requested_at = Some(now);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    read_documents_facts(access, &locations, lang, now)
                })
                .await
            },
            |joined| Message::Settings(SettingsMessage::FactsLoaded(joined.unwrap_or_default())),
        )
    }

    /// While filing runs, the Status line of the open Index page moves: the
    /// facts are read again on the tick that moves the title-bar rate, at most
    /// every `FACTS_REFRESH_EVERY`.
    pub(in crate::app) fn refresh_documents_facts(&mut self) -> Task<Message> {
        let due = self.facts_requested_at.is_none_or(|asked| {
            self.wall_clock
                .now()
                .duration_since(asked)
                .is_ok_and(|waited| waited >= FACTS_REFRESH_EVERY)
        });
        let Some(state) = self.settings.page.as_mut() else {
            return Task::none();
        };
        if !due
            || state.facts_in_flight
            || state.feature != SettingsFeature::Index
            || state.loaded.is_none()
        {
            return Task::none();
        }
        state.facts_requested = false;
        self.load_documents_facts()
    }

    pub(in crate::app) fn pick_read_folder(&self, kind: AddAs) -> Task<Message> {
        (self.pick_folder)(&self.home)
            .map(move |path| Message::Settings(SettingsMessage::FolderPicked { kind, path }))
    }

    pub(in crate::app) fn grant_read_folder(&self, kind: AddAs, path: PathBuf) -> Task<Message> {
        if self.sandbox.grants_dir.is_none() {
            return Task::done(Message::Settings(SettingsMessage::FolderGranted {
                kind,
                path,
                result: Ok(()),
            }));
        }
        self.grant_chosen_folder(&path, move |picked, result| {
            Message::Settings(SettingsMessage::FolderGranted {
                kind,
                path: picked,
                result,
            })
        })
    }

    /// The chat opens in place of the settings, at the change of `area` when
    /// one is named.
    pub(in crate::app) fn open_chat_from_settings(
        &mut self,
        to_change: Option<AreaFacts>,
    ) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = self.update_hermes_chat(HermesChatMessage::Open);
        let changed = match to_change {
            Some(named) => self.update_hermes_chat(HermesChatMessage::ChangeArea {
                folder: named.read_folder,
                area: named.name,
            }),
            None => Task::none(),
        };
        Task::batch([left, opened, changed])
    }

    /// Puts the run of `batch` back; the page reads its facts again afterwards.
    pub(in crate::app) fn undo_run(&self, batch: i64) -> Task<Message> {
        let ncrs_hermes_chat::Availability::Ready(access) = self.chat_availability() else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || undo_batch(&access, batch))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            |result| Message::Settings(SettingsMessage::RunUndone(result)),
        )
    }
}

fn undo_batch(access: &ChatAccess, batch: i64) -> Result<(), String> {
    match access.run(ChatWork::Undo {
        batch: BatchId::from_number(batch),
    }) {
        ChatResult::Undone(undone) => undone.map(|_report| ()),
        _ => Ok(()),
    }
}

/// How many runs "Show all" can list.
const RUNS_LISTED_AT_MOST: usize = 50;

fn read_documents_facts(
    chat_access: Option<ChatAccess>,
    locations: &[FilingLocation],
    lang: Language,
    now: SystemTime,
) -> LoadedFacts {
    let disconnected: Vec<PathBuf> = locations
        .iter()
        .filter(|location| std::fs::metadata(&location.path).is_err())
        .map(|location| location.path.clone())
        .collect();
    let Some(access) = chat_access else {
        return LoadedFacts {
            disconnected,
            ..LoadedFacts::default()
        };
    };
    let mut areas = Vec::new();
    let mut folder_files = BTreeMap::new();
    let mut read_folders = ReadFolderCoverage::default();
    for location in locations {
        if !disconnected.contains(&location.path) {
            if let Ok(files) = access.documents_under(&location.path) {
                folder_files.insert(location.path.clone(), files);
                if location.mode == LocationMode::Archive {
                    if let Ok((with_text, understood)) = access.ai_coverage_under(&location.path) {
                        let wide = |count: usize| u64::try_from(count).unwrap_or(u64::MAX);
                        read_folders.documents += wide(files);
                        read_folders.with_text += wide(with_text);
                        read_folders.understood += wide(understood);
                    }
                }
            }
        }
        let Ok(structure) = access.known_structure(&location.path) else {
            continue;
        };
        areas.extend(
            structure
                .areas
                .iter()
                .filter(|area| !area.folder.is_empty())
                .map(|area| AreaFacts {
                    name: area.folder.clone(),
                    files: usize::try_from(area.documents).unwrap_or(usize::MAX),
                    read_folder: location.path.clone(),
                    root: location.path.join(&area.folder),
                    order: area_order_plain(lang, area),
                }),
        );
    }
    let runs = access
        .recent_runs(RUNS_LISTED_AT_MOST)
        .unwrap_or_default()
        .iter()
        .map(|run| run_facts(run, now))
        .collect();
    LoadedFacts {
        areas,
        runs,
        folder_files,
        read_folders: Some(read_folders),
        disconnected,
    }
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .unwrap_or(folder.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn run_facts(run: &RecentRun, now: SystemTime) -> RunFacts {
    let finished = Local
        .timestamp_opt(run.summary.finished_at, 0)
        .single()
        .unwrap_or_default();
    let (day, time) = run_day_and_time(finished, DateTime::<Local>::from(now));
    RunFacts {
        batch: run.summary.batch.number(),
        day,
        time,
        files: run.summary.items,
        from: run
            .summary
            .source_folder
            .as_deref()
            .map(folder_name)
            .unwrap_or_default(),
        to: run
            .target_folder
            .as_deref()
            .map(folder_name)
            .unwrap_or_default(),
        examples: run
            .examples
            .iter()
            .map(|example| example.to_string_lossy().into_owned())
            .collect(),
        undoable: run.undoable,
    }
}

/// Today and yesterday by the local calendar; older runs by their date.
fn run_day_and_time(finished: DateTime<Local>, now: DateTime<Local>) -> (RunDay, String) {
    let date = finished.date_naive();
    let today = now.date_naive();
    let day = if date == today {
        RunDay::Today
    } else if today.pred_opt() == Some(date) {
        RunDay::Yesterday
    } else {
        RunDay::On(date.format("%Y-%m-%d").to_string())
    };
    (day, finished.format("%H:%M").to_string())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, 2, 0)
            .single()
            .expect("a valid local time")
    }

    #[test]
    fn a_run_is_today_yesterday_or_dated_by_the_local_calendar() {
        let now = at(4, 9);
        assert_eq!(
            run_day_and_time(at(4, 1), now),
            (RunDay::Today, "01:02".to_owned())
        );
        assert_eq!(run_day_and_time(at(3, 23), now).0, RunDay::Yesterday);
        assert_eq!(
            run_day_and_time(at(1, 14), now),
            (RunDay::On("2026-11-01".to_owned()), "14:02".to_owned())
        );
    }

    fn filing_location(path: PathBuf, mode: LocationMode) -> FilingLocation {
        FilingLocation {
            path,
            mode,
            images: false,
            is_target: false,
            transfer: ncrs_config::config::LocationTransfer::Move,
        }
    }

    fn index_document(
        index: &mut ncrs_index::Index,
        seed: u8,
        path: PathBuf,
        text: Option<&str>,
        understood: bool,
    ) {
        let id = index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash([seed; 32]),
                path,
                source: ncrs_index::DocumentSource::Archive,
                received_at: chrono::Utc::now(),
                stamp: None,
            })
            .expect("insert");
        if let Some(body) = text {
            index.set_document_text(id, body).expect("text");
        }
        if understood {
            index.set_ai_read_version(id, 2).expect("asked");
        }
    }

    #[test]
    fn read_folder_coverage_counts_only_what_lies_below_read_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (archive, inbox) = (dir.path().join("archive"), dir.path().join("inbox"));
        std::fs::create_dir_all(&archive).expect("archive");
        std::fs::create_dir_all(&inbox).expect("inbox");
        let mut index =
            ncrs_index::Index::create_plain(&dir.path().join("index.db")).expect("index");
        index_document(&mut index, 1, archive.join("read.pdf"), Some("Rent"), true);
        index_document(
            &mut index,
            2,
            archive.join("open.pdf"),
            Some("Letter"),
            false,
        );
        index_document(&mut index, 3, archive.join("scan.png"), None, false);
        index_document(
            &mut index,
            4,
            inbox.join("former-archive.pdf"),
            Some("Bill"),
            true,
        );
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let access = ChatAccess::new(
            Arc::new(std::sync::Mutex::new(Some(index))),
            Arc::new(ncrs_intake::TextChain::new().with_source(ncrs_intake::PlainText)),
            Arc::new(ncrs_fs::JournaledTrash::new(
                dir.path().join("trash.toml"),
                ncrs_fs::FolderBackend(bin),
            )),
        );
        let locations = [
            filing_location(archive, LocationMode::Archive),
            filing_location(inbox, LocationMode::Inbox),
        ];
        let loaded = read_documents_facts(
            Some(access),
            &locations,
            Language::English,
            SystemTime::now(),
        );
        assert_eq!(
            loaded.read_folders,
            Some(ReadFolderCoverage {
                documents: 3,
                with_text: 2,
                understood: 1,
            })
        );
    }
}
