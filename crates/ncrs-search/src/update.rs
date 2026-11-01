use iced::widget::operation;
use iced::Task;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::filters::SearchFilters;
use crate::message::SearchMessage;
use crate::state::{release_focus, Search};
use crate::view::QUERY_ID;
use crate::view_state::SearchField;

/// Tagging comes from the keys only, and the keys are the panel's when the
/// other one is active. The toggles are not guarded the same way: a click on
/// one has to work from the other panel and activates this one.
pub fn needs_keys(message: &SearchMessage) -> bool {
    matches!(
        message,
        SearchMessage::ToggleTag
            | SearchMessage::TagMove(_)
            | SearchMessage::TagAll
            | SearchMessage::ClearTags
    )
}

/// Using the view makes its panel the active one; what runs by itself does
/// not.
pub fn activates_panel(message: &SearchMessage) -> bool {
    !matches!(
        message,
        SearchMessage::Events { .. }
            | SearchMessage::IndexRan { .. }
            | SearchMessage::SnippetsLoaded { .. }
            | SearchMessage::ReportUpdated(_)
            | SearchMessage::Debounced(_)
            | SearchMessage::Close
            | SearchMessage::Scrolled(_)
    )
}

pub fn update(
    search: &mut Search,
    message: SearchMessage,
    ctx: &Ctx,
) -> (Task<SearchMessage>, Option<Action>) {
    if !search.open {
        return (Task::none(), None);
    }
    if needs_keys(&message) && !ctx.has_keys {
        return (Task::none(), None);
    }
    let rows = search.rows;
    let Some(view) = search.view.as_mut() else {
        return (Task::none(), None);
    };
    let only = |task| (task, None);
    match message {
        SearchMessage::QueryChanged(text) => {
            view.query = text;
            view.focused = SearchField::Query;
            only(search.schedule())
        }
        SearchMessage::IncludeChanged(text) => {
            view.include = text;
            view.focused = SearchField::Include;
            only(search.schedule())
        }
        SearchMessage::ExcludeChanged(text) => {
            view.exclude = text;
            view.focused = SearchField::Exclude;
            only(search.schedule())
        }
        SearchMessage::AppendToQuery(typed) => {
            view.query.push_str(&typed);
            view.focused = SearchField::Query;
            let scheduled = search.schedule();
            let id = QUERY_ID;
            only(Task::batch([
                scheduled,
                operation::focus(id).chain(operation::move_cursor_to_end(id)),
            ]))
        }
        SearchMessage::ToggleCase | SearchMessage::ToggleWholeWord | SearchMessage::ToggleRegex
            if view.is_filtered() =>
        {
            only(Task::none())
        }
        SearchMessage::ToggleCase => {
            view.case_sensitive = !view.case_sensitive;
            only(search.schedule())
        }
        SearchMessage::ToggleWholeWord => {
            view.whole_word = !view.whole_word;
            only(search.schedule())
        }
        SearchMessage::ToggleRegex => {
            view.regex = !view.regex;
            only(search.schedule())
        }
        SearchMessage::ToggleTag => {
            view.tag_and_move(1, rows);
            only(Task::none())
        }
        SearchMessage::TagMove(delta) => {
            view.tag_and_move(delta, rows);
            only(Task::none())
        }
        SearchMessage::TagAll => {
            view.tag_all();
            only(Task::none())
        }
        SearchMessage::ClearTags => {
            view.tagged.clear();
            only(Task::none())
        }
        SearchMessage::Debounced(generation) => {
            if generation == view.generation {
                only(search.start(generation, ctx))
            } else {
                only(Task::none())
            }
        }
        SearchMessage::Events { generation, events } => {
            if generation == view.generation {
                for event in events {
                    view.apply_event(event);
                }
                view.ensure_visible(rows);
                if !view.running {
                    search.handle = None;
                }
            }
            only(Task::none())
        }
        SearchMessage::Close => (Task::none(), Some(Action::Close)),
        SearchMessage::OpenHitList(side) => (Task::none(), Some(Action::OpenHitList(side))),
        SearchMessage::IndexRan { generation, result } => {
            let Some(filters) = view
                .filters
                .as_mut()
                .filter(|filters| filters.is_current(generation))
            else {
                return only(Task::none());
            };
            match result {
                Ok(hits) => filters.ran(hits),
                Err(detail) => filters.failed(detail),
            }
            if view.is_filtered() {
                let shown_rows = search.rows;
                if let Some(kept) = search.view.as_mut() {
                    kept.mirror_index(shown_rows);
                }
                search.refresh_rows(ctx);
                return only(search.request_snippets(ctx));
            }
            search.refresh_rows(ctx);
            only(Task::none())
        }
        SearchMessage::SnippetsLoaded { generation, result } => {
            if let (Some(filters), Ok(found)) = (
                view.filters
                    .as_mut()
                    .filter(|filters| filters.is_current(generation)),
                result,
            ) {
                filters.add_snippets(found);
                if view.is_filtered() {
                    view.mirror_index(rows);
                }
            }
            only(Task::none())
        }
        SearchMessage::ToggleFacet(value) => {
            if let Some(filters) = view.filters.as_mut() {
                filters.toggle(value);
            }
            only(search.filters_changed(ctx))
        }
        SearchMessage::ClearFacet(facet) => {
            if let Some(filters) = view.filters.as_mut() {
                filters.clear(facet);
            }
            only(search.filters_changed(ctx))
        }
        SearchMessage::SaveAsReport => {
            let unsaved = view
                .filters
                .as_ref()
                .filter(|filters| filters.can_save_as_report())
                .map(SearchFilters::spec);
            if unsaved.is_none() {
                return only(Task::none());
            }
            (
                Task::none(),
                Some(Action::HandToReports {
                    unsaved,
                    report: None,
                }),
            )
        }
        SearchMessage::UpdateReport => {
            let Some((id, spec)) = view
                .filters
                .as_ref()
                .and_then(|filters| Some((filters.report()?, filters.spec())))
            else {
                return only(Task::none());
            };
            let Some(access) = ctx.index.clone() else {
                return only(Task::none());
            };
            only(Task::perform(
                tokio::task::spawn_blocking(move || access.replace_spec(id, &spec)),
                |joined| {
                    let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                    SearchMessage::ReportUpdated(result)
                },
            ))
        }
        SearchMessage::ReportUpdated(result) => match result {
            Ok(()) => {
                let report = view.filters.as_ref().and_then(SearchFilters::report);
                (
                    Task::none(),
                    Some(Action::HandToReports {
                        unsaved: None,
                        report,
                    }),
                )
            }
            Err(detail) => {
                if let Some(filters) = view.filters.as_mut() {
                    filters.failed(detail);
                }
                only(Task::none())
            }
        },
        SearchMessage::Move(delta) => {
            view.move_cursor(delta, rows);
            only(Task::none())
        }
        SearchMessage::Page(direction) => {
            view.page(direction, rows);
            only(Task::none())
        }
        SearchMessage::First => {
            view.first(rows);
            only(Task::none())
        }
        SearchMessage::Last => {
            view.last(rows);
            only(Task::none())
        }
        SearchMessage::Activate => (Task::none(), Some(Action::ActivateCursor)),
        SearchMessage::RowClicked(index) => {
            view.select_row(index, rows);
            only(release_focus())
        }
        SearchMessage::RowActivated(index) => {
            view.select_row(index, rows);
            (Task::none(), Some(Action::ActivateCursor))
        }
        SearchMessage::ToggleTagAt(index) => {
            view.select_row(index, rows);
            view.tag_and_move(0, rows);
            only(release_focus())
        }
        SearchMessage::ContextAt(index) => {
            // As in the panel: a click outside the tags drops them, so the
            // menu cannot act on files the user did not point at.
            if !view.is_tagged(index) {
                view.tagged.clear();
            }
            view.select_row(index, rows);
            (Task::none(), Some(Action::OpenContextMenu))
        }
        SearchMessage::Scrolled(delta) => {
            view.scroll(delta, rows);
            only(Task::none())
        }
    }
}
