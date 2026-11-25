use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::Task;
use ncrs_config::config::Filing;
use ncrs_filing::sorting::{forget_backlog, run_watched, SortingAccess, WatchedFiles};
use ncrs_fs::transfer::Progress;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_intake::{canonical_path, SortSource};
use ncrs_jobs::{Job, JobKind};
use ncrs_pipeline::Runner;

use crate::action::{Action, StartProgressJob};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Hermes;
use crate::watching::{SourceRun, WatchedRoots};

/// Files the watched sources from what the pipeline finished since the last
/// tick, in one job over the sources that have something due. Never during
/// another job, never two at once, and not for a source whose "Ablegen" tab
/// still plans or files; what comes meanwhile waits for the next run.
pub fn run_watching(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    if !ctx.watching_allowed || !hermes.filing_config.hermes_on() {
        return;
    }
    let sources = watched_sources(&hermes.filing_config);
    if sources.is_empty() {
        return;
    }
    if let Some(snapshot) = hermes.live_pipeline().map(Runner::snapshot) {
        hermes.watching.collect(&snapshot, || roots_of(&sources));
    }
    let job_is_starting = actions
        .iter()
        .any(|action| matches!(action, Action::StartJobWithProgress(_)));
    if ctx.jobs_busy || job_is_starting {
        return;
    }
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)))
    else {
        return;
    };
    let busy_source = ctx.filing_tab_source.map(Path::to_path_buf);
    let due = hermes.watching.take_due(&sources, |source| {
        is_source_of(busy_source.as_deref(), source)
    });
    if due.is_empty() {
        return;
    }
    let due_sources: Vec<_> = due
        .into_iter()
        .map(|(source, files)| {
            let is_switched_on = hermes.watching.is_switched_on(&source.path);
            (source, files, is_switched_on)
        })
        .collect();
    let template = ctx.lang.text(Msg::HermesChatRunNameContinuous).to_owned();
    actions.push(Action::StartJobWithProgress(StartProgressJob {
        job: Job {
            kind: JobKind::File,
            path: PathBuf::new(),
            total: None,
        },
        build: Box::new(move |progress_tx| {
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let mut progress = ncrs_fs::transfer::Reporting::new(&progress_tx);
                    Ok::<_, String>(file_watched(
                        &access,
                        &due_sources,
                        busy_source.as_deref(),
                        &template,
                        &mut progress,
                    ))
                }),
                |joined| {
                    Message::ContinuousFiled(joined.unwrap_or_else(|join| Err(join.to_string())))
                },
            )
        }),
    }));
}

/// The saved sources that are watched and not paused.
fn watched_sources(filing: &Filing) -> Vec<SortSource> {
    filing
        .sources
        .iter()
        .filter(|entry| entry.paused.is_none() && entry.source.watch)
        .map(|entry| entry.source.clone())
        .collect()
}

/// The pipeline spells a path the way the folder is on disk, which can differ
/// from what was typed.
fn roots_of(sources: &[SortSource]) -> Vec<WatchedRoots> {
    sources
        .iter()
        .map(|source| {
            let mut roots = vec![source.path.clone()];
            roots.extend(canonical_path(&source.path).ok());
            WatchedRoots {
                source: source.path.clone(),
                roots,
            }
        })
        .collect()
}

fn is_source_of(folder: Option<&Path>, source: &SortSource) -> bool {
    let Some(busy_folder) = folder else {
        return false;
    };
    busy_folder == source.path
        || canonical_path(busy_folder)
            .ok()
            .zip(canonical_path(&source.path).ok())
            .is_some_and(|(busy_root, root)| busy_root == root)
}

fn file_watched<P: Progress>(
    access: &SortingAccess,
    due: &[(SortSource, WatchedFiles, bool)],
    busy_source: Option<&Path>,
    run_name_template: &str,
    progress: &mut P,
) -> Vec<SourceRun> {
    due.iter()
        .map(|(source, files, is_switched_on)| {
            let folder = source
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            let run_name = fill_template(run_name_template, &[("{folder}", &folder)]);
            let stock = if *is_switched_on {
                forget_backlog(access, std::slice::from_ref(&source.path))
            } else {
                Ok(())
            };
            SourceRun {
                source: source.path.clone(),
                result: stock.and_then(|()| {
                    run_watched(access, source, files, busy_source, &run_name, progress)
                }),
            }
        })
        .collect()
}

