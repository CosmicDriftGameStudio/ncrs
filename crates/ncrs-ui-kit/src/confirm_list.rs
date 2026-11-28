//! What a delete dialog lists: a line per thing with its name, its folder, what was checked,
//! a bar and the size, and a sum line closing the bordered box.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, scrollable, text};
use iced::{alignment, Color, Element, Length, Padding};

use crate::format::ellipsize_middle;
use crate::layout::SIZE_COLUMN_WIDTH;
use crate::palette::Palette;
use crate::size_row::size_bar;
use crate::theme::{self, font_size, spacing};
use crate::APP_FONT_BOLD;

/// What fits after the name before the folder is cut in the middle.
const PATH_CHARS: usize = 34;
const CHECK_CELL: f32 = 200.0;
const LINE_HEIGHT: f32 = 40.0;
const BAR_CELL: f32 = 110.0;

#[derive(Debug, Clone)]
pub struct ConfirmLine {
    pub name: String,
    pub path: String,
    /// The first check that was made, shown with a tick.
    pub check: Option<String>,
    pub size_text: String,
    /// Against the biggest line, `0.0` to `1.0`.
    pub fraction: f32,
    pub color: Color,
}

/// The sum under the lines: its label and the size.
#[derive(Debug, Clone)]
pub struct ConfirmSum {
    pub label: String,
    pub size_text: String,
}

fn line_view<'a, M: 'a>(line: ConfirmLine, palette: &Palette) -> Element<'a, M> {
    let words = |content: String, font: f32, ink: Color| {
        text(content).size(font).color(ink).wrapping(Wrapping::None)
    };
    let check = line
        .check
        .map_or_else(String::new, |check| format!("\u{2713} {check}"));
    container(
        row![
            container(column![
                words(line.name, font_size::BODY, palette.text),
                words(
                    ellipsize_middle(&line.path, PATH_CHARS),
                    font_size::SMALL,
                    palette.dim_text,
                ),
            ])
            .width(Length::FillPortion(3))
            .clip(true),
            container(words(check, font_size::SMALL, palette.success))
                .width(CHECK_CELL)
                .clip(true),
            container(size_bar(line.fraction, line.color, palette)).width(BAR_CELL),
            container(words(line.size_text, font_size::BODY, palette.text))
                .width(SIZE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
        ]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center),
    )
    .height(LINE_HEIGHT)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The lines in a bordered box that scrolls at `scroll_height`; the sum stays under them.
pub fn confirm_list<'a, M: Clone + 'a>(
    lines: Vec<ConfirmLine>,
    sum: Option<ConfirmSum>,
    scroll_height: f32,
    palette: &Palette,
) -> Element<'a, M> {
    let body = container(column(
        lines.into_iter().map(|line| line_view(line, palette)),
    ))
    .padding(Padding {
        top: spacing::XS,
        bottom: spacing::XS,
        left: spacing::M,
        right: spacing::XL,
    });
    let mut inside = column![scrollable(body).height(scroll_height)];
    if let Some(sum) = sum {
        inside = inside
            .push(theme::horizontal_rule(palette.border_inactive))
            .push(
                container(
                    row![
                        text(sum.label)
                            .size(font_size::BODY)
                            .font(APP_FONT_BOLD)
                            .wrapping(Wrapping::None),
                        iced::widget::Space::new().width(Length::Fill),
                        text(sum.size_text)
                            .size(font_size::BODY)
                            .font(APP_FONT_BOLD)
                            .wrapping(Wrapping::None),
                    ]
                    .align_y(alignment::Vertical::Center),
                )
                .padding([spacing::XS, spacing::M]),
            );
    }
    container(inside)
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into()
}
