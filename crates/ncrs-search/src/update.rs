use iced::widget::operation;
use iced::Task;
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::filters::SearchFilters;
use crate::message::SearchMessage;
use crate::state::Search;
use crate::view::{field_id, NAMES_ID, QUERY_ID};
use crate::view_state::{resolve_folder, SearchField};

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
        SearchMessage::NamesChanged(text) => {
            view.names = text;
            view.focused = SearchField::Names;
            only(search.schedule())
        }
        SearchMessage::ToggleInFiles => {
            view.in_files = !view.in_files;
            view.focused = if view.in_files {
                SearchField::Query
            } else {
                SearchField::Names
            };
            let focus = operation::focus(field_id(view.focused));
            search.refresh_rows(ctx);
            only(Task::batch([search.schedule(), focus]))
        }
        SearchMessage::ExcludeChanged(text) => {
            view.exclude = text;
            view.focused = SearchField::Exclude;
            only(search.schedule())
        }
        // A server's folder is not one this computer can walk or choose.
        SearchMessage::FolderChanged(_)
        | SearchMessage::ChooseFolder
        | SearchMessage::FolderPicked(_)
            if ctx.side_is_remote =>
        {
            only(Task::none())
        }
        SearchMessage::FolderChanged(text) => {
            view.focused = SearchField::Folder;
            match resolve_folder(&text, &view.relative_base) {
                Some(folder) => {
                    view.root = folder;
                    view.folder_missing = false;
                    view.folder_text = text;
                    only(search.schedule())
                }
                None => {
                    view.folder_text = text;
                    view.folder_missing = true;
                    search.stop_and_clear();
                    only(Task::none())
                }
            }
        }
        SearchMessage::ChooseFolder => (Task::none(), Some(Action::PickFolder(view.root.clone()))),
        SearchMessage::FolderPicked(Some(folder)) => {
            view.set_root(folder);
            only(search.schedule())
        }
        SearchMessage::FolderPicked(None) => only(Task::none()),
        SearchMessage::AppendToQuery(typed) => {
            let id = if view.in_files && view.focused == SearchField::Query {
                view.query.push_str(&typed);
                QUERY_ID
            } else {
                view.names.push_str(&typed);
                view.focused = SearchField::Names;
                NAMES_ID
            };
            let scheduled = search.schedule();
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
        // A list taken while the search runs would hold only the hits so far.
        SearchMessage::OpenHitList(_) if view.running => only(Task::none()),
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

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use std::path::Path;

    use iced::Size;

    use super::*;
    use crate::view_state::SearchView;

    fn ctx(side_is_remote: bool) -> Ctx {
        Ctx {
            window_size: Size::new(1200.0, 800.0),
            side_is_remote,
            has_keys: true,
            index: None,
        }
    }

    fn send(search: &mut Search, message: SearchMessage, ctx: &Ctx) -> Option<Action> {
        let (task, action) = update(search, message, ctx);
        drop(task);
        action
    }

    fn open_search(root: &std::path::Path) -> Search {
        let mut search = Search::new(Size::new(1200.0, 800.0));
        search.view = Some(SearchView::new(root.to_path_buf()));
        search.open = true;
        search
    }

    fn view(search: &Search) -> &SearchView {
        search.view.as_ref().unwrap()
    }

    #[test]
    fn typing_an_existing_folder_searches_there() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());
        let before = search.generation;

        let text = to.path().display().to_string();
        let action = send(
            &mut search,
            SearchMessage::FolderChanged(text.clone()),
            &ctx(false),
        );

        assert_eq!(action, None);
        assert_eq!(view(&search).root, to.path());
        assert_eq!(view(&search).folder_text, text);
        assert!(!view(&search).folder_missing);
        assert_eq!(view(&search).focused, SearchField::Folder);
        assert_ne!(search.generation, before, "a new search is scheduled");
    }

    #[test]
    fn typing_a_missing_folder_stops_the_search_and_keeps_the_root() {
        let from = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());
        let running = search.view.as_mut().unwrap();
        running.begin(1);
        running.apply_event(crate::engine::SearchEvent::File {
            path: from.path().join("a.txt"),
            lines: Vec::new(),
            size: 0,
            modified: None,
        });
        let missing = from.path().join("nowhere");

        send(
            &mut search,
            SearchMessage::FolderChanged(missing.display().to_string()),
            &ctx(false),
        );

        let shown = view(&search);
        assert!(shown.folder_missing);
        assert_eq!(shown.root, from.path());
        assert!(shown.results.is_empty());
        assert!(!shown.running);
        assert!(search.handle.is_none());
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let from = tempfile::tempdir().unwrap();
        let file = from.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let mut search = open_search(from.path());

        send(
            &mut search,
            SearchMessage::FolderChanged(file.display().to_string()),
            &ctx(false),
        );

        assert!(view(&search).folder_missing);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_may_not_be_read_counts_as_missing() {
        use std::os::unix::fs::PermissionsExt as _;
        let from = tempfile::tempdir().unwrap();
        let locked = from.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut search = open_search(from.path());

        send(
            &mut search,
            SearchMessage::FolderChanged(locked.display().to_string()),
            &ctx(false),
        );

        let missing = view(&search).folder_missing;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(missing);
    }

    #[test]
    fn a_missing_folder_is_searched_again_once_it_exists() {
        let from = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());
        let later = from.path().join("later");
        send(
            &mut search,
            SearchMessage::FolderChanged(later.display().to_string()),
            &ctx(false),
        );
        assert!(view(&search).folder_missing);

        std::fs::create_dir(&later).unwrap();
        send(
            &mut search,
            SearchMessage::FolderChanged(later.display().to_string()),
            &ctx(false),
        );

        assert!(!view(&search).folder_missing);
        assert_eq!(view(&search).root, later);
    }

    #[test]
    fn a_tilde_stands_for_the_home_folder() {
        let from = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());

        send(
            &mut search,
            SearchMessage::FolderChanged("~".to_owned()),
            &ctx(false),
        );

        assert_eq!(view(&search).root, ncrs_fs::home_dir());
        assert_eq!(view(&search).folder_text, "~", "the typed text stays");
    }

    #[test]
    fn a_tilde_in_front_of_a_path_is_the_home_folder_below() {
        let root = Path::new("/");
        assert_eq!(resolve_folder("~/", root), Some(ncrs_fs::home_dir()));
        assert_eq!(
            resolve_folder("  ~/nonexistent-folder-for-test  ", root),
            None
        );
    }

    #[test]
    fn a_relative_folder_is_taken_from_where_the_search_started() {
        let from = tempfile::tempdir().unwrap();
        std::fs::create_dir(from.path().join("sub")).unwrap();
        let mut search = open_search(from.path());

        send(
            &mut search,
            SearchMessage::FolderChanged("sub".to_owned()),
            &ctx(false),
        );
        assert_eq!(view(&search).root, from.path().join("sub"));

        std::fs::create_dir(from.path().join("sub").join("deeper")).unwrap();
        send(
            &mut search,
            SearchMessage::FolderChanged("sub/deeper".to_owned()),
            &ctx(false),
        );
        assert_eq!(view(&search).root, from.path().join("sub/deeper"));
        assert!(!view(&search).folder_missing);
    }

    #[test]
    fn a_picked_folder_is_ignored_on_a_remote_side() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());

        send(
            &mut search,
            SearchMessage::FolderPicked(Some(to.path().to_path_buf())),
            &ctx(true),
        );

        assert_eq!(view(&search).root, from.path());
    }

    #[test]
    fn a_picked_folder_becomes_the_root() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());
        search.view.as_mut().unwrap().folder_missing = true;
        let before = search.generation;

        send(
            &mut search,
            SearchMessage::FolderPicked(Some(to.path().to_path_buf())),
            &ctx(false),
        );

        assert_eq!(view(&search).root, to.path());
        assert_eq!(view(&search).folder_text, to.path().display().to_string());
        assert!(!view(&search).folder_missing);
        assert_ne!(search.generation, before);
    }

    #[test]
    fn a_cancelled_dialog_changes_nothing() {
        let from = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());
        let before = search.generation;

        send(&mut search, SearchMessage::FolderPicked(None), &ctx(false));

        assert_eq!(view(&search).root, from.path());
        assert_eq!(search.generation, before);
    }

    #[test]
    fn the_button_asks_for_a_dialog_starting_in_the_folder() {
        let from = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());

        let action = send(&mut search, SearchMessage::ChooseFolder, &ctx(false));

        assert_eq!(action, Some(Action::PickFolder(from.path().to_path_buf())));
    }

    #[test]
    fn the_folder_of_a_server_panel_is_not_changed() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut search = open_search(from.path());

        let chosen = send(&mut search, SearchMessage::ChooseFolder, &ctx(true));
        send(
            &mut search,
            SearchMessage::FolderChanged(to.path().display().to_string()),
            &ctx(true),
        );

        assert_eq!(chosen, None);
        assert_eq!(view(&search).root, from.path());
    }
}
