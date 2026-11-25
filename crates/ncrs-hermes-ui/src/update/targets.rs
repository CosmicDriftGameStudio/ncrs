//! The targets of the page Einsortieren: folding their rules, the dialog of a
//! target and removing one.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::{
    add_sort_target, remove_sort_target, upsert_sort_source, upsert_sort_target, Filing,
    FilingWriteError,
};
use ncrs_filing::filing::FilingModule;
use ncrs_filing::sorting::is_on_own_volume;
use ncrs_i18n::Msg;
use ncrs_intake::{SortSource, SortTarget};

use super::{hermes_file, lifecycle, rules, sorting, suggest};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Hermes;
use crate::target_dialog::{
    target_name, Removed, SuggestAfter, TargetDialog, TargetRemoval, TargetsMessage,
};

fn targets(message: TargetsMessage) -> Message {
    Message::Targets(message)
}

/// Opens the dialog of a new target at `folder`; once it is saved, the
/// proposals for it open, the pattern `wish` first.
pub fn open_dialog_then_suggest(
    hermes: &mut Hermes,
    folder: &Path,
    wish: Option<String>,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    let mut dialog = TargetDialog::new(None);
    dialog.folder = folder.to_string_lossy().into_owned();
    dialog.then_suggest = Some(SuggestAfter::new(wish, &hermes.filing_config));
    hermes.target_dialog = Some(dialog);
    check_folder(hermes, ctx)
}

pub(super) fn update(
    hermes: &mut Hermes,
    message: TargetsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        TargetsMessage::ToggleFold(id) => {
            if let Some(page) = hermes.rules.as_mut() {
                page.toggle_fold(id);
            }
        }
        TargetsMessage::Suggest(id) => {
            let (task, more) = suggest::open_scheme(hermes, id, None, ctx);
            actions.extend(more);
            return task;
        }
        TargetsMessage::Add => hermes.target_dialog = Some(TargetDialog::new(None)),
        TargetsMessage::Change(index) => {
            hermes.target_dialog = hermes
                .filing_config
                .targets
                .get(index)
                .map(|setting| TargetDialog::new(Some(setting)));
            return check_folder(hermes, ctx);
        }
        TargetsMessage::Confirm(index) => return confirm(hermes, index, ctx, actions),
        TargetsMessage::FolderChecked { generation, exists } => {
            if let Some(dialog) = hermes.target_dialog.as_mut() {
                dialog.folder_checked(generation, exists);
            }
        }
        TargetsMessage::StatesLoaded(states) => {
            if let Some(page) = hermes.rules.as_mut() {
                page.set_target_states(states);
            }
        }
        TargetsMessage::AskRemove(index) => ask_remove(hermes, index),
        TargetsMessage::CancelRemove => hermes.target_removal = None,
        TargetsMessage::ConfirmRemove => return confirm_remove(hermes, ctx, actions),
        TargetsMessage::Name(text) => {
            if let Some(dialog) = hermes.target_dialog.as_mut() {
                dialog.name = text;
            }
        }
        TargetsMessage::Folder(text) => {
            if let Some(dialog) = hermes.target_dialog.as_mut() {
                dialog.folder = text;
            }
            return check_folder(hermes, ctx);
        }
        TargetsMessage::PickFolder => {
            if let Some(dialog) = hermes.target_dialog.as_ref().filter(|d| !d.folder.is_empty()) {
                actions.push(Action::PickTargetFolder(Some(dialog.path(ctx.home))));
            } else {
                actions.push(Action::PickTargetFolder(None));
            }
        }
        TargetsMessage::FolderPicked(picked) => {
            if let (Some(folder), Some(dialog)) = (picked, hermes.target_dialog.as_mut()) {
                dialog.folder = folder.to_string_lossy().into_owned();
                return check_folder(hermes, ctx);
            }
        }
        TargetsMessage::Save => return save(hermes, ctx, actions),
        TargetsMessage::Cancel => hermes.target_dialog = None,
        TargetsMessage::Written(result) => return written(hermes, *result, ctx, actions),
        TargetsMessage::Removed(removed) => return removed_target(hermes, *removed, ctx, actions),
    }
    Task::none()
}

/// Asks the disk, off the UI thread, whether the folder of the dialog is
/// there: a drive can hang.
fn check_folder(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(dialog) = hermes.target_dialog.as_mut() else {
        return Task::none();
    };
    let folder = dialog.path(ctx.home);
    let generation = dialog.begin_check();
    Task::perform(
        tokio::task::spawn_blocking(move || folder.is_dir()),
        move |joined| {
            targets(TargetsMessage::FolderChecked {
                generation,
                exists: joined.unwrap_or(false),
            })
        },
    )
}

