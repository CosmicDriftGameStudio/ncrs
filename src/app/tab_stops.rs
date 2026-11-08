//! Tab and Shift+Tab walk through the open tabs: the left panel, the right
//! one, then the pages that stand in for the panels.

use crate::app::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TabStop {
    Left,
    Right,
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
        (true, TabStop::Left),
        (true, TabStop::Right),
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
            ShownTab::Panels if self.panels.active == PanelSide::Left => TabStop::Left,
            ShownTab::Panels => TabStop::Right,
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

    pub(in crate::app) fn switch_tab(&mut self, direction: TabDirection) -> Task<Message> {
        let current = self.current_stop();
        let target = next_stop(self.open_pages(), current, direction);
        match target {
            TabStop::Left | TabStop::Right => {
                self.panels.active = if target == TabStop::Left {
                    PanelSide::Left
                } else {
                    PanelSide::Right
                };
                self.shown_tab = ShownTab::Panels;
            }
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
    use TabStop::{Hermes, Left, Monitor, Right, Settings};

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
    fn without_pages_the_panels_alternate() {
        assert_eq!(next_stop(NONE, Left, Forward), Right);
        assert_eq!(next_stop(NONE, Right, Forward), Left);
        assert_eq!(next_stop(NONE, Left, Backward), Right);
    }

    #[test]
    fn open_pages_come_after_the_right_panel_and_wrap_to_the_left() {
        assert_eq!(next_stop(ALL, Right, Forward), Settings);
        assert_eq!(next_stop(ALL, Settings, Forward), Monitor);
        assert_eq!(next_stop(ALL, Monitor, Forward), Hermes);
        assert_eq!(next_stop(ALL, Hermes, Forward), Left);
        assert_eq!(next_stop(ALL, Left, Backward), Hermes);
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
        assert_eq!(next_stop(monitor_only, Right, Forward), Monitor);
        assert_eq!(next_stop(chat_and_settings, Settings, Forward), Hermes);
        assert_eq!(next_stop(chat_and_settings, Left, Backward), Hermes);
        assert_eq!(next_stop(chat_and_settings, Hermes, Backward), Settings);
    }
}
