//! A compact button that opens a list of entries. Unlike `pick_list`, whose
//! list is exactly as wide as the field, the list is as wide as its widest
//! entry and is moved left when it would leave the window.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::mouse;
use iced::widget::overlay::menu;
use iced::widget::pick_list;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{
    alignment, Element, Event, Length, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme,
    Vector,
};

use crate::messages::Message;

/// One row of the list and what picking it sends.
#[derive(Debug, Clone)]
pub struct Entry {
    pub label: String,
    pub position: usize,
    pub message: Message,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

type FieldStyle<'a> = Box<dyn Fn(&Theme, pick_list::Status) -> pick_list::Style + 'a>;

/// Width of the open list: its widest entry with the padding of a row, but
/// never narrower than the button.
fn list_width(widest_entry: f32, pad_x: f32, button_width: f32) -> f32 {
    (widest_entry.ceil() + 2.0 * pad_x).max(button_width)
}

/// Left edge of the open list. It starts at the button; when it would pass
/// the right edge of the window it ends at the right edge of the button
/// instead, and never starts left of the window.
fn list_left(button_left: f32, button_width: f32, list_width: f32, window_width: f32) -> f32 {
    if button_left + list_width <= window_width {
        button_left
    } else {
        (button_left + button_width - list_width).max(0.0)
    }
}

#[derive(Default)]
struct State {
    menu: menu::State,
    is_open: bool,
    hovered_option: Option<usize>,
    widest_entry: f32,
    last_status: Option<pick_list::Status>,
}

pub struct MenuButton<'a> {
    entries: Vec<Entry>,
    caption: String,
    width: f32,
    height: f32,
    padding: Padding,
    text_size: f32,
    field_style: FieldStyle<'a>,
    menu_class: <Theme as menu::Catalog>::Class<'a>,
}

impl std::fmt::Debug for MenuButton<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuButton")
            .field("caption", &self.caption)
            .finish_non_exhaustive()
    }
}

impl<'a> MenuButton<'a> {
    pub fn new(
        entries: Vec<Entry>,
        caption: String,
        size: Size,
        padding: Padding,
        text_size: f32,
        field_style: impl Fn(&Theme, pick_list::Status) -> pick_list::Style + 'a,
        menu_style: impl Fn(&Theme) -> menu::Style + 'a,
    ) -> Self {
        Self {
            entries,
            caption,
            width: size.width,
            height: size.height,
            padding,
            text_size,
            field_style: Box::new(field_style),
            menu_class: Box::new(menu_style) as menu::StyleFn<'a, Theme>,
        }
    }
}

fn text_width(renderer: &Renderer, content: &str, size: f32) -> f32 {
    <Renderer as text::Renderer>::Paragraph::with_text(Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: LineHeight::default(),
        font: renderer.default_font(),
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::default(),
        wrapping: Wrapping::None,
    })
    .min_bounds()
    .width
}

impl Widget<Message, Theme, Renderer> for MenuButton<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Fixed(self.height))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        state.widest_entry = self
            .entries
            .iter()
            .map(|entry| text_width(renderer, &entry.label, self.text_size))
            .fold(0.0, f32::max);
        let size = limits
            .width(Length::Fixed(self.width))
            .height(Length::Fixed(self.height))
            .resolve(
                Length::Fixed(self.width),
                Length::Fixed(self.height),
                Size::ZERO,
            );
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(iced::touch::Event::FingerPressed { .. })
        ) {
            if state.is_open {
                // The list did not take the press: it hit outside the list.
                state.is_open = false;
                shell.capture_event();
            } else if cursor.is_over(layout.bounds()) {
                state.is_open = true;
                state.hovered_option = None;
                shell.capture_event();
            }
        }
        let hovered = cursor.is_over(layout.bounds());
        let status = if state.is_open {
            pick_list::Status::Opened {
                is_hovered: hovered,
            }
        } else if hovered {
            pick_list::Status::Hovered
        } else {
            pick_list::Status::Active
        };
        if state.last_status.is_some_and(|last| last != status) {
            shell.request_redraw();
        }
        state.last_status = Some(status);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let hovered = cursor.is_over(bounds);
        let status = if state.is_open {
            pick_list::Status::Opened {
                is_hovered: hovered,
            }
        } else if hovered {
            pick_list::Status::Hovered
        } else {
            pick_list::Status::Active
        };
        let style = (self.field_style)(theme, status);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: style.border,
                ..renderer::Quad::default()
            },
            style.background,
        );
        renderer.fill_text(
            Text {
                content: self.caption.clone(),
                size: Pixels(self.text_size),
                line_height: LineHeight::Absolute(Pixels(bounds.height - self.padding.y())),
                font: renderer.default_font(),
                bounds: Size::new(
                    bounds.width - self.padding.x(),
                    bounds.height - self.padding.y(),
                ),
                align_x: text::Alignment::Default,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::default(),
                wrapping: Wrapping::default(),
            },
            Point::new(bounds.x + self.padding.left, bounds.center_y()),
            style.placeholder_color,
            *viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.is_open {
            return None;
        }
        let bounds = layout.bounds();
        let width = list_width(state.widest_entry, self.padding.left, bounds.width);
        let menu = menu::Menu::new(
            &mut state.menu,
            &self.entries,
            &mut state.hovered_option,
            |entry: Entry| {
                state.is_open = false;
                entry.message
            },
            None,
            &self.menu_class,
        )
        .width(width)
        .padding(self.padding)
        .font(renderer.default_font())
        .text_size(self.text_size);
        let anchor = layout.position() + translation;
        // Placed at x = 0 so that the list may use the whole window width;
        // `AnchoredList` moves it to its real left edge.
        let inner = menu.overlay(
            Point::new(0.0, anchor.y),
            *viewport,
            bounds.height,
            Length::Shrink,
        );
        Some(overlay::Element::new(Box::new(AnchoredList {
            inner,
            button_left: anchor.x,
            button_width: bounds.width,
        })))
    }
}

struct AnchoredList<'a> {
    inner: overlay::Element<'a, Message, Theme, Renderer>,
    button_left: f32,
    button_width: f32,
}

impl overlay::Overlay<Message, Theme, Renderer> for AnchoredList<'_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.inner.as_overlay_mut().layout(renderer, bounds);
        let left = list_left(
            self.button_left,
            self.button_width,
            node.size().width,
            bounds.width,
        );
        let top = node.bounds().y;
        node.move_to(Point::new(left, top))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.inner
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, shell);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }
}

impl<'a> From<MenuButton<'a>> for Element<'a, Message> {
    fn from(button: MenuButton<'a>) -> Self {
        Element::new(button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_is_as_wide_as_its_widest_entry() {
        assert_eq!(list_width(100.2, 9.0, 28.0), 101.0 + 18.0);
    }

    #[test]
    fn list_is_never_narrower_than_the_button() {
        assert_eq!(list_width(4.0, 9.0, 60.0), 60.0);
    }

    #[test]
    fn list_starts_at_the_button_when_it_fits() {
        assert_eq!(list_left(100.0, 28.0, 120.0, 1200.0), 100.0);
    }

    #[test]
    fn list_at_the_right_edge_ends_at_the_button() {
        let left = list_left(1172.0, 28.0, 120.0, 1200.0);
        assert_eq!(left, 1080.0);
        assert_eq!(left + 120.0, 1200.0);
    }

    #[test]
    fn list_stays_inside_a_narrow_window() {
        assert_eq!(list_left(10.0, 28.0, 120.0, 100.0), 0.0);
    }
}
