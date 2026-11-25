//! A text pattern with the building blocks that can be added to it.

use iced::widget::{column, Row};
use iced::{alignment, Element};

use crate::palette::Palette;
use crate::theme::spacing;

use super::{button, dim, error_line, mono_text_field, Tone};

fn token_buttons<'a, M: Clone + 'a>(
    tokens: &[(&str, M)],
    palette: &Palette,
) -> Vec<Element<'a, M>> {
    tokens
        .iter()
        .map(|(label, on_press)| button(label, Some(on_press.clone()), Tone::Secondary, palette))
        .collect()
}

/// `label` dimmed, then one button per building block; a press sends the
/// block's message.
pub fn token_row<'a, M: Clone + 'a>(
    label: &str,
    tokens: &[(&str, M)],
    palette: &Palette,
) -> Row<'a, M> {
    let line = Row::new()
        .push(dim(label.to_owned(), palette))
        .spacing(spacing::LINE_GAP)
        .align_y(alignment::Vertical::Center);
    token_buttons(tokens, palette)
        .into_iter()
        .fold(line, |line, token| line.push(token))
}

impl<M> std::fmt::Debug for PatternField<'_, '_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternField")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

pub struct PatternField<'a, 't, M> {
    pub value: &'a str,
    pub id: &'static str,
    pub on_input: Box<dyn Fn(String) -> M + 'a>,
    pub on_submit: M,
    /// The blocks that can be added: label and what pressing it sends.
    pub tokens: &'t [(&'t str, M)],
    /// What the pattern makes of an example, dimmed under the blocks.
    pub preview: Option<String>,
    /// What is wrong with the pattern, in the error color under the preview.
    pub problem: Option<String>,
}

/// A pattern as a monospace text field, the blocks that can be added to it
/// (they wrap), the example it produces and what is wrong with it.
pub fn pattern_field<'a, 't, M: Clone + 'a>(
    field: PatternField<'a, 't, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut lines = column![
        mono_text_field(
            "",
            field.value,
            field.id,
            Some(field.on_input),
            field.on_submit,
            palette,
        ),
        Row::with_children(token_buttons(field.tokens, palette))
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center)
            .wrap()
            .vertical_spacing(spacing::XS),
    ]
    .spacing(spacing::XS);
    if let Some(example) = field.preview {
        lines = lines.push(dim(example, palette));
    }
    if let Some(problem) = field.problem {
        lines = lines.push(error_line(problem, palette));
    }
    lines.into()
}
