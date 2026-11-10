use iced::widget::operation;
use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::query_work::{self, QueryAccess};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, SavedQueriesMessage};
use crate::saved_queries::SavedQueries;
use crate::state::Hermes;
use crate::ui::saved_queries as saved_queries_ui;

pub fn update(
    hermes: &mut Hermes,
    message: SavedQueriesMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        SavedQueriesMessage::Loaded { generation, result } => {
            if generation != hermes.saved_queries_generation {
                return Task::none();
            }
            let Some(view) = hermes.saved_queries.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(rows) => {
                    let prefilled = view.is_prefilled();
                    view.loaded(rows);
                    let focus = if prefilled {
                        operation::focus(saved_queries_ui::NAME_ID)
                    } else {
                        Task::none()
                    };
                    Task::batch([focus, run_selected_query(hermes), run_query_counts(hermes)])
                }
                Err(detail) => {
                    view.failed(detail);
                    Task::none()
                }
            }
        }
        SavedQueriesMessage::Counted { generation, result } => {
            if generation == hermes.saved_queries_generation {
                if let Some(view) = hermes.saved_queries.as_mut() {
                    match result {
                        Ok(counts) => view.counted(counts),
                        Err(detail) => view.failed(detail),
                    }
                }
            }
            Task::none()
        }
        SavedQueriesMessage::Edit(index) => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.begin_edit(index);
            }
            let needs_run = hermes
                .saved_queries
                .as_ref()
                .is_some_and(|view| view.edit_is_open() && view.results().is_none());
            if needs_run {
                run_selected_query(hermes)
            } else {
                Task::none()
            }
        }
        SavedQueriesMessage::New => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.begin_new();
            }
            Task::batch([
                operation::focus(saved_queries_ui::NAME_ID),
                run_selected_query(hermes),
            ])
        }
        SavedQueriesMessage::Template(choice) => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.choose_template(choice, ctx.lang);
            }
            run_selected_query(hermes)
        }
        SavedQueriesMessage::EditInSearch => {
            let Some(view) = hermes.saved_queries.as_ref() else {
                return Task::none();
            };
            let Some(spec) = view.selected_spec().cloned() else {
                return Task::none();
            };
            let report = view.selected().map(|query| query.id);
            actions.push(Action::OpenSearchFor { spec, report });
            Task::none()
        }
        SavedQueriesMessage::NameChanged(name) => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.set_name(name);
            }
            Task::none()
        }
        SavedQueriesMessage::Save => {
            let Some(request) = hermes
                .saved_queries
                .as_mut()
                .and_then(SavedQueries::begin_save)
            else {
                return Task::none();
            };
            let generation = hermes.saved_queries_generation;
            let Some(access) = query_access(hermes) else {
                return Task::none();
            };
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    access.save(request.id, &request.name, &request.spec)
                }),
                move |joined| {
                    let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                    Message::SavedQueries(SavedQueriesMessage::Saved { generation, result })
                },
            )
        }
        SavedQueriesMessage::Saved { generation, result } => {
            if generation != hermes.saved_queries_generation {
                return Task::none();
            }
            let Some(view) = hermes.saved_queries.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(id) => view.saved(id),
                Err(detail) => {
                    view.failed(detail);
                    return Task::none();
                }
            }
            run_query_load(hermes)
        }
        SavedQueriesMessage::AskDelete(index) => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.select(index);
                view.ask_delete();
            }
            Task::none()
        }
        SavedQueriesMessage::CancelStep => close_dialog(hermes),
        SavedQueriesMessage::ConfirmDelete => {
            let Some(id) = hermes
                .saved_queries
                .as_mut()
                .and_then(SavedQueries::begin_delete)
            else {
                return Task::none();
            };
            let generation = hermes.saved_queries_generation;
            let Some(access) = query_access(hermes) else {
                return Task::none();
            };
            Task::perform(
                tokio::task::spawn_blocking(move || access.delete(id).map(|_| id)),
                move |joined| {
                    let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                    Message::SavedQueries(SavedQueriesMessage::Deleted { generation, result })
                },
            )
        }
        SavedQueriesMessage::Deleted { generation, result } => {
            if generation != hermes.saved_queries_generation {
                return Task::none();
            }
            let Some(view) = hermes.saved_queries.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(id) => view.deleted(id),
                Err(detail) => {
                    view.failed(detail);
                    return Task::none();
                }
            }
            run_query_load(hermes)
        }
        SavedQueriesMessage::Ran {
            generation,
            serial,
            result,
        } => {
            if generation == hermes.saved_queries_generation {
                if let Some(view) = hermes.saved_queries.as_mut() {
                    match result {
                        Ok(hits) => view.ran(serial, hits),
                        Err(detail) => view.failed(detail),
                    }
                }
            }
            Task::none()
        }
        SavedQueriesMessage::ToggleMissing => {
            if let Some(view) = hermes.saved_queries.as_mut() {
                view.toggle_missing();
            }
            Task::none()
        }
        SavedQueriesMessage::StartExport(index) => {
            // The other panel's folder is where files are usually wanted.
            let folder = ctx.other_panel_path.to_path_buf();
            let Some(view) = hermes.saved_queries.as_mut() else {
                return Task::none();
            };
            view.select(index);
            if view.cursor() != Some(index) {
                return Task::none();
            }
            view.export_when_ready(folder);
            if view.results().is_some() {
                return Task::none();
            }
            run_selected_query(hermes)
        }
        SavedQueriesMessage::PickFolder => {
            let start_in = hermes
                .saved_queries
                .as_ref()
                .and_then(|view| view.export_step())
                .map_or_else(
                    || ctx.active_panel_path.to_path_buf(),
                    |step| step.folder.clone(),
                );
            actions.push(Action::PickExportFolder(start_in));
            Task::none()
        }
        SavedQueriesMessage::FolderPicked(folder) => {
            if let (Some(view), Some(picked)) = (hermes.saved_queries.as_mut(), folder) {
                view.set_export_folder(&picked);
            }
            Task::none()
        }
        SavedQueriesMessage::Export => {
            let Some(request) = hermes
                .saved_queries
                .as_mut()
                .and_then(SavedQueries::begin_export)
            else {
                return Task::none();
            };
            let generation = hermes.saved_queries_generation;
            let now = chrono::Local::now().naive_local();
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    query_work::export(&request.hits, &request.folder, now)
                }),
                move |joined| {
                    let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                    Message::SavedQueries(SavedQueriesMessage::Exported { generation, result })
                },
            )
        }
        SavedQueriesMessage::Exported { generation, result } => {
            if generation == hermes.saved_queries_generation {
                if let Some(view) = hermes.saved_queries.as_mut() {
                    match result {
                        Ok(summary) => view.export_done(summary),
                        Err(detail) => view.failed(detail),
                    }
                }
            }
            Task::none()
        }
    }
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.saved_queries.is_some() {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.saved_queries_generation = hermes.saved_queries_generation.wrapping_add(1);
    hermes.saved_queries = Some(
        SavedQueries::new(chrono::Datelike::year(&chrono::Local::now()))
            .with_kinds(ctx.kinds.clone()),
    );
    run_query_load(hermes)
}

