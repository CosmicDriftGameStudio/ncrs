//! A tab of the title bar: rules at its sides, a line on top, the label, and
//! optionally a cross that closes it and a small mark after the label.

use iced::widget::{button, column, container, row, text, Space};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

const CLOSE_CROSS: &str = "×";
/// Space between the label and what follows it.
pub const TAB_LABEL_GAP: f32 = spacing::M;
/// Space between the tab edge and its label.
pub const TAB_PADDING_X: f32 = spacing::L;

/// What a title bar tab shows and does. `id` anchors overlays to the tab.
#[derive(Debug)]
pub struct TitleTab<'a, M> {
    id: &'static str,
    label: String,
    active: bool,
    on_press: M,
    on_close: Option<M>,
    mark: Option<String>,
    left_edge: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

/// A tab showing `label`; a press selects it, the cross (when `on_close` is
/// given) closes it.
pub fn title_tab<'a, M: Clone + 'a>(
    id: &'static str,
    label: String,
    active: bool,
    on_press: M,
    on_close: Option<M>,
) -> TitleTab<'a, M> {
    TitleTab {
        id,
        label,
        active,
        on_press,
        on_close,
        mark: None,
        left_edge: false,
        _marker: std::marker::PhantomData,
    }
}

impl<'a, M: Clone + 'a> TitleTab<'a, M> {
    /// A dim mark after the label (and the cross).
    #[must_use]
    pub fn mark(mut self, mark: String) -> Self {
        self.mark = Some(mark);
        self
    }

    /// Draws a rule on the left side too.
    #[must_use]
    pub fn left_edge(mut self, left_edge: bool) -> Self {
        self.left_edge = left_edge;
        self
    }

    pub fn view(self, palette: &Palette) -> Element<'a, M> {
        let mut label = row![text(self.label)
            .size(font_size::FILE)
            .font(crate::APP_FONT)
            .width(Length::Fill)
            .wrapping(text::Wrapping::None)]
        .spacing(TAB_LABEL_GAP)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center);
        if let Some(on_close) = self.on_close {
            label = label.push(close_cross(on_close, palette));
        }
        if let Some(mark) = self.mark {
            label = label.push(text(mark).size(font_size::SMALL).color(palette.dim_text));
        }
        let content = button(label)
            .on_press(self.on_press)
            .padding([0.0, TAB_PADDING_X])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(theme::title_tab(palette, self.active));
        let left_edge: Element<'a, M> = if self.left_edge {
            theme::vertical_rule(palette.border_inactive)
        } else {
            Space::new().width(0.0).into()
        };
        container(row![
            left_edge,
            column![theme::tab_top_line(palette, self.active), content],
            theme::vertical_rule(palette.border_inactive),
        ])
        .id(self.id)
        .width(size::TAB_WIDTH)
        .height(Length::Fill)
        .into()
    }
}

fn close_cross<'a, M: Clone + 'a>(on_press: M, palette: &Palette) -> Element<'a, M> {
    button(text(CLOSE_CROSS).size(font_size::FILE))
        .on_press(on_press)
        .padding([0.0, spacing::XS])
        .style(theme::close_button(palette))
        .into()
}
