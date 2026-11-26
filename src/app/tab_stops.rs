//! Ctrl+Tab and Ctrl+Shift+Tab walk through the open tabs: the panels, then
//! the pages that stand in for them. Tab itself stays inside the shown tab.

use crate::app::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDirection {
    Forward,
    Backward,
}

/// Which of the pages that stand in for the panels are open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::app) struct OpenPages {
    pub settings: bool,
    pub monitor: bool,
    pub viewer: bool,
    pub notes: bool,
    pub filing: bool,
    pub repos: bool,
    /// The open Git tabs, in the order they were opened.
    pub git: Vec<GitTabId>,
    pub hermes: bool,
}

impl OpenPages {
    fn is_open(&self, tab: ShownTab) -> bool {
        match tab {
            ShownTab::Panels => true,
            ShownTab::Settings => self.settings,
            ShownTab::Monitor => self.monitor,
            ShownTab::Viewer => self.viewer,
            ShownTab::Notes => self.notes,
            ShownTab::Filing => self.filing,
            ShownTab::Repos => self.repos,
            ShownTab::Git(id) => self.git.contains(&id),
            ShownTab::Hermes => self.hermes,
        }
    }
}

/// The order Ctrl+Tab walks the tabs in, up to the Git tabs.
const TABS_BEFORE_GIT: [ShownTab; 7] = [
    ShownTab::Panels,
    ShownTab::Settings,
    ShownTab::Monitor,
    ShownTab::Viewer,
    ShownTab::Notes,
    ShownTab::Filing,
    ShownTab::Repos,
];

impl OpenPages {
    /// The open tabs in the order Ctrl+Tab walks them; each Git tab is a stop.
    fn stops(&self) -> Vec<ShownTab> {
        TABS_BEFORE_GIT
            .into_iter()
            .chain(self.git.iter().map(|id| ShownTab::Git(*id)))
            .chain([ShownTab::Hermes])
            .filter(|tab| self.is_open(*tab))
            .collect()
    }
}

/// When several pages open at once, the last of these is the one shown; the
/// Git tabs rank between `SHOWN_BEFORE_GIT` and `SHOWN_AFTER_GIT`.
const SHOWN_BEFORE_GIT: [ShownTab; 4] = [
    ShownTab::Viewer,
    ShownTab::Notes,
    ShownTab::Filing,
    ShownTab::Repos,
];
const SHOWN_AFTER_GIT: [ShownTab; 3] = [ShownTab::Hermes, ShownTab::Monitor, ShownTab::Settings];

/// The tab after `current` among the open ones; the panels are always open.
pub(in crate::app) fn next_stop(
    pages: &OpenPages,
    current: ShownTab,
    direction: TabDirection,
) -> ShownTab {
    let open = pages.stops();
    let at = open.iter().position(|stop| *stop == current).unwrap_or(0);
    let next = match direction {
        TabDirection::Forward => (at + 1) % open.len(),
        TabDirection::Backward => (at + open.len() - 1) % open.len(),
    };
    open.get(next).copied().unwrap_or(current)
}

impl App {
    /// A page that opened is the one shown; the shown one that closed gives
    /// way to the panels. `was_open` is the pages before the update.
    pub(in crate::app) fn follow_opened_pages(&mut self, was_open: OpenPages) {
        let now = self.open_pages();
        let git_tabs = now.git.iter().map(|id| ShownTab::Git(*id));
        for tab in SHOWN_BEFORE_GIT
            .into_iter()
            .chain(git_tabs)
            .chain(SHOWN_AFTER_GIT)
        {
            if now.is_open(tab) && !was_open.is_open(tab) {
                self.shown_tab = tab;
            }
        }
        if !now.is_open(self.shown_tab) {
            self.shown_tab = ShownTab::Panels;
        }
    }

    /// Tab inside a shown page: from field to field while a text field has the
    /// keyboard; on the settings tab otherwise between the page list and the
    /// rows; on the other pages to the next widget that takes the keyboard.
    pub(in crate::app) fn tab_inside_page(
        &mut self,
        backward: bool,
        field_focused: bool,
    ) -> Task<Message> {
        let in_widgets = || {
            if backward {
                iced::widget::operation::focus_previous()
            } else {
                iced::widget::operation::focus_next()
            }
        };
        if self.hermes_dialog_keys() == HermesDialogKeys::Target {
            return next_target_field();
        }
        if self.shown_tab != ShownTab::Settings || field_focused {
            return in_widgets();
        }
        let Some(page) = self.settings.page.as_ref() else {
            return Task::none();
        };
        let key = match page.focus.area {
            ncrs_settings::focus::Area::Pages => FocusKey::Right,
            ncrs_settings::focus::Area::Rows => FocusKey::Escape,
        };
        self.settings_focus_key(key)
    }