/// Closes the dialog that is up and drops a search that was not saved.
pub fn close_dialog(hermes: &mut Hermes) -> Task<Message> {
    if let Some(view) = hermes.saved_queries.as_mut() {
        view.cancel_step();
    }
    Task::none()
}

fn run_query_load(hermes: &mut Hermes) -> Task<Message> {
    let generation = hermes.saved_queries_generation;
    let Some(access) = query_access(hermes) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.load()),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::SavedQueries(SavedQueriesMessage::Loaded { generation, result })
        },
    )
}

/// Asks the index for the hits of the selected report.
fn run_selected_query(hermes: &mut Hermes) -> Task<Message> {
    let Some((serial, spec)) = hermes
        .saved_queries
        .as_mut()
        .and_then(SavedQueries::begin_run)
    else {
        return Task::none();
    };
    let generation = hermes.saved_queries_generation;
    let Some(access) = query_access(hermes) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.run(&spec)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::SavedQueries(SavedQueriesMessage::Ran {
                generation,
                serial,
                result,
            })
        },
    )
}

/// Counts the hits of every report for the list.
fn run_query_counts(hermes: &mut Hermes) -> Task<Message> {
    let Some(rows) = hermes
        .saved_queries
        .as_ref()
        .map(|view| view.rows().to_vec())
    else {
        return Task::none();
    };
    if rows.is_empty() {
        return Task::none();
    }
    let generation = hermes.saved_queries_generation;
    let Some(access) = query_access(hermes) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.counts(&rows)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::SavedQueries(SavedQueriesMessage::Counted { generation, result })
        },
    )
}

fn query_access(hermes: &mut Hermes) -> Option<QueryAccess> {
    let access = hermes.filing.as_ref().and_then(FilingModule::query_access);
    if access.is_none() {
        if let Some(view) = hermes.saved_queries.as_mut() {
            view.failed(INDEX_NOT_OPEN.to_owned());
        }
    }
    access
}
