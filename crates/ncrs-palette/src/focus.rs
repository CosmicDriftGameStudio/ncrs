use iced::Task;

/// Takes the keyboard focus from the field that has it, so the keys go to the
/// panels again.
pub fn release_focus<T: Send + 'static>() -> Task<T> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
}
