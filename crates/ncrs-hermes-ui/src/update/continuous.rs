use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::LocationTransfer;
use ncrs_filing::chat_work::{file_new, open_in};
use ncrs_filing::filing::FilingModule;
use ncrs_filing::filing::LocationStatus;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::Transfer;
use ncrs_jobs::{Job, JobKind};
use ncrs_pipeline::{ContinuousCandidate, RunnerState};

use crate::action::{Action, StartProgressJob};
use crate::ctx::{ContinuousTools, Ctx};
use crate::message::Message;
use crate::state::Hermes;

/// What one read folder that files by itself gets to file in one go.
struct Batch {
    source: PathBuf,
    transfer: Transfer,
    files: Vec<PathBuf>,
}

/// The files the rules left open in the read folders that file continuously
/// are handed to the model in one job, one folder after the other. Waits for
/// a busy queue, a pipeline at work and an "Ablegen" tab of the folder that
/// still plans or files.
pub fn run_continuous(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    if actions
        .iter()
        .any(|action| matches!(action, Action::StartJobWithProgress(_)))
    {
        return;
    }
    let Some(tools) = ctx.continuous.clone() else {
        return;
    };
    if ctx.jobs_busy {
        return;
    }
    let Some(runner) = hermes.live_pipeline() else {
        return;
    };
    if runner.state() != RunnerState::Idle {
        return;
    }
    let Some(module) = hermes.filing.as_ref() else {
        return;
    };
    let Some((target, folders)) = target_and_folders(module, &hermes.filing_config.locations)
    else {
        return;
    };
    let wanted = |location: &Path| {
        folders.iter().any(|(root, _)| root == location) && ctx.filing_tab_source != Some(location)
    };
    if !folders.is_empty() && !runner.continuous_seeded() {
        let roots: Vec<PathBuf> = folders.iter().map(|(root, _)| root.clone()).collect();
        // ponytail: a file the model was unsure about is planned again once per app start.
        if let Ok(found) = open_in(&tools.access, &roots) {
            runner.seed_continuous_candidates(
                found
                    .into_iter()
                    .map(|(location, path)| ContinuousCandidate { location, path })
                    .collect(),
            );
        }
    }
    if !runner.has_continuous_candidates(&wanted) {
        return;
    }
    let batches = batches_of(&folders, runner.take_continuous_candidates(&wanted));
    let template = ctx.lang.text(Msg::HermesChatRunNameContinuous).to_owned();
    actions.push(Action::StartJobWithProgress(StartProgressJob {
        job: Job {
            kind: JobKind::File,
            path: PathBuf::new(),
            total: None,
        },
        build: Box::new(move |_progress| {
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    file_batches(&tools, &target, batches, &template)
                }),
                |joined| {
                    Message::ContinuousFiled(joined.unwrap_or_else(|join| Err(join.to_string())))
                },
            )
        }),
    }));
}

/// The write folder and the folders that file by themselves. The flag and the
/// transfer come from the saved config: the module keeps the ones it was
/// built with until the next start, and "continuous" must work at once.
fn target_and_folders(
    module: &FilingModule,
    saved: &[ncrs_config::config::FilingLocation],
) -> Option<(PathBuf, Vec<(PathBuf, Transfer)>)> {
    let (target, folders) = target_and_continuous(module, saved);
    Some((target?, folders))
}

/// The runner learns at once which roots file by themselves, also when the
/// module is only replaced at the next start.
pub fn sync_continuous_roots(hermes: &Hermes) {
    let (Some(runner), Some(module)) = (hermes.live_pipeline(), hermes.filing.as_ref()) else {
        return;
    };
    let (_, folders) = target_and_continuous(module, &hermes.filing_config.locations);
    runner.set_continuous_roots(folders.into_iter().map(|(root, _)| root));
}

fn target_and_continuous(
    module: &FilingModule,
    saved: &[ncrs_config::config::FilingLocation],
) -> (Option<PathBuf>, Vec<(PathBuf, Transfer)>) {
    let mut folders = Vec::new();
    for entry in module.locations() {
        let LocationStatus::Ready(location) = &entry.status else {
            continue;
        };
        let saved_entry = saved
            .iter()
            .find(|candidate| candidate.path == entry.configured.path);
        if let Some(chosen) = saved_entry.filter(|candidate| candidate.continuous) {
            let transfer = match chosen.transfer {
                LocationTransfer::Move => Transfer::Move,
                LocationTransfer::Copy => Transfer::Copy,
            };
            folders.push((location.path.clone(), transfer));
        }
    }
    (saved_target(module, saved), folders)
}

/// The write folder of the saved config, which a module started before it was
/// chosen does not know. Its resolved path when the module has it.
fn saved_target(
    module: &FilingModule,
    saved: &[ncrs_config::config::FilingLocation],
) -> Option<PathBuf> {
    let chosen = saved.iter().find(|candidate| candidate.is_target)?;
    let resolved = module
        .locations()
        .iter()
        .find_map(|entry| match &entry.status {
            LocationStatus::Ready(location) if entry.configured.path == chosen.path => {
                Some(location.path.clone())
            }
            _ => None,
        });
    Some(resolved.unwrap_or_else(|| chosen.path.clone()))
}