/// The background filing is over: the queue is free and the user hears of
/// what was filed, if anything.
pub fn continuous_filed(
    hermes: &mut Hermes,
    result: Result<Vec<SourceRun>, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    actions.extend([Action::FinishJob, Action::ClearJobProgress]);
    let settled = hermes.watching.finish(&result);
    if settled.counts_changed {
        actions.push(Action::RequestHermesCounts);
    }
    actions.extend(notice_of(ctx.lang, &settled.filed).map(Action::ShowNotice));
    actions.extend(settled.errors.into_iter().map(Action::ShowError));
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
    use ncrs_config::config::{Filing, SourceSetting};
    use ncrs_core::PanelSide;
    use ncrs_filing::filing::{FilingModule, Phase};
    use ncrs_filing::sorting::{SortReport, WatchedReport};
    use ncrs_index::Kinds;
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets};

    use crate::ctx::{Bin, PanelFacts};

    use super::*;

    fn watched_source(path: &str) -> SourceSetting {
        SourceSetting {
            source: SortSource {
                path: PathBuf::from(path),
                include: String::new(),
                exclude: String::new(),
                targets: Vec::new(),
                watch: true,
                transfer: LocationTransfer::Move,
                no_text: OnNoText::default(),
                several: OnSeveralTargets::default(),
                own_volume: None,
            },
            paused: None,
        }
    }

    /// A ready module without a pipeline and one watched source that is due
    /// in full.
    fn hermes_with_a_due_source() -> Hermes {
        let mut hermes = Hermes {
            filing_config: Filing {
                enabled: true,
                first_run: false,
                sources: vec![watched_source("/watched")],
                ..Filing::default()
            },
            filing: Some(FilingModule::in_phase(Phase::Ready)),
            ..Hermes::default()
        };
        hermes.watching.mark_all();
        hermes
    }

    fn with_ctx<R>(
        jobs_busy: bool,
        filing_tab_source: Option<&Path>,
        step: impl FnOnce(&Ctx<'_>) -> R,
    ) -> R {
        let kinds = Kinds::default();
        let tags = ncrs_config::file_list_tags::FileListTags::default();
        let nowhere = PathBuf::from("/nowhere");
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
            search_open: false,
            jobs_busy,
            watching_allowed: true,
            trash: Bin(ncrs_fs::system_trash(None)),
            filing_tab_source,
            config_path: None,
            home: &nowhere,
            filing_status_shortcut: None,
            active_panel_path: &nowhere,
            other_panel_path: &nowhere,
            now: std::time::SystemTime::now(),
            panels: [panel(PanelSide::Left), panel(PanelSide::Right)],
            cursor_row: None,
            overlay_open: false,
            action_sources: Vec::new(),
        };
        step(&ctx)
    }

    fn starts_a_job(
        hermes: &mut Hermes,
        jobs_busy: bool,
        filing_tab_source: Option<&Path>,
    ) -> bool {
        with_ctx(jobs_busy, filing_tab_source, |ctx| {
            let mut actions = Vec::new();
            run_watching(hermes, ctx, &mut actions);
            actions
                .iter()
                .any(|action| matches!(action, Action::StartJobWithProgress(_)))
        })
    }

    #[test]
    fn a_due_source_starts_the_filing_job_and_no_second_one_while_it_runs() {
        let mut hermes = hermes_with_a_due_source();

        assert!(starts_a_job(&mut hermes, false, None));
        hermes.watching.mark_all();

        assert!(!starts_a_job(&mut hermes, false, None));
    }

    #[test]
    fn another_job_puts_the_run_off_until_the_queue_is_free() {
        let mut hermes = hermes_with_a_due_source();

        assert!(!starts_a_job(&mut hermes, true, None));
        assert!(starts_a_job(&mut hermes, false, None));
    }

    #[test]
    fn the_source_of_the_working_filing_tab_is_skipped_until_the_tab_is_done() {
        let mut hermes = hermes_with_a_due_source();

        assert!(!starts_a_job(
            &mut hermes,
            false,
            Some(Path::new("/watched"))
        ));
        assert!(starts_a_job(&mut hermes, false, None));
    }

    #[test]
    fn a_run_that_ends_frees_the_next_one_and_tells_what_was_filed() {
        let mut hermes = hermes_with_a_due_source();
        assert!(starts_a_job(&mut hermes, false, None));

        let actions = with_ctx(false, None, |ctx| {
            let mut actions = Vec::new();
            let ended = vec![SourceRun {
                source: PathBuf::from("/watched"),
                result: Ok(WatchedReport::Done {
                    run: SortReport::default(),
                    asked: 1,
                }),
            }];
            continuous_filed(&mut hermes, Ok(ended), ctx, &mut actions);
            actions
        });

        let has = |wanted: fn(&Action) -> bool| actions.iter().any(wanted);
        assert!(has(|action| matches!(action, Action::FinishJob)));
        assert!(has(|action| matches!(action, Action::RequestHermesCounts)));
        assert!(!has(|action| matches!(action, Action::ShowNotice(_))));
        hermes.watching.mark_all();
        assert!(starts_a_job(&mut hermes, false, None));
    }

    fn after_the_first_run(hermes: &mut Hermes) {
        assert!(starts_a_job(hermes, false, None));
        drop(hermes.watching.finish(&Ok(Vec::new())));
        assert!(!starts_a_job(hermes, false, None));
    }

    #[tokio::test]
    async fn a_saved_rule_asks_for_everything_again() {
        let mut hermes = hermes_with_a_due_source();
        after_the_first_run(&mut hermes);

        let generation = hermes.rules_generation;
        with_ctx(false, None, |ctx| {
            drop(super::super::rules::update(
                &mut hermes,
                crate::message::RulesMessage::Changed {
                    generation,
                    result: Ok(()),
                },
                ctx,
                &mut Vec::new(),
            ));
        });

        assert!(starts_a_job(&mut hermes, false, None));
    }

    #[test]
    fn saving_other_sources_asks_for_everything_again_and_saving_the_same_ones_does_not() {
        let mut hermes = hermes_with_a_due_source();
        after_the_first_run(&mut hermes);
        let same = hermes.filing_config.clone();

        drop(
            super::super::lifecycle::keep_running_module_with_saved_transfers(
                &mut hermes,
                same.clone(),
            ),
        );
        assert!(!starts_a_job(&mut hermes, false, None));

        let other = Filing {
            sources: vec![watched_source("/watched"), watched_source("/second")],
            ..same
        };
        drop(super::super::lifecycle::keep_running_module_with_saved_transfers(&mut hermes, other));
        assert!(starts_a_job(&mut hermes, false, None));
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