    pub(in crate::app) fn switch_tab(&mut self, direction: TabDirection) -> Task<Message> {
        let target = next_stop(&self.open_pages(), self.shown_tab, direction);
        self.shown_tab = target;
        if target == ShownTab::Hermes {
            return ncrs_hermes_chat::focus_input();
        }
        Task::none()
    }
}

/// The dialog of a target has two fields. Named, so Tab never walks to a field
/// behind the dialog.
fn next_target_field() -> Task<Message> {
    use ncrs_hermes_ui::ui::target_dialog::{FOLDER_FIELD_ID, NAME_FIELD_ID};
    operation::is_focused(NAME_FIELD_ID).then(|on_name| {
        operation::focus(if on_name {
            FOLDER_FIELD_ID
        } else {
            NAME_FIELD_ID
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ShownTab::{Hermes, Monitor, Notes, Panels, Settings, Viewer};
    use TabDirection::{Backward, Forward};

    const ALL: OpenPages = OpenPages {
        settings: true,
        monitor: true,
        viewer: true,
        notes: true,
        filing: false,
        repos: false,
        git: Vec::new(),
        hermes: true,
    };
    const NONE: OpenPages = OpenPages {
        settings: false,
        monitor: false,
        viewer: false,
        notes: false,
        filing: false,
        repos: false,
        git: Vec::new(),
        hermes: false,
    };

    #[test]
    fn without_pages_the_panels_are_the_only_stop() {
        assert_eq!(next_stop(&NONE, Panels, Forward), Panels);
        assert_eq!(next_stop(&NONE, Panels, Backward), Panels);
    }

    #[test]
    fn open_pages_come_after_the_panels_and_wrap() {
        assert_eq!(next_stop(&ALL, Panels, Forward), Settings);
        assert_eq!(next_stop(&ALL, Settings, Forward), Monitor);
        assert_eq!(next_stop(&ALL, Monitor, Forward), Viewer);
        assert_eq!(next_stop(&ALL, Viewer, Forward), Notes);
        assert_eq!(next_stop(&ALL, Notes, Forward), Hermes);
        assert_eq!(next_stop(&ALL, Hermes, Forward), Panels);
        assert_eq!(next_stop(&ALL, Panels, Backward), Hermes);
        assert_eq!(next_stop(&ALL, Hermes, Backward), Notes);
        assert_eq!(next_stop(&ALL, Notes, Backward), Viewer);
        assert_eq!(next_stop(&ALL, Viewer, Backward), Monitor);
        assert_eq!(next_stop(&ALL, Monitor, Backward), Settings);
    }

    #[test]
    fn a_closed_page_is_skipped() {
        let monitor_only = OpenPages {
            monitor: true,
            ..NONE
        };
        let chat_and_settings = OpenPages {
            settings: true,
            hermes: true,
            ..NONE
        };
        assert_eq!(next_stop(&monitor_only, Panels, Forward), Monitor);
        assert_eq!(next_stop(&chat_and_settings, Settings, Forward), Hermes);
        assert_eq!(next_stop(&chat_and_settings, Panels, Backward), Hermes);
        assert_eq!(next_stop(&chat_and_settings, Hermes, Backward), Settings);
    }

    #[test]
    fn every_git_tab_is_a_stop_where_the_git_tab_was() {
        let first = ShownTab::Git(GitTabId(4));
        let second = ShownTab::Git(GitTabId(7));
        let pages = OpenPages {
            notes: true,
            hermes: true,
            git: vec![GitTabId(4), GitTabId(7)],
            ..NONE
        };
        assert_eq!(next_stop(&pages, Notes, Forward), first);
        assert_eq!(next_stop(&pages, first, Forward), second);
        assert_eq!(next_stop(&pages, second, Forward), Hermes);
        assert_eq!(next_stop(&pages, Hermes, Backward), second);
        assert_eq!(next_stop(&pages, second, Backward), first);
        assert_eq!(next_stop(&pages, first, Backward), Notes);
    }
}