fn batches_of(folders: &[(PathBuf, Transfer)], taken: Vec<ContinuousCandidate>) -> Vec<Batch> {
    folders
        .iter()
        .filter_map(|(root, transfer)| {
            let files: Vec<PathBuf> = taken
                .iter()
                .filter(|candidate| &candidate.location == root)
                .filter_map(|candidate| candidate.path.strip_prefix(root).ok())
                .map(Path::to_path_buf)
                .collect();
            (!files.is_empty()).then(|| Batch {
                source: root.clone(),
                transfer: *transfer,
                files,
            })
        })
        .collect()
}

/// The folders and how many files each had filed. A folder that fails does
/// not hold back the next one; the first error is reported at the end.
fn file_batches(
    tools: &ContinuousTools,
    target: &Path,
    batches: Vec<Batch>,
    run_name_template: &str,
) -> Result<Vec<(PathBuf, usize)>, String> {
    let mut filed = Vec::new();
    let mut first_error = None;
    for batch in batches {
        let folder = batch
            .source
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let run_name = fill_template(run_name_template, &[("{folder}", &folder)]);
        match file_new(
            &tools.access,
            &batch.source,
            target,
            &batch.files,
            batch.transfer,
            tools.model.as_ref(),
            run_name,
        ) {
            Ok(report) => filed.push((batch.source, report.filed)),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if filed.is_empty() => Err(error),
        _ => Ok(filed),
    }
}

/// The background filing is over: the queue is free and the user hears of
/// what was filed, if anything.
pub fn continuous_filed(
    result: Result<Vec<(PathBuf, usize)>, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    actions.extend([Action::FinishJob, Action::ClearJobProgress]);
    match result {
        Ok(filed) => actions.extend(notice_of(ctx.lang, &filed).map(Action::ShowNotice)),
        Err(detail) => actions.push(Action::ShowError(detail)),
    }
}

fn notice_of(lang: Language, filed: &[(PathBuf, usize)]) -> Option<String> {
    let count: usize = filed.iter().map(|(_, filed_count)| filed_count).sum();
    if count == 0 {
        return None;
    }
    let folder = filed
        .iter()
        .filter(|(_, each)| *each > 0)
        .filter_map(|(root, _)| root.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    Some(fill_template(
        lang.text(Msg::StatusContinuousFiled),
        &[("{count}", &count.to_string()), ("{folder}", &folder)],
    ))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};

    use ncrs_ai::{LocalModelStatus, ScriptedChatModel};
    use ncrs_config::config::{Filing, FilingLocation};
    use ncrs_core::PanelSide;
    use ncrs_filing::chat_work::ChatAccess;
    use ncrs_fs::{FolderBackend, JournaledTrash};
    use ncrs_index::{Index, Kinds};
    use ncrs_intake::LocationMode;
    use ncrs_pipeline::{Load, Runner, Settings, SystemClock};
    use tempfile::TempDir;

    use crate::ctx::{Bin, PanelFacts};

    use super::*;

    fn configured(
        path: &Path,
        mode: LocationMode,
        is_target: bool,
        continuous: bool,
    ) -> FilingLocation {
        FilingLocation {
            path: path.to_path_buf(),
            mode,
            images: false,
            is_target,
            transfer: LocationTransfer::Copy,
            continuous,
        }
    }

    struct Scene {
        dir: TempDir,
        hermes: Hermes,
        tools: ContinuousTools,
    }

    /// A ready module over `module_locations` and the saved config
    /// `saved_locations`, a running idle pipeline without locations, and one
    /// waiting candidate in the inbox.
    fn scene(
        module_locations: impl FnOnce(&Path, &Path) -> Vec<FilingLocation>,
        saved_locations: impl FnOnce(&Path, &Path) -> Vec<FilingLocation>,
    ) -> Scene {
        let dir = TempDir::new().unwrap();
        let inbox = dir.path().join("inbox");
        let archive = dir.path().join("archive");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::create_dir_all(archive.join("Alfa")).unwrap();
        std::fs::write(inbox.join("new.txt"), "Something").unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let slot = Arc::new(Mutex::new(Some(index)));
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let trash = Arc::new(JournaledTrash::new(
            dir.path().join("trash.toml"),
            FolderBackend(bin),
        ));
        let access = ChatAccess::new(
            Arc::clone(&slot),
            Arc::new(ncrs_intake::TextChain::new()),
            trash,
        );
        let module_list = module_locations(&inbox, &archive);
        let module = FilingModule::ready_over_index_with_locations(
            Index::create_plain(&dir.path().join("module.db")).unwrap(),
            &module_list,
        );
        let root = module
            .reachable_locations()
            .find(|location| location.mode == LocationMode::Inbox)
            .map_or(inbox.clone(), |location| location.path.clone());
        let runner = Runner::start(
            Settings::new(Load::Gentle, NonZeroUsize::MIN),
            Vec::new(),
            Vec::new(),
            Arc::new(SystemClock),
        );
        for _ in 0..250 {
            if runner.state() == RunnerState::Idle {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(runner.state(), RunnerState::Idle);
        runner.seed_continuous_candidates(vec![ContinuousCandidate {
            location: root.clone(),
            path: root.join("new.txt"),
        }]);
        let hermes = Hermes {
            filing_config: Filing {
                enabled: true,
                locations: saved_locations(&inbox, &archive),
                ..Filing::default()
            },
            filing: Some(module),
            pipeline: Some(runner),
            ..Hermes::default()
        };
        let tools = ContinuousTools {
            access,
            model: Arc::new(ScriptedChatModel::new(LocalModelStatus::NotLoadedYet, [])),
        };
        Scene { dir, hermes, tools }
    }

    fn started_job(scene: &mut Scene) -> bool {
        let kinds = Kinds::default();
        let tags = ncrs_config::file_list_tags::FileListTags::default();
        let nowhere = scene.dir.path().to_path_buf();
        let panel = |side| PanelFacts {
            side,
            directory: &nowhere,
            request_id: 0,
            readable: true,
            visible: Vec::new(),
        };
        let ctx = Ctx {
            lang: Language::English,
            kinds: &kinds,
            file_list_tags: &tags,
            start_over_dialog_open: false,
            hermes_is_restarting: false,
            hermes_has_open_reviews: false,
            context_menu_open: false,
            search_open: false,
            hermes_active: true,
            chat_open: false,
            hermes_counts_missing: false,
            jobs_busy: false,
            continuous: Some(scene.tools.clone()),
            trash: Bin(ncrs_fs::system_trash(None)),
            filing_tab_source: None,
            window_size: iced::Size::new(800.0, 600.0),
            config_path: None,
            home: &nowhere,
            filing_status_shortcut: None,
            active_panel_path: &nowhere,
            other_panel_path: &nowhere,
            quick_fix_anchor: iced::Point::ORIGIN,
            inquiry_paths: Vec::new(),
            now: std::time::SystemTime::now(),
            panels: [panel(PanelSide::Left), panel(PanelSide::Right)],
            cursor_entry: None,
            cursor_row: None,
            overlay_open: false,
            action_sources: Vec::new(),
        };
        let mut actions = Vec::new();
        run_continuous(&mut scene.hermes, &ctx, &mut actions);
        actions
            .iter()
            .any(|action| matches!(action, Action::StartJobWithProgress(_)))
    }

    fn inbox_and_archive(inbox: &Path, archive: &Path) -> Vec<FilingLocation> {
        vec![
            configured(inbox, LocationMode::Inbox, false, true),
            configured(archive, LocationMode::Archive, true, false),
        ]
    }

    #[test]
    fn a_waiting_candidate_of_a_continuous_inbox_starts_the_filing_job() {
        let mut scene = scene(inbox_and_archive, inbox_and_archive);

        assert!(started_job(&mut scene));
    }

    #[test]
    fn a_write_folder_saved_after_the_module_started_is_the_target_at_once() {
        let mut scene = scene(
            |inbox, _| vec![configured(inbox, LocationMode::Inbox, false, false)],
            inbox_and_archive,
        );

        assert!(started_job(&mut scene));
    }

    fn candidate(root: &str, file: &str) -> ContinuousCandidate {
        ContinuousCandidate {
            location: PathBuf::from(root),
            path: PathBuf::from(root).join(file),
        }
    }

    #[test]
    fn each_folder_gets_its_own_files_as_paths_below_it_and_an_empty_one_none() {
        let folders = [
            (PathBuf::from("/in/a"), Transfer::Move),
            (PathBuf::from("/in/b"), Transfer::Copy),
            (PathBuf::from("/in/c"), Transfer::Move),
        ];
        let taken = vec![
            candidate("/in/a", "x.pdf"),
            candidate("/in/b", "sub/y.pdf"),
            candidate("/in/a", "z.pdf"),
        ];

        let batches = batches_of(&folders, taken);

        let seen: Vec<_> = batches
            .into_iter()
            .map(|batch| (batch.source, batch.transfer, batch.files))
            .collect();
        assert_eq!(
            seen,
            [
                (
                    PathBuf::from("/in/a"),
                    Transfer::Move,
                    vec![PathBuf::from("x.pdf"), PathBuf::from("z.pdf")]
                ),
                (
                    PathBuf::from("/in/b"),
                    Transfer::Copy,
                    vec![PathBuf::from("sub/y.pdf")]
                ),
            ]
        );
    }

    #[test]
    fn the_notice_names_the_count_and_the_folder_and_is_silent_for_zero() {
        let filed = [
            (PathBuf::from("/in/Scans"), 3),
            (PathBuf::from("/in/Mail"), 0),
        ];

        assert_eq!(
            notice_of(Language::English, &filed).as_deref(),
            Some("Hermes filed 3 files from Scans.")
        );
        assert_eq!(
            notice_of(Language::English, &[(PathBuf::from("/in/Scans"), 0)]),
            None
        );
    }
}
