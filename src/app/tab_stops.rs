//! Tab and Shift+Tab walk through the open tabs: the left panel, the right
//! one, then the pages that stand in for the panels.

use crate::app::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TabStop {
    Left,
    Right,
    Settings,
    Monitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDirection {
    Forward,
    Backward,
}

/// The stop after `current` among the open ones; the panels are always open.
pub(in crate::app) fn next_stop(
    settings_open: bool,
    monitor_open: bool,
    current: TabStop,
    direction: TabDirection,
) -> TabStop {
    let open: Vec<TabStop> = [
        (true, TabStop::Left),
        (true, TabStop::Right),
        (settings_open, TabStop::Settings),
        (monitor_open, TabStop::Monitor),
    ]
    .into_iter()
    .filter_map(|(is_open, stop)| is_open.then_some(stop))
    .collect();
    let at = open.iter().position(|stop| *stop == current).unwrap_or(0);
    let next = match direction {
        TabDirection::Forward => (at + 1) % open.len(),
        TabDirection::Backward => (at + open.len() - 1) % open.len(),
    };
    open[next]
}

impl App {
    fn current_stop(&self) -> TabStop {
        if self.monitor.is_open() {
            TabStop::Monitor
        } else if self.settings.page.is_some() {
            TabStop::Settings
        } else if self.panels.active == PanelSide::Left {
            TabStop::Left
        } else {
            TabStop::Right
        }
    }

    pub(in crate::app) fn switch_tab(&mut self, direction: TabDirection) -> Task<Message> {
        let current = self.current_stop();
        let target = next_stop(
            self.settings.page.is_some(),
            self.monitor.is_open(),
            current,
            direction,
        );
        if target == current {
            return Task::none();
        }
        let side = match target {
            TabStop::Left => PanelSide::Left,
            TabStop::Right => PanelSide::Right,
            // Only one page is open at a time and it is the current stop.
            TabStop::Settings | TabStop::Monitor => return Task::none(),
        };
        self.panels.active = side;
        match current {
            TabStop::Monitor => self.update_monitor(MonitorMessage::Close),
            TabStop::Settings => self.leave_settings_page(),
            TabStop::Left | TabStop::Right => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TabDirection::{Backward, Forward};
    use TabStop::{Left, Monitor, Right, Settings};

    #[test]
    fn without_pages_the_panels_alternate() {
        assert_eq!(next_stop(false, false, Left, Forward), Right);
        assert_eq!(next_stop(false, false, Right, Forward), Left);
        assert_eq!(next_stop(false, false, Left, Backward), Right);
    }

    #[test]
    fn open_pages_come_after_the_right_panel_and_wrap_to_the_left() {
        assert_eq!(next_stop(true, true, Right, Forward), Settings);
        assert_eq!(next_stop(true, true, Settings, Forward), Monitor);
        assert_eq!(next_stop(true, true, Monitor, Forward), Left);
        assert_eq!(next_stop(true, true, Left, Backward), Monitor);
        assert_eq!(next_stop(true, true, Monitor, Backward), Settings);
    }

    #[test]
    fn a_closed_page_is_skipped() {
        assert_eq!(next_stop(false, true, Right, Forward), Monitor);
        assert_eq!(next_stop(true, false, Settings, Forward), Left);
        assert_eq!(next_stop(true, false, Left, Backward), Settings);
    }
}
