//! Ctrl+Tab and Ctrl+Shift+Tab walk through the open tabs: the panels, then
//! the pages that stand in for them. Tab itself stays inside the shown tab.

use crate::app::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TabStop {
    Panels,
    Settings,
    Monitor,
    Hermes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDirection {
    Forward,
    Backward,
}

/// Which of the pages that stand in for the panels are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::app) struct OpenPages {
    pub settings: bool,
    pub monitor: bool,
    pub hermes: bool,
}

/// The stop after `current` among the open ones; the panels are always open.
pub(in crate::app) fn next_stop(
    pages: OpenPages,
    current: TabStop,
    direction: TabDirection,
) -> TabStop {
    let open: Vec<TabStop> = [
        (true, TabStop::Panels),
        (pages.settings, TabStop::Settings),
        (pages.monitor, TabStop::Monitor),
        (pages.hermes, TabStop::Hermes),
    ]
    .into_iter()
    .filter_map(|(is_open, stop)| is_open.then_some(stop))
    .collect();
    let at = open.iter().position(|stop| *stop == current).unwrap_or(0);
    let next = match direction {
        TabDirection::Forward => (at + 1) % open.len(),
        TabDirection::Backward => (at + open.len() - 1) % open.len(),
    };
    open.get(next).copied().unwrap_or(current)
}

impl App {
    fn current_stop(&self) -> TabStop {
        match self.shown_tab {
            ShownTab::Monitor => TabStop::Monitor,
            ShownTab::Hermes => TabStop::Hermes,
            ShownTab::Settings => TabStop::Settings,
            ShownTab::Panels => TabStop::Panels,
        }
    }

    /// A page that opened is the one shown; the shown one that closed gives
    /// way to the panels. `was_open` is the pages before the update.
    pub(in crate::app) fn follow_opened_pages(&mut self, was_open: OpenPages) {
        let now = self.open_pages();
        if now.hermes && !was_open.hermes {
            self.shown_tab = ShownTab::Hermes;
        }
        if now.monitor && !was_open.monitor {
            self.shown_tab = ShownTab::Monitor;
        }
        if now.settings && !was_open.settings {
            self.shown_tab = ShownTab::Settings;
        }
        let shown_is_closed = match self.shown_tab {
            ShownTab::Settings => !now.settings,
            ShownTab::Monitor => !now.monitor,
            ShownTab::Hermes => !now.hermes,
            ShownTab::Panels => false,
        };
        if shown_is_closed {
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
        let current = self.current_stop();
        let target = next_stop(self.open_pages(), current, direction);
        match target {
            TabStop::Panels => self.shown_tab = ShownTab::Panels,
            TabStop::Settings => self.shown_tab = ShownTab::Settings,
            TabStop::Monitor => self.shown_tab = ShownTab::Monitor,
            TabStop::Hermes => self.shown_tab = ShownTab::Hermes,
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TabDirection::{Backward, Forward};
    use TabStop::{Hermes, Monitor, Panels, Settings};

    const ALL: OpenPages = OpenPages {
        settings: true,
        monitor: true,
        hermes: true,
    };
    const NONE: OpenPages = OpenPages {
        settings: false,
        monitor: false,
        hermes: false,
    };

    #[test]
    fn without_pages_the_panels_are_the_only_stop() {
        assert_eq!(next_stop(NONE, Panels, Forward), Panels);
        assert_eq!(next_stop(NONE, Panels, Backward), Panels);
    }

    #[test]
    fn open_pages_come_after_the_panels_and_wrap() {
        assert_eq!(next_stop(ALL, Panels, Forward), Settings);
        assert_eq!(next_stop(ALL, Settings, Forward), Monitor);
        assert_eq!(next_stop(ALL, Monitor, Forward), Hermes);
        assert_eq!(next_stop(ALL, Hermes, Forward), Panels);
        assert_eq!(next_stop(ALL, Panels, Backward), Hermes);
        assert_eq!(next_stop(ALL, Hermes, Backward), Monitor);
        assert_eq!(next_stop(ALL, Monitor, Backward), Settings);
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
        assert_eq!(next_stop(monitor_only, Panels, Forward), Monitor);
        assert_eq!(next_stop(chat_and_settings, Settings, Forward), Hermes);
        assert_eq!(next_stop(chat_and_settings, Panels, Backward), Hermes);
        assert_eq!(next_stop(chat_and_settings, Hermes, Backward), Settings);
    }
}
