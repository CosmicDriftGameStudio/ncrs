//! The tabs of the title bar side by side. A few tabs at the start are pinned
//! and always show. The others share the rest evenly, shrink down to a
//! minimum width, and past that show a window of whole tabs with an arrow at
//! each end that has more behind it.

use iced::widget::{button, container, row, text, Space};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, size};

const ARROW_LEFT: &str = "‹";
const ARROW_RIGHT: &str = "›";

/// How a strip of tabs is laid out in the room it has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripLayout {
    /// The width of every pinned tab.
    pub pinned_width: f32,
    /// The width of every other tab.
    pub tab_width: f32,
    /// How many of the other tabs show at once.
    pub visible: usize,
    /// The index (among the other tabs) of the first one that shows.
    pub first: usize,
    /// How many other tabs there are.
    pub others: usize,
}

impl TabStripLayout {
    /// Not all tabs fit even at the minimum width: the strip shows a window.
    pub fn overflows(&self) -> bool {
        self.visible < self.others
    }
}

/// Lays out `pinned` pinned tabs and `others` more in `available` pixels.
/// `active` is the index among the others of the tab that has to show, if one
/// of them is active; `requested_first` is where the window starts when it can
/// stay there.
pub fn tab_strip_layout(
    pinned: usize,
    others: usize,
    available: f32,
    active: Option<usize>,
    requested_first: usize,
) -> TabStripLayout {
    let total = pinned + others;
    let even = available / total.max(1) as f32;
    if even >= size::TAB_MIN_WIDTH {
        let width = even.min(size::TAB_WIDTH);
        return TabStripLayout {
            pinned_width: width,
            tab_width: width,
            visible: others,
            first: 0,
            others,
        };
    }
    let arrows = 2.0 * size::TAB_ARROW;
    let pinned_width = if pinned == 0 {
        size::TAB_WIDTH
    } else {
        ((available - arrows - size::TAB_MIN_WIDTH) / pinned as f32)
            .clamp(size::TAB_MIN_WIDTH, size::TAB_WIDTH)
    };
    let room = (available - pinned as f32 * pinned_width - arrows).max(size::TAB_MIN_WIDTH);
    let visible = ((room / size::TAB_MIN_WIDTH).floor() as usize).clamp(1, others.max(1));
    let mut first = requested_first.min(others.saturating_sub(visible));
    if let Some(shown) = active {
        if shown < first {
            first = shown;
        } else if shown >= first + visible {
            first = shown + 1 - visible;
        }
    }
    TabStripLayout {
        pinned_width,
        tab_width: (room / visible as f32).min(size::TAB_WIDTH),
        visible,
        first,
        others,
    }
}

/// The `pinned` tabs, then the `others`, built at the widths of `layout`. An
/// arrow press sends the index (among the others) the window should start at.
pub fn tab_strip<'a, M: Clone + 'a>(
    layout: TabStripLayout,
    pinned: Vec<Element<'a, M>>,
    others: Vec<Element<'a, M>>,
    on_first: impl Fn(usize) -> M,
    palette: &Palette,
) -> Element<'a, M> {
    let mut strip = row![].height(Length::Fill);
    for tab in pinned {
        strip = strip.push(tab);
    }
    if layout.overflows() {
        let end = layout.first + layout.visible;
        strip = strip.push(arrow(
            ARROW_LEFT,
            (layout.first > 0).then(|| on_first(layout.first - 1)),
            palette,
        ));
        for tab in others.into_iter().skip(layout.first).take(layout.visible) {
            strip = strip.push(tab);
        }
        strip = strip.push(arrow(
            ARROW_RIGHT,
            (end < layout.others).then(|| on_first(layout.first + 1)),
            palette,
        ));
    } else {
        for tab in others {
            strip = strip.push(tab);
        }
    }
    container(strip)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}

fn arrow<'a, M: Clone + 'a>(
    glyph: &'static str,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let content: Element<'a, M> = match on_press {
        Some(message) => button(
            text(glyph)
                .size(font_size::TITLE)
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill),
        )
        .on_press(message)
        .padding(0.0)
        .width(size::TAB_ARROW)
        .height(size::BUTTON)
        .style(theme::icon_button(palette))
        .into(),
        None => Space::new().width(size::TAB_ARROW).into(),
    };
    container(content)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_tabs_keep_their_full_width() {
        let layout = tab_strip_layout(2, 1, 1000.0, Some(0), 0);
        assert_eq!(layout.tab_width, size::TAB_WIDTH);
        assert_eq!(layout.pinned_width, size::TAB_WIDTH);
        assert!(!layout.overflows());
    }

    #[test]
    fn many_tabs_share_the_room_evenly() {
        let layout = tab_strip_layout(2, 8, 1200.0, Some(7), 0);
        assert_eq!(layout.tab_width, 120.0);
        assert_eq!(layout.pinned_width, 120.0);
        assert!(!layout.overflows());
    }

    #[test]
    fn beyond_the_minimum_the_pinned_tabs_stay_full_and_the_others_scroll() {
        let layout = tab_strip_layout(2, 17, 1400.0, Some(16), 0);
        assert!(layout.overflows());
        assert_eq!(layout.pinned_width, size::TAB_WIDTH);
        assert!(layout.tab_width >= size::TAB_MIN_WIDTH);
        assert_eq!(layout.first + layout.visible, 17);
        let back = tab_strip_layout(2, 17, 1400.0, Some(1), 9);
        assert_eq!(back.first, 1);
    }

    #[test]
    fn the_window_stays_where_it_was_put_while_the_active_tab_shows() {
        let layout = tab_strip_layout(2, 17, 1400.0, Some(5), 4);
        assert_eq!(layout.first, 4);
        let pinned_active = tab_strip_layout(2, 17, 1400.0, None, 4);
        assert_eq!(pinned_active.first, 4);
    }

    #[test]
    fn pinned_tabs_shrink_only_when_the_others_would_get_less_than_the_minimum() {
        let layout = tab_strip_layout(2, 17, 600.0, Some(3), 0);
        assert!(layout.pinned_width < size::TAB_WIDTH);
        assert!(layout.pinned_width >= size::TAB_MIN_WIDTH);
    }
}
