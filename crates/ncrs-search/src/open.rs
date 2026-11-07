//! Opening the search and changing its fields from outside the view.

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_core::PanelSide;
use ncrs_index::{KindCatalog, QuerySpec, SavedQueryId};

use crate::filters::SearchFilters;
use crate::state::Search;
use crate::view_state::{SearchField, SearchView};

impl Search {
    /// The search view, while it is up and its panel is the active one.
    pub fn open_view(&self, has_keys: bool) -> Option<&SearchView> {
        self.view.as_ref().filter(|_| has_keys)
    }

    /// The next field takes the focus: the key that opened the search was
    /// pressed again.
    pub fn focus_next_field(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.next_field();
        }
    }

    /// Shows the search in place of the panel on `side`, below `root`. A view
    /// kept from before keeps its fields and options. The filters exist when
    /// the index can answer.
    pub fn show(&mut self, side: PanelSide, root: PathBuf, indexed: bool, kinds: Arc<KindCatalog>) {
        self.side = side;
        self.open = true;
        let view = self
            .view
            .get_or_insert_with(|| SearchView::new(root.clone()));
        view.set_root(root);
        view.filters = if indexed {
            view.filters
                .take()
                .or_else(|| Some(SearchFilters::new(kinds)))
        } else {
            None
        };
    }

    /// The words typed in the box that asked, or the words of a report: they
    /// are searched in the files. Without words only the names are searched.
    pub fn set_query(&mut self, text: String) {
        if let Some(view) = self.view.as_mut() {
            view.in_files = !text.is_empty();
            view.focused = if view.in_files {
                SearchField::Query
            } else {
                SearchField::Names
            };
            view.query = text;
        }
    }

    /// The query and the filters of a report's criteria. With a `report` the
    /// search changes that one.
    pub fn start_from(&mut self, spec: &QuerySpec, report: Option<SavedQueryId>) {
        self.set_query(spec.full_text.clone().unwrap_or_default());
        if let Some(filters) = self.view.as_mut().and_then(|view| view.filters.as_mut()) {
            match report {
                Some(id) => filters.edit_report(id, spec),
                None => filters.start_from(spec),
            }
        }
    }

    /// The folder and the name of the file under the cursor, for showing it
    /// in its folder.
    pub fn cursor_location(&self, has_keys: bool) -> Option<(PathBuf, String)> {
        let (path, _) = self.open_view(has_keys)?.cursor_target()?;
        let directory = path.parent()?.to_path_buf();
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some((directory, name))
    }

    /// What the status bar shows instead of the panel's entry while the
    /// search has the keys: the cursor's file below the root. Empty without a
    /// cursor, so the panel's entry does not pose as the search's.
    pub fn status(&self, has_keys: bool) -> Option<String> {
        if !has_keys {
            return None;
        }
        let view = self.view.as_ref()?;
        let Some((path, _)) = view.cursor_target() else {
            return Some(String::new());
        };
        let relative = path.strip_prefix(&view.root).unwrap_or(&path);
        Some(relative.display().to_string())
    }
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use iced::Size;

    use super::*;

    fn search_with_view() -> Search {
        let mut search = Search::new(Size::new(1200.0, 800.0));
        search.view = Some(SearchView::new(PathBuf::from("/root")));
        search
    }

    #[test]
    fn the_text_of_an_old_report_becomes_the_text_in_files() {
        let mut search = search_with_view();
        let spec = QuerySpec {
            full_text: Some("invoice".to_owned()),
            ..QuerySpec::default()
        };
        search.start_from(&spec, None);
        let view = search.view.unwrap();
        assert_eq!(view.query, "invoice");
        assert!(view.in_files);
        assert_eq!(view.focused, SearchField::Query);
        assert_eq!(view.content_text(), "invoice");
    }

    #[test]
    fn a_report_without_text_searches_names_only() {
        let mut search = search_with_view();
        if let Some(view) = search.view.as_mut() {
            view.in_files = true;
        }
        search.start_from(&QuerySpec::default(), None);
        let view = search.view.unwrap();
        assert!(!view.in_files);
        assert_eq!(view.focused, SearchField::Names);
    }

    #[test]
    fn words_typed_in_the_box_are_searched_in_the_files() {
        let mut search = search_with_view();
        search.set_query("words".to_owned());
        let view = search.view.unwrap();
        assert!(view.in_files);
        assert_eq!(view.query, "words");
    }
}