/// Reads the states of all targets, off the UI thread.
pub(super) fn load_states(hermes: &Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let ids: Vec<_> = hermes
        .filing_config
        .targets
        .iter()
        .map(|setting| setting.target.id.clone())
        .collect();
    let access = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(std::sync::Arc::clone(&ctx.trash.0)));
    let Some(access) = access else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || {
            ids.into_iter()
                .map(|id| {
                    let state = access.target_state(&id);
                    (id, state)
                })
                .collect::<Vec<_>>()
        }),
        |joined| targets(TargetsMessage::StatesLoaded(joined.unwrap_or_default())),
    )
}

/// "Bestätigen": remembers whether the folder is on a drive of its own, which
/// is what a target taken over from an older version lacks.
fn confirm(
    hermes: &mut Hermes,
    index: usize,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(setting) = hermes.filing_config.targets.get(index) else {
        return Task::none();
    };
    let Some(config_path) = ctx.config_path.map(PathBuf::from) else {
        no_config_path(ctx, actions);
        return Task::none();
    };
    let target = setting.target.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let own_volume = is_on_own_volume(&target.path).unwrap_or(false);
            let confirmed = SortTarget {
                own_volume: Some(own_volume),
                ..target
            };
            upsert_sort_target(&config_path, &confirmed).map(|config| config.filing)
        }),
        |joined| targets(TargetsMessage::Written(Box::new(joined.unwrap_or_else(join_failed)))),
    )
}

fn ask_remove(hermes: &mut Hermes, index: usize) {
    let Some(setting) = hermes.filing_config.targets.get(index) else {
        return;
    };
    let id = setting.target.id.clone();
    let rules = hermes
        .rules
        .as_ref()
        .map_or(0, |page| page.rows_of(&id).count());
    hermes.target_removal = Some(TargetRemoval {
        name: target_name(&setting.target),
        id,
        rules,
        busy: false,
    });
}

fn no_config_path(ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    actions.push(Action::ShowError(
        ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned(),
    ));
}

fn save(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(dialog) = hermes.target_dialog.as_ref() else {
        return Task::none();
    };
    if !dialog.can_save(&hermes.filing_config, ctx.home) {
        return Task::none();
    }
    let Some(config_path) = ctx.config_path.map(PathBuf::from) else {
        no_config_path(ctx, actions);
        return Task::none();
    };
    let draft = dialog.clone();
    let typed = dialog.path(ctx.home);
    if let Some(dialog) = hermes.target_dialog.as_mut() {
        dialog.saving = true;
    }
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let folder = ncrs_intake::canonical_path(&typed).unwrap_or(typed);
            let own_volume = is_on_own_volume(&folder);
            let config = match draft.replacing.clone() {
                None => add_sort_target(&config_path, draft.name.trim(), &folder, own_volume)
                    .map(|(_, config)| config),
                Some(id) => upsert_sort_target(
                    &config_path,
                    &draft.target(id, folder, own_volume),
                ),
            };
            config.map(|config| config.filing)
        }),
        |joined| targets(TargetsMessage::Written(Box::new(joined.unwrap_or_else(join_failed)))),
    )
}

fn join_failed<T>(join: tokio::task::JoinError) -> Result<T, FilingWriteError> {
    Err(FilingWriteError::Io {
        path: PathBuf::new(),
        reason: join.to_string(),
    })
}

fn written(
    hermes: &mut Hermes,
    result: Result<Filing, FilingWriteError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(filing) => {
            let then_suggest = hermes
                .target_dialog
                .take()
                .and_then(|dialog| dialog.then_suggest)
                .and_then(|after| after.added(&filing).map(|added| (added, after.wish)));
            let applying = take_up(hermes, filing, ctx, actions);
            let suggesting = match then_suggest {
                Some((target, wish)) => {
                    let (task, more) = suggest::open_scheme(hermes, target, wish, ctx);
                    actions.extend(more);
                    task
                }
                None => Task::none(),
            };
            Task::batch([
                applying,
                sorting::load(hermes, ctx),
                load_states(hermes, ctx),
                suggesting,
            ])
        }
        Err(error) => {
            if let Some(dialog) = hermes.target_dialog.as_mut() {
                dialog.saving = false;
            }
            hermes_file::role_save_failed(hermes, &error, ctx, actions);
            Task::none()
        }
    }
}

