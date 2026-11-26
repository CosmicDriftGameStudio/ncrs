//! A line of a file with the commit it comes from at the left.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Space};
use iced::{alignment, Color, Element, Length, Padding};

use crate::format::{ellipsize_end, estimated_width};
use crate::git_rows::{code_text, number_text};
use crate::palette::Palette;
use crate::theme::{self, font_size, git, size, spacing};

const SHA_WIDTH: f32 = 64.0;
const AUTHOR_WIDTH: f32 = 120.0;
const DATE_WIDTH: f32 = 130.0;

#[derive(Debug)]
pub struct BlameRow<M> {
    pub sha_short: String,
    pub author: String,
    pub date: String,
    pub line_no: u32,
    pub text: String,
    /// The first line of a run of one commit: only it names the commit.
    pub first_of_group: bool,
    pub chosen: bool,
    pub on_sha_press: M,
    pub on_press: M,
}

fn cell<'a, M: 'a>(content: String, width: f32, ink: Color) -> Element<'a, M> {
    container(
        text(content)
            .size(font_size::SMALL)
            .color(ink)
            .wrapping(Wrapping::None),
    )
    .width(width)
    .clip(true)
    .into()
}

fn author_that_fits(author: &str) -> String {
    let taken = estimated_width(author, font_size::SMALL);
    if taken <= AUTHOR_WIDTH {
        return author.to_owned();
    }
    let chars = author.chars().count();
    ellipsize_end(author, (chars as f32 * AUTHOR_WIDTH / taken) as usize)
}

fn commit_cells<'a, M: Clone + 'a>(
    line: &BlameRow<M>,
    (ink, dim): (Color, Color),
    palette: &Palette,
) -> Element<'a, M> {
    let link = if line.chosen {
        ink
    } else {
        git::purple(palette)
    };
    let sha = button(cell(line.sha_short.clone(), SHA_WIDTH, link))
        .padding(0.0)
        .on_press(line.on_sha_press.clone())
        .style(move |_theme: &iced::Theme, _status| button::Style {
            text_color: link,
            ..button::Style::default()
        });
    row![
        sha,
        cell(author_that_fits(&line.author), AUTHOR_WIDTH, ink),
        cell(line.date.clone(), DATE_WIDTH, dim),
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The commit of the line (short SHA, author, date), the line number and the
/// text. A line of its own above the first line of a commit sets the runs apart.
pub fn blame_row<'a, M: Clone + 'a>(line: BlameRow<M>, palette: &Palette) -> Element<'a, M> {
    let (ink, dim) = if line.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let commit: Element<'a, M> = if line.first_of_group {
        commit_cells(&line, (ink, dim), palette)
    } else {
        Space::new()
            .width(SHA_WIDTH + AUTHOR_WIDTH + DATE_WIDTH + 2.0 * spacing::XS)
            .into()
    };
    let height = if line.first_of_group {
        size::ROW - spacing::BORDER_WIDTH
    } else {
        size::ROW
    };
    let content = button(
        container(
            row![
                commit,
                number_text(Some(line.line_no), dim),
                code_text(line.text, ink),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
        )
        .padding(Padding {
            left: spacing::CELL_PADDING_X,
            right: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .clip(true)
        .center_y(Length::Fill),
    )
    .on_press(line.on_press)
    .width(Length::Fill)
    .height(height)
    .padding(0.0)
    .style(theme::menu_item(palette, line.chosen));
    if line.first_of_group {
        column![theme::horizontal_rule(palette.border_inactive), content].into()
    } else {
        content.into()
    }
}
