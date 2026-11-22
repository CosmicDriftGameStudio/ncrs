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

    pub fn showing_filing_for_test(mut self, filing: ncrs_filing_view::State) -> Self {
        self.filing_view = filing;
        self.shown_tab = ShownTab::Filing;
        self
    }

    pub fn showing_git_for_test(mut self, git: ncrs_git_view::State) -> Self {
        let id = self.git_tabs.adopt_for_test(git);
        self.shown_tab = ShownTab::Git(id);
        self
    }
}
