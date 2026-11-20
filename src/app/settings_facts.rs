//! What the Index and Hermes pages show that only the app knows: the figures
//! of the index, the order read per folder, the last run, and the folders
//! that cannot be reached.

use crate::app::*;
use ncrs_config::SettingsFeature;
pub(in crate::app) use ncrs_hermes_ui::documents_facts::ai_detail;
use ncrs_hermes_ui::documents_facts::{read_documents_facts, undo_batch};
use ncrs_i18n::Msg;
use ncrs_pipeline::RunnerState;
use ncrs_settings::facts::{AiState, AreaFacts, DocumentsFacts, LoadedFacts};
use ncrs_settings::folders::AddAs;

/// How often the Index page may read its facts again while filing runs.
const FACTS_REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

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
            chat_keep_days: self.hermes_chat.chat_keep_days(),
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
            ncrs_hermes_chat::Availability::Off
            | ncrs_hermes_chat::Availability::Failed(_)
            | ncrs_hermes_chat::Availability::ModelFailed(_) => None,
        };
        let Some(locations) = state.config().map(|config| config.filing.locations.clone()) else {
            return Task::none();
        };
        state.facts_requested = true;
        state.facts_in_flight = true;
        let lang = self.lang;
        let now = self.wall_clock.now();
        self.facts_requested_at = Some(now);
        let stopped = lang.text(Msg::SettingsBackgroundWorkStopped).to_owned();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    read_documents_facts(access, &locations, lang, now)
                })
                .await
            },
            move |joined| {
                let loaded = joined.unwrap_or_else(|_| LoadedFacts {
                    error: Some(stopped),
                    ..LoadedFacts::default()
                });
                Message::Settings(SettingsMessage::FactsLoaded(loaded))
            },
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
        let unexpected = self
            .lang
            .text(Msg::SettingsHermesHistoryUndoUnexpected)
            .to_owned();
        let stopped = self
            .lang
            .text(Msg::SettingsBackgroundWorkStopped)
            .to_owned();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || undo_batch(&access, batch, unexpected))
                    .await
                    .unwrap_or(Err(stopped))
            },
            |result| Message::Settings(SettingsMessage::RunUndone(result)),
        )
    }
}
