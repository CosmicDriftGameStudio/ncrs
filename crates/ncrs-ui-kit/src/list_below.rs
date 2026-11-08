//! A list that floats right below a field, over the content that follows it,
//! like the menu of a `pick_list`: the field keeps its place and size and
//! nothing around it moves.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::operation::Operation;
use iced::advanced::widget::{tree, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{mouse, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

struct ListBelow<'a, M> {
    field: Element<'a, M>,
    list: Element<'a, M>,
}

/// `field` as it is, with `list` floating below it as long as it is shown.
pub fn list_below<'a, M: 'a>(field: Element<'a, M>, list: Element<'a, M>) -> Element<'a, M> {
    Element::new(ListBelow { field, list })
}

// `children` is built by `children`/`diff` with exactly the field's tree
// first and the list's second.
#[allow(clippy::indexing_slicing)]
fn field_tree(tree: &Tree) -> &Tree {
    &tree.children[0]
}

#[allow(clippy::indexing_slicing)]
fn field_tree_mut(tree: &mut Tree) -> &mut Tree {
    &mut tree.children[0]
}

impl<M> Widget<M, Theme, Renderer> for ListBelow<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(self.field.as_widget()),
            Tree::new(self.list.as_widget()),
        ]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.field.as_widget(), self.list.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.field.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.field.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.field
            .as_widget_mut()
            .layout(field_tree_mut(tree), renderer, limits)
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
        self.field.as_widget().draw(
            field_tree(tree),
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.field
            .as_widget_mut()
            .operate(field_tree_mut(tree), layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.field.as_widget_mut().update(
            field_tree_mut(tree),
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        self.field.as_widget().mouse_interaction(
            field_tree(tree),
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let [_, list_tree] = tree.children.as_mut_slice() else {
            return None;
        };
        let field = Rectangle::new(layout.position() + translation, layout.bounds().size());
        Some(overlay::Element::new(Box::new(FloatingList {
            list: &mut self.list,
            tree: list_tree,
            field,
        })))
    }
}

struct FloatingList<'a, 'b, M> {
    list: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    field: Rectangle,
}

/// Where the list's top edge goes: below the field, or above it when the
/// window has no room below and more above.
fn list_top(field: Rectangle, list_height: f32, window_height: f32) -> f32 {
    let below = field.y + field.height;
    let fits_below = below + list_height <= window_height;
    let fits_above = field.y - list_height >= 0.0;
    if fits_below || !fits_above {
        below
    } else {
        field.y - list_height
    }
}

impl<M> overlay::Overlay<M, Theme, Renderer> for FloatingList<'_, '_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, Size::new(self.field.width, bounds.height));
        let node = self
            .list
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let top = list_top(self.field, node.size().height, bounds.height);
        node.move_to(Point::new(self.field.x, top))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.list.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        self.list.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.list.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> Rectangle {
        Rectangle::new(Point::new(10.0, 100.0), Size::new(200.0, 30.0))
    }

    #[test]
    fn the_list_goes_below_the_field() {
        assert_eq!(list_top(field(), 160.0, 600.0), 130.0);
    }

    #[test]
    fn near_the_bottom_it_goes_above_when_there_is_room() {
        let low = Rectangle::new(Point::new(10.0, 300.0), Size::new(200.0, 30.0));
        assert_eq!(list_top(low, 160.0, 400.0), 140.0);
    }

    #[test]
    fn without_room_either_way_it_stays_below() {
        assert_eq!(list_top(field(), 160.0, 200.0), 130.0);
    }
}