fn take_up(
    hermes: &mut Hermes,
    filing: Filing,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let (applies, applying) = lifecycle::apply_saved_filing(hermes, filing, ctx);
    actions.push(Action::ShowNotice(lifecycle::saved_filing_notice(
        applies, ctx,
    )));
    actions.push(Action::RulesChanged);
    applying
}

/// The sources that name the target lose it; one left without a target is
/// paused by the config, which says so on the page.
fn sources_without(hermes: &Hermes, id: &ncrs_index::TargetId) -> Vec<SortSource> {
    hermes
        .filing_config
        .sources
        .iter()
        .filter(|setting| setting.source.targets.contains(id))
        .map(|setting| SortSource {
            targets: setting
                .source
                .targets
                .iter()
                .filter(|named| *named != id)
                .cloned()
                .collect(),
            ..setting.source.clone()
        })
        .collect()
}

fn confirm_remove(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(removal) = hermes.target_removal.as_ref().filter(|asked| !asked.busy) else {
        return Task::none();
    };
    let Some(config_path) = ctx.config_path.map(PathBuf::from) else {
        no_config_path(ctx, actions);
        return Task::none();
    };
    let id = removal.id.clone();
    let sources = sources_without(hermes, &id);
    let access = hermes.filing.as_ref().and_then(FilingModule::rules_access);
    if let Some(asked) = hermes.target_removal.as_mut() {
        asked.busy = true;
    }
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let filing = sources
                .iter()
                .try_for_each(|source| {
                    upsert_sort_source(&config_path, source, None).map(|_| ())
                })
                .and_then(|()| remove_sort_target(&config_path, &id))
                .map(|config| config.filing);
            let rules_failure = match (&filing, access) {
                (Ok(_), Some(access)) => access.delete_rules_of(&id).err(),
                (Ok(_), None) => Some(ncrs_core::INDEX_NOT_OPEN.to_owned()),
                (Err(_), _) => None,
            };
            Removed {
                filing,
                rules_failure,
            }
        }),
        |joined| {
            let removed = joined.unwrap_or_else(|join| Removed {
                filing: join_failed(join),
                rules_failure: None,
            });
            targets(TargetsMessage::Removed(Box::new(removed)))
        },
    )
}

fn removed_target(
    hermes: &mut Hermes,
    removed: Removed,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    hermes.target_removal = None;
    if let Some(reason) = removed.rules_failure {
        actions.push(Action::ShowError(reason));
    }
    match removed.filing {
        Ok(filing) => {
            let applying = take_up(hermes, filing, ctx, actions);
            Task::batch([
                applying,
                sorting::load(hermes, ctx),
                rules::reload(hermes),
                load_states(hermes, ctx),
            ])
        }
        Err(error) => {
            hermes_file::role_save_failed(hermes, &error, ctx, actions);
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use ncrs_config::config::TargetSetting;
    use ncrs_filing::rules_work::RulesLoad;
    use ncrs_index::TargetId;
    use ncrs_intake::SortTarget;

    use super::*;
    use crate::rules_page::fixtures::listed;
    use crate::rules_page::RulesPage;

    fn hermes_with_rules(rules_in_first: usize) -> Hermes {
        let first = TargetId::new("t1").unwrap();
        let mut hermes = Hermes::default();
        hermes.filing_config.targets = vec![TargetSetting {
            target: SortTarget {
                id: first.clone(),
                name: "Company".to_owned(),
                path: PathBuf::from("/data/company"),
                own_volume: Some(false),
            },
            paused: None,
        }];
        let mut page = RulesPage::new();
        page.loaded(RulesLoad {
            rules: (0..rules_in_first)
                .map(|at| {
                    let mut row = listed(i64::try_from(at).unwrap() + 1, at);
                    row.rule.target = first.clone();
                    row
                })
                .collect(),
        });
        hermes.rules = Some(page);
        hermes
    }

    #[test]
    fn removing_a_target_asks_first_and_names_how_many_rules_go_with_it() {
        let mut hermes = hermes_with_rules(3);

        ask_remove(&mut hermes, 0);

        let asked = hermes.target_removal.expect("the question");
        assert_eq!((asked.name.as_str(), asked.rules, asked.busy), ("Company", 3, false));
        assert_eq!(hermes.filing_config.targets.len(), 1);
    }

    #[test]
    fn a_place_that_is_not_in_the_list_asks_nothing() {
        let mut hermes = hermes_with_rules(0);

        ask_remove(&mut hermes, 5);

        assert!(hermes.target_removal.is_none());
    }
}
