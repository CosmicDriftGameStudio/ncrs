use iced::Task;

/// Takes the keyboard focus from the field that has it, so the keys go to the
/// panels again.
pub fn release_focus<T: Send + 'static>() -> Task<T> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
}

/// Whether a text field has the keyboard, asked of the widgets themselves.
pub fn field_has_focus() -> Task<bool> {
    use iced::advanced::widget::operation::{focusable::Focusable, Operation, Outcome};
    use iced::advanced::widget::Id;
    use iced::Rectangle;

    struct Find {
        found: bool,
    }

    impl Operation<bool> for Find {
        fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
            self.found |= state.is_focused();
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<bool>)) {
            operate(self);
        }

        fn finish(&self) -> Outcome<bool> {
            Outcome::Some(self.found)
        }
    }

    iced::advanced::widget::operate(Find { found: false })
}
