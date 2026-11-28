//! Pages that are filled by hand and shown, for the screenshot generators.

use crate::app::App;
use crate::messages::ShownTab;

impl App {
    pub fn showing_monitor_for_test(mut self, monitor: ncrs_monitor::State) -> Self {
        self.monitor = monitor;
        self.shown_tab = ShownTab::Monitor;
        self
    }

    pub fn showing_notes_for_test(mut self, notes: ncrs_notes::State) -> Self {
        self.notes = notes;
        self.shown_tab = ShownTab::Notes;
        self
    }

    pub fn showing_viewer_for_test(mut self, viewer: ncrs_viewer::State) -> Self {
        self.viewer = viewer;
        self.shown_tab = ShownTab::Viewer;
        self
    }

    pub fn showing_repos_for_test(mut self, repos: ncrs_repos::State) -> Self {
        self.repos = Some(repos);
        self.shown_tab = ShownTab::Repos;
        self
    }

    /// The Repos tab as the Mac App Store build offers it: no write action.
    pub fn in_sandbox_build_for_test(mut self) -> Self {
        self.repos_read_only = true;
        self
    }

    pub fn showing_filing_for_test(mut self, filing: ncrs_filing_view::State) -> Self {
        self.filing_view = filing;
        self.shown_tab = ShownTab::Filing;
        self
    }
}

impl App {
    pub fn showing_space_for_test(mut self, space: ncrs_space_view::State) -> Self {
        self.space = Some(space);
        self.shown_tab = ShownTab::Space;
        self
    }
}
