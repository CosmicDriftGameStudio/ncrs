//! The open Git tabs: one per repository, each with its own state and an id
//! that stays the same while other tabs open and close.

use crate::messages::GitTabId;
use iced::Task;
use ncrs_git_view::State;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(in crate::app) struct GitTabs {
    next_id: u32,
    tabs: Vec<(GitTabId, State)>,
}

impl GitTabs {
    /// The open tabs, in the order they were opened.
    pub(in crate::app) fn ids(&self) -> Vec<GitTabId> {
        self.tabs.iter().map(|(id, _)| *id).collect()
    }

    pub(in crate::app) fn iter(&self) -> impl Iterator<Item = (GitTabId, &State)> {
        self.tabs.iter().map(|(id, state)| (*id, state))
    }

    pub(in crate::app) fn get(&self, id: GitTabId) -> Option<&State> {
        self.tabs
            .iter()
            .find_map(|(tab, state)| (*tab == id).then_some(state))
    }

    pub(in crate::app) fn get_mut(&mut self, id: GitTabId) -> Option<&mut State> {
        self.tabs
            .iter_mut()
            .find_map(|(tab, state)| (*tab == id).then_some(state))
    }

    /// The tab that shows the repository at `root`.
    pub(in crate::app) fn id_of(&self, root: &Path) -> Option<GitTabId> {
        self.iter()
            .find_map(|(id, state)| (state.root.as_deref() == Some(root)).then_some(id))
    }

    /// Shows the repository at `root`: the tab that already has it, else a new
    /// one. The repository is read anew either way.
    pub(in crate::app) fn open(
        &mut self,
        root: PathBuf,
        name: String,
        ctx: &ncrs_git_view::Ctx,
    ) -> (GitTabId, Task<ncrs_git_view::Message>) {
        let id = self.id_of(&root).unwrap_or_else(|| {
            let id = GitTabId(self.next_id);
            self.next_id += 1;
            self.tabs.push((id, State::default()));
            id
        });
        let task = self.get_mut(id).map_or_else(Task::none, |state| {
            ncrs_git_view::open(state, root, name, ctx)
        });
        (id, task)
    }

    /// Forgets the tabs that were closed.
    pub(in crate::app) fn drop_closed(&mut self) {
        self.tabs.retain(|(_, state)| state.is_open());
    }

    /// A tab that is already filled, for the screenshot generators and tests.
    #[cfg(test)]
    pub(in crate::app) fn adopt_for_test(&mut self, state: State) -> GitTabId {
        let id = GitTabId(self.next_id);
        self.next_id += 1;
        self.tabs.push((id, state));
        id
    }

    /// Stops every read in flight, at quit.
    pub(in crate::app) fn stop(&mut self) {
        for (_, state) in &mut self.tabs {
            state.stop();
        }
        ncrs_git::askpass::cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(tabs: &mut GitTabs, root: &str) -> GitTabId {
        tabs.open(
            PathBuf::from(root),
            root.to_owned(),
            &ncrs_git_view::Ctx::new(ncrs_i18n::Language::English),
        )
        .0
    }

    #[test]
    fn another_repository_opens_another_tab() {
        let mut tabs = GitTabs::default();
        let first = open(&mut tabs, "/work/one");
        let second = open(&mut tabs, "/work/two");
        assert_ne!(first, second);
        assert_eq!(tabs.ids(), vec![first, second]);
    }

    #[test]
    fn the_same_repository_shows_the_tab_it_has() {
        let mut tabs = GitTabs::default();
        let first = open(&mut tabs, "/work/one");
        open(&mut tabs, "/work/two");
        assert_eq!(open(&mut tabs, "/work/one"), first);
        assert_eq!(tabs.ids().len(), 2);
    }

    #[test]
    fn closing_one_tab_leaves_the_other_with_its_id() {
        let mut tabs = GitTabs::default();
        let first = open(&mut tabs, "/work/one");
        let second = open(&mut tabs, "/work/two");
        if let Some(state) = tabs.get_mut(first) {
            state.close();
        }
        tabs.drop_closed();
        assert_eq!(tabs.ids(), vec![second]);
        let third = open(&mut tabs, "/work/three");
        assert_ne!(third, first);
        assert_eq!(tabs.ids(), vec![second, third]);
    }
}
