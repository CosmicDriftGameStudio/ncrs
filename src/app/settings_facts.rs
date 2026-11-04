//! What the Index and Hermes pages show that only the app knows: the figures
//! of the index, the order read per folder, the last run, and the folders
//! that cannot be reached.

use std::time::{Duration, SystemTime};

use crate::app::*;
use ncrs_config::config::FilingLocation;
use ncrs_config::SettingsFeature;
use ncrs_filing::chat_work::{ChatAccess, ChatResult, ChatWork, RunSummary};
use ncrs_intake::area_order_plain;
use ncrs_pipeline::RunnerState;
use ncrs_settings::facts::{AiState, AreaFacts, DocumentsFacts, LoadedFacts, RunFacts};
use ncrs_settings::folders::AddAs;

impl App {
    pub(in crate::app) fn documents_facts(
        &self,
        overview: &OverviewFacts,
        state: &settings_ui::SettingsState,
    ) -> DocumentsFacts {
        let counts = overview.counts.unwrap_or_default();
        let loaded = state.loaded.clone().unwrap_or_default();
        let status = &self.hermes.filing_status;
        DocumentsFacts {
            documents: counts.archive_documents,
            understood: counts.archive_documents_with_text,
            paused: status.snapshot().state == RunnerState::Paused,
            unreadable: status.errors_newest_first().count(),
            disconnected: loaded.disconnected,
            areas: state.loaded.as_ref().map(|_| loaded.areas),
            last_run: loaded.last_run,
            rules: usize::try_from(counts.confirmed_rules).unwrap_or(usize::MAX),
            ai: match overview.ai_tile() {
                crate::hermes_overview::AiTile::Off => AiState::Off,
                crate::hermes_overview::AiTile::NotLoaded => AiState::Missing,
                crate::hermes_overview::AiTile::Downloading { .. } => AiState::Downloading,
                crate::hermes_overview::AiTile::Ready => AiState::Ready,
                crate::hermes_overview::AiTile::OwnServer { model } => AiState::OwnServer { model },
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
            ncrs_hermes_chat::Availability::Ready(access) => Some(access),
            ncrs_hermes_chat::Availability::Starting => return Task::none(),
            ncrs_hermes_chat::Availability::NeedsSetup => None,
        };
        let Some(locations) = state.config().map(|config| config.filing.locations.clone()) else {
            return Task::none();
        };
        state.facts_requested = true;
        let lang = self.lang;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || read_documents_facts(access, &locations, lang))
                    .await
            },
            |joined| Message::Settings(SettingsMessage::FactsLoaded(joined.unwrap_or_default())),
        )
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
        self.grant_chosen_folder(&path, move |path, result| {
            Message::Settings(SettingsMessage::FolderGranted { kind, path, result })
        })
    }

    /// The chat opens in place of the settings, at the change of `area` when
    /// one is named.
    pub(in crate::app) fn open_chat_from_settings(
        &mut self,
        area: Option<AreaFacts>,
    ) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = self.update_hermes_chat(HermesChatMessage::Open);
        let changed = match area {
            Some(area) => self.update_hermes_chat(HermesChatMessage::ChangeArea {
                folder: area.read_folder,
                area: area.name,
            }),
            None => Task::none(),
        };
        Task::batch([left, opened, changed])
    }

    pub(in crate::app) fn open_rules_from_settings(&mut self) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = self.update(Message::Rules(RulesMessage::Open));
        Task::batch([left, opened])
    }

    /// Puts the last run back; the page reads its facts again afterwards.
    pub(in crate::app) fn undo_last_run(&self) -> Task<Message> {
        let ncrs_hermes_chat::Availability::Ready(access) = self.chat_availability() else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || undo_last(&access))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            |result| Message::Settings(SettingsMessage::LastRunUndone(result)),
        )
    }
}

fn undo_last(access: &ChatAccess) -> Result<(), String> {
    let ChatResult::LastRun(last) = access.run(ChatWork::LastRun) else {
        return Ok(());
    };
    let Some(run) = last? else {
        return Ok(());
    };
    match access.run(ChatWork::Undo { batch: run.batch }) {
        ChatResult::Undone(undone) => undone.map(|_report| ()),
        _ => Ok(()),
    }
}

fn read_documents_facts(
    access: Option<ChatAccess>,
    locations: &[FilingLocation],
    lang: Language,
) -> LoadedFacts {
    let disconnected = locations
        .iter()
        .filter(|location| std::fs::metadata(&location.path).is_err())
        .map(|location| location.path.clone())
        .collect();
    let Some(access) = access else {
        return LoadedFacts {
            disconnected,
            ..LoadedFacts::default()
        };
    };
    let mut areas = Vec::new();
    for location in locations.iter().filter(|location| !location.is_target) {
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
    let last_run = match access.run(ChatWork::LastRun) {
        ChatResult::LastRun(Ok(Some(run))) => Some(run_facts(&run)),
        _ => None,
    };
    LoadedFacts {
        areas,
        last_run,
        disconnected,
    }
}

fn run_facts(run: &RunSummary) -> RunFacts {
    let seconds = u64::try_from(run.finished_at).unwrap_or_default();
    RunFacts {
        when: ncrs_ui_kit::format::time(Some(
            SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
        )),
        files: run.items,
        from: run
            .source_folder
            .as_deref()
            .map(|folder| {
                folder
                    .file_name()
                    .unwrap_or(folder.as_os_str())
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_default(),
    }
}
