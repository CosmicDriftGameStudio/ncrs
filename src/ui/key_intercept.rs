//! A wrapper that claims a key press for the app before the text input inside
//! it can type its character.
//!
//! A text input inserts the `text` of a press and captures the event, so no
//! subscription ever sees Option+C on a Mac ("ç"). The wrapper looks first, and
//! only while the input inside has the focus: every field of the view is
//! wrapped, and the press must act once.
use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::operation::{Focusable, Operation};
use iced::advanced::widget::{tree, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::keyboard::{self, key::Physical, Key, Modifiers};
use iced::{mouse, Element, Event, Length, Rectangle, Size, Vector};

/// What a press means to the app, or `None` when the input may have it.
type Resolver<'a, Message> = Box<dyn Fn(&Key, Physical, Modifiers) -> Option<Message> + 'a>;

struct KeyIntercept<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    resolve: Resolver<'a, Message>,
}

/// Wraps `content`, a text input: a press that `resolve` maps to a message
/// publishes it instead of typing, once per press (not for a key repeat).
pub fn key_intercept<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    resolve: impl Fn(&Key, Physical, Modifiers) -> Option<Message> + 'a,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    Element::new(KeyIntercept {
        content: content.into(),
        resolve: Box::new(resolve),
    })
}

/// Asks the focusable widgets inside whether one of them has the focus.
#[derive(Default)]
struct FocusProbe {
    focused: bool,
}

impl Operation for FocusProbe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        self.focused |= state.is_focused();
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for KeyIntercept<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    // Everything but `update` is the input's own, and the tree is passed
    // through untouched: a different tree shape would lose the input's state,
    // and with it the focus.
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            repeat,
            ..
        }) = event
        {
            if let Some(message) = (self.resolve)(key, *physical_key, *modifiers) {
                let mut probe = FocusProbe::default();
                self.content
                    .as_widget_mut()
                    .operate(tree, layout, renderer, &mut probe);
                if probe.focused {
                    // A held key must not flip the toggle back and forth, but
                    // it must not type the character either.
                    if !*repeat {
                        shell.publish(message);
                    }
                    shell.capture_event();
                    return;
                }
            }
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
