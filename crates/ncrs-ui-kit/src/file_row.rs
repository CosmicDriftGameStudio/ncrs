//! One row of a file list: the star, the name with its chips, size, date and,
//! in a panel, the Hermes cell. Also the rows of a hit list.
use std::path::Path;

use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Row, Space};
use iced::{alignment, Element, Font, Length};
use ncrs_core::{FileEntry, Selection};
use ncrs_i18n::{Language, Msg};

use crate::format;
use crate::hermes_state::{
    abbreviated, role_message, state_label, summary_items, FolderRole, FolderSummary,
    HermesFileState,
};
use crate::kit::{self, Icon};
use crate::layout::{
    DATE_COLUMN_WIDTH, HERMES_CELL_GAP, HERMES_COLUMN_WIDTH, ROW_HEIGHT, SIZE_COLUMN_WIDTH,
    TAG_COLUMN_WIDTH,
};
use crate::palette::Palette;
use crate::tags::ShownTags;
use crate::theme::{self, font_size, size, spacing};

/// The star column of a row. Its own button so a click there tags the row
/// instead of moving the cursor.
pub fn tag_star<'a, M: Clone + 'a>(
    mark: Selection,
    panel_active: bool,
    palette: &Palette,
    row_height: f32,
    pressed: M,
) -> Element<'a, M> {
    button(
        text(if mark.tagged { "*" } else { " " })
            .size(font_size::FILE)
            .font(Font::MONOSPACE)
            .color(if mark.tagged {
                theme::tag_star_color(palette, mark.cursor && panel_active, panel_active)
            } else {
                palette.dim_text
            })
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fixed(TAG_COLUMN_WIDTH))
    .height(row_height)
    .style(theme::tag_button(palette, mark.tagged))
    .on_press(pressed)
    .into()
}

/// A file found by a search, drawn as a row of the hit list: the name, the
/// folder below `scope`, size and date, and optionally a third line.
pub struct HitRow<'a, M> {
    pub entry: FileEntry,
    pub mark: Selection,
    pub scope: &'a Path,
    pub height: f32,
    pub third_line: Option<Element<'a, M>>,
    /// What a right click on the row sends.
    pub on_context: Option<M>,
}

impl<M> std::fmt::Debug for HitRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitRow").finish_non_exhaustive()
    }
}

pub fn hit_row<'a, M: Clone + 'a>(
    hit: HitRow<'a, M>,
    panel_active: bool,
    palette: &Palette,
    on_click: impl Fn(RowClick) -> M,
    prefix: Row<'a, M>,
) -> Element<'a, M> {
    let HitRow {
        entry,
        mark,
        scope,
        height,
        third_line,
        on_context,
    } = hit;
    let row = file_row(
        &entry,
        mark,
        panel_active,
        palette,
        on_click,
        prefix,
        RowExtras {
            hermes: None,
            hit_scope: Some((scope, height)),
            third_line,
        },
    );
    match on_context {
        Some(message) => mouse_area(row).on_right_press(message).into(),
        None => row,
    }
}

/// What a click on a row of the panel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClick {
    /// The star in the tag column: toggles the tag.
    Tag,
    /// The rest of the row: moves the cursor there.
    Cursor,
    /// A double click on the row: opens it like Enter.
    Open,
}

/// What only some rows have: the filing cell, and for a hit list the folder
/// the scope is measured from with the row height.
pub struct RowExtras<'a, M> {
    pub hermes: Option<HermesCell<'a, M>>,
    pub hit_scope: Option<(&'a Path, f32)>,
    /// Under the folder of a hit.
    pub third_line: Option<Element<'a, M>>,
}

impl<M> std::fmt::Debug for RowExtras<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowExtras").finish_non_exhaustive()
    }
}

/// One file row: the name, size and date, clickable as a whole.
///
/// `prefix` is the tag button, built by the caller because it needs its own
/// message; the row itself is one button around the rest.
pub fn file_row<'a, M: Clone + 'a>(
    entry: &FileEntry,
    mark: Selection,
    panel_active: bool,
    palette: &Palette,
    on_click: impl Fn(RowClick) -> M,
    prefix: Row<'a, M>,
    extras: RowExtras<'a, M>,
) -> Element<'a, M> {
    let RowExtras {
        hermes,
        hit_scope,
        third_line,
    } = extras;
    // The cursor row of the active panel is the one drawn as highlighted; a
    // tagged row is marked in its own column and tinted by the row style.
    let highlighted = mark.cursor && panel_active;
    let color = theme::row_text_color(
        palette,
        entry.is_dir,
        highlighted,
        mark.tagged,
        panel_active,
    );
    let font = if entry.is_dir || (highlighted && panel_active) {
        Font {
            weight: iced::font::Weight::Bold,
            ..Font::MONOSPACE
        }
    } else {
        Font::MONOSPACE
    };

    // Display only. The identity is `entry.name` as the filesystem spells it;
    // this is the lossy form, computed here and thrown away. A name that is not
    // valid UTF-8 shows as U+FFFD, which is what the platform's own file
    // managers do.
    let name = if entry.is_dir && !entry.is_parent {
        format!("/{}", entry.name.to_string_lossy())
    } else if entry.is_symlink {
        format!("~{}", entry.name.to_string_lossy())
    } else {
        entry.name.to_string_lossy().into_owned()
    };

    let cell = |content: String| {
        text(content)
            .size(font_size::FILE)
            .font(font)
            .color(color)
            .wrapping(text::Wrapping::None)
    };

    let fitted = hermes.as_ref().map(|cell_of| {
        HermesCell::<M>::fit_tags(cell_of.tags(), cell_of.panel_width(), name.chars().count())
    });
    let shown_name = match hermes.as_ref() {
        Some(cell_of) if cell_of.has_tags() => {
            format::ellipsize_middle(&name, HermesCell::<M>::name_room(cell_of.panel_width()))
        }
        _ => name,
    };
    let name_cell = row![cell(shown_name)]
        .extend(hermes.as_ref().and_then(HermesCell::role_tag))
        .extend(
            hermes
                .as_ref()
                .zip(fitted.as_ref())
                .into_iter()
                .flat_map(|(cell_of, tags)| cell_of.tag_chips(tags, highlighted)),
        )
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    let name_column: Element<'a, M> = match hit_scope {
        Some((scope, _)) => column![
            name_cell,
            text(format::folder_below(&entry.path, scope))
                .size(font_size::SMALL)
                .font(Font::MONOSPACE)
                .color(if highlighted { color } else { palette.dim_text })
                .wrapping(text::Wrapping::None),
        ]
        .into(),
        None => name_cell.into(),
    };
    let name_element: Element<'a, M> = container(name_column).width(Length::Fill).clip(true).into();
    let size_cell: Element<'a, M> = cell(format::entry_size(entry))
        .width(SIZE_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Right)
        .into();
    let date_cell: Element<'a, M> = cell(format::time(entry.modified))
        .width(DATE_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Right)
        .into();
    // The third line runs under size and date too, beside the tag column.
    let mut content = match third_line {
        Some(third) => row![
            prefix,
            column![
                row![name_element, size_cell, date_cell].align_y(alignment::Vertical::Center),
                container(third).width(Length::Fill).clip(true),
            ]
            .width(Length::Fill),
        ],
        None => row![prefix, name_element, size_cell, date_cell],
    }
    .align_y(alignment::Vertical::Center);
    if let Some(status_cell) = hermes {
        content = content.push(status_cell.status());
    }

    // A button captures the press, so the double click is read inside it: the
    // area takes both presses and the button only draws the row.
    let clicks = mouse_area(content.width(Length::Fill))
        .on_press(on_click(RowClick::Cursor))
        .on_double_click(on_click(RowClick::Open));
    button(clicks)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(hit_scope.map_or(ROW_HEIGHT, |(_, height)| height))
        .width(Length::Fill)
        .style(theme::row(palette, mark.tagged, mark.cursor, panel_active))
        .into()
}

/// Width of a digit in the small font, in font sizes, to see what fits.
const DIGIT_WIDTH_PER_SIZE: f32 = 0.6;
const SUMMARY_GAP: f32 = 6.0;
const ICON_COUNT_GAP: f32 = 2.0;
const ROLE_TAG_FONT: f32 = 10.0;
/// Room kept free beside the cells of a row, for borders and rounding.
const NAME_SAFETY_MARGIN: f32 = 4.0;
/// A name is never cut shorter than this, whatever the chips need.
const MIN_NAME_CHARS: f32 = 8.0;
const NOT_COUNTED: &str = "…";

/// What the Hermes column shows for one row. Built from lookups only, so
/// drawing a row never reads the index.
pub struct HermesCell<'a, M> {
    content: HermesContent,
    role: Option<FolderRole>,
    lang: Language,
    palette: Palette,
    angle: f32,
    tags: ShownTags,
    panel_width: f32,
    _message: std::marker::PhantomData<&'a M>,
}

impl<M> std::fmt::Debug for HermesCell<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HermesCell").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum HermesContent {
    None,
    File(HermesFileState),
    Folder(Option<FolderSummary>),
}

impl<'a, M: 'a> HermesCell<'a, M> {
    pub fn new(
        content: HermesContent,
        role: Option<FolderRole>,
        lang: Language,
        palette: &Palette,
        angle: f32,
        tags: ShownTags,
        panel_width: f32,
    ) -> Self {
        Self {
            content,
            role,
            lang,
            palette: *palette,
            angle,
            tags,
            panel_width,
            _message: std::marker::PhantomData,
        }
    }

    fn tags(&self) -> &ShownTags {
        &self.tags
    }

    fn has_tags(&self) -> bool {
        !self.tags.chips.is_empty() || self.tags.more > 0
    }

    const fn panel_width(&self) -> f32 {
        self.panel_width
    }

    /// How many characters of the name fit in the row. The font of the name
    /// is monospaced, so this is exact enough; the row clips what is left.
    fn name_room(panel_width: f32) -> usize {
        (Self::room_beside_name(panel_width) / Self::name_char_width()).max(MIN_NAME_CHARS) as usize
    }

    const fn name_char_width() -> f32 {
        font_size::FILE * DIGIT_WIDTH_PER_SIZE
    }

    fn room_beside_name(panel_width: f32) -> f32 {
        let fixed = TAG_COLUMN_WIDTH
            + SIZE_COLUMN_WIDTH
            + DATE_COLUMN_WIDTH
            + HERMES_COLUMN_WIDTH
            + 2.0 * spacing::CELL_PADDING_X
            + NAME_SAFETY_MARGIN;
        panel_width - fixed
    }

    fn chip_width(label: &str) -> f32 {
        format::estimated_width(label, ROLE_TAG_FONT)
            + 2.0 * (spacing::XS + spacing::BORDER_WIDTH)
            + spacing::XS
    }

    /// The chips that fit behind a name of `name_chars` characters, in order.
    /// The name keeps its width; the chips left out are counted in `more`, and
    /// the count shows only when it fits as well.
    fn fit_tags(tags: &ShownTags, panel_width: f32, name_chars: usize) -> ShownTags {
        let left =
            Self::room_beside_name(panel_width) - name_chars as f32 * Self::name_char_width();
        let total = tags.chips.len() + tags.more;
        let fits = |kept: usize| {
            let chips: f32 = tags
                .chips
                .iter()
                .take(kept)
                .map(|chip| Self::chip_width(&chip.tag))
                .sum();
            let count = if total > kept {
                Self::chip_width(&format!("+{}", total - kept))
            } else {
                0.0
            };
            chips + count <= left
        };
        let fitting = (0..=tags.chips.len()).rev().find(|count| fits(*count));
        match fitting {
            Some(kept) => ShownTags {
                chips: tags.chips.iter().take(kept).cloned().collect(),
                more: total - kept,
            },
            None => ShownTags::default(),
        }
    }

    /// The tags of the file after its name, small, the ones the user set with
    /// an outline in the accent color.
    fn tag_chips(&self, tags: &ShownTags, on_fill: bool) -> Vec<Element<'a, M>> {
        let mut chips: Vec<Element<'a, M>> = tags
            .chips
            .iter()
            .map(|chip| {
                let label = text(chip.tag.clone())
                    .size(ROLE_TAG_FONT)
                    .wrapping(text::Wrapping::None);
                let boxed = container(label).padding([0.0, spacing::XS]);
                if chip.own {
                    boxed.style(theme::own_chip(&self.palette)).into()
                } else {
                    boxed.style(theme::chip(&self.palette, on_fill)).into()
                }
            })
            .collect();
        if tags.more > 0 {
            chips.push(
                text(format!("+{}", tags.more))
                    .size(ROLE_TAG_FONT)
                    .color(self.palette.dim_text)
                    .wrapping(text::Wrapping::None)
                    .into(),
            );
        }
        chips
    }

    /// A small outlined label after a folder's name.
    fn role_tag(&self) -> Option<Element<'a, M>> {
        let role = self.role?;
        let color = theme::hermes_role_color(&self.palette, role);
        let tag = container(
            text(self.lang.text(role_message(role)))
                .size(ROLE_TAG_FONT)
                .color(color)
                .wrapping(text::Wrapping::None),
        )
        .padding([0.0, spacing::XS])
        .style(theme::outlined_in(color));
        Some(match role {
            FolderRole::Archive => hint(
                tag.into(),
                self.lang
                    .text(Msg::HermesColumnRoleArchiveTooltip)
                    .to_owned(),
                &self.palette,
            ),
            FolderRole::Inbox | FolderRole::FilingLocation => tag.into(),
        })
    }

    /// The trailing cell, always [`HERMES_COLUMN_WIDTH`] wide and cut at its
    /// edge, so it never moves the size and date columns.
    fn status(self) -> Element<'a, M> {
        let inner: Element<'a, M> = match &self.content {
            HermesContent::None => Space::new().into(),
            HermesContent::File(state) => self.state_icon(*state, true),
            HermesContent::Folder(summary) => self.summary(summary.as_ref()),
        };
        container(inner)
            .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
            .width(HERMES_COLUMN_WIDTH)
            .clip(true)
            .into()
    }

    fn state_icon(&self, state: HermesFileState, with_hint: bool) -> Element<'a, M> {
        let color = theme::hermes_state_color(&self.palette, state);
        let glyph = match state {
            HermesFileState::Unread => kit::icon(Icon::Clock, color),
            HermesFileState::Planned => kit::icon(Icon::CircleArrowRight, color),
            HermesFileState::Reading => kit::icon_rotated(Icon::Loader, color, self.angle),
            HermesFileState::Filed => kit::icon(Icon::CircleCheck, color),
            HermesFileState::Inquiry => kit::icon(Icon::CircleHelp, color),
            HermesFileState::Error => kit::icon(Icon::TriangleAlert, color),
            HermesFileState::Excluded => kit::icon(Icon::Ban, color),
        };
        if with_hint {
            hint(
                glyph,
                state_label(state, self.lang).to_owned(),
                &self.palette,
            )
        } else {
            glyph
        }
    }

    fn summary(&self, counted: Option<&FolderSummary>) -> Element<'a, M> {
        let small = |content: String, color| {
            text(content)
                .size(font_size::SMALL)
                .color(color)
                .wrapping(text::Wrapping::None)
        };
        let Some(summary) = counted else {
            return hint(
                small(NOT_COUNTED.to_owned(), self.palette.dim_text).into(),
                self.lang.text(Msg::HermesColumnNotCounted).to_owned(),
                &self.palette,
            );
        };
        if summary.excluded {
            let color = theme::hermes_state_color(&self.palette, HermesFileState::Excluded);
            return row![
                self.state_icon(HermesFileState::Excluded, false),
                small(
                    self.lang.text(Msg::HermesColumnExcludedWord).to_owned(),
                    color
                ),
            ]
            .spacing(ICON_COUNT_GAP)
            .align_y(alignment::Vertical::Center)
            .into();
        }
        let items = summary_items(summary);
        if items.is_empty() {
            return Space::new().into();
        }
        let room = HERMES_COLUMN_WIDTH - HERMES_CELL_GAP;
        let ellipsis_width = font_size::SMALL * DIGIT_WIDTH_PER_SIZE + SUMMARY_GAP;
        let mut used = 0.0;
        let mut shown = Row::new()
            .spacing(SUMMARY_GAP)
            .align_y(alignment::Vertical::Center);
        let total = items.len();
        for (index, item) in items.iter().enumerate() {
            let count = abbreviated(item.count);
            let width = size::ICON
                + ICON_COUNT_GAP
                + count.chars().count() as f32 * font_size::SMALL * DIGIT_WIDTH_PER_SIZE
                + SUMMARY_GAP;
            let reserve = if index + 1 < total {
                ellipsis_width
            } else {
                0.0
            };
            if used + width + reserve > room {
                shown = shown.push(small(NOT_COUNTED.to_owned(), self.palette.dim_text));
                break;
            }
            used += width;
            let color = theme::hermes_state_color(&self.palette, item.state);
            shown = shown.push(hint(
                row![self.state_icon(item.state, false), small(count, color)]
                    .spacing(ICON_COUNT_GAP)
                    .align_y(alignment::Vertical::Center)
                    .into(),
                state_label(item.state, self.lang).to_owned(),
                &self.palette,
            ));
        }
        shown.into()
    }
}

/// `content` with `label` in a small box on hover.
fn hint<'a, M: 'a>(content: Element<'a, M>, label: String, palette: &Palette) -> Element<'a, M> {
    tooltip(
        content,
        container(
            text(label)
                .size(font_size::SMALL)
                .wrapping(text::Wrapping::None),
        )
        .padding([2.0, 6.0])
        .style(theme::dialog(palette)),
        tooltip::Position::Top,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::FileChip;

    fn chips_of(labels: &[&str], more: usize) -> ShownTags {
        ShownTags {
            chips: labels
                .iter()
                .map(|label| FileChip {
                    tag: (*label).to_owned(),
                    own: false,
                })
                .collect(),
            more,
        }
    }

    #[test]
    fn a_short_name_keeps_all_its_chips() {
        let tags = chips_of(&["invoice", "telekom", "2026"], 0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 1200.0, 10);
        assert_eq!(fitted, tags);
    }

    #[test]
    fn a_long_name_keeps_its_text_and_the_chips_turn_into_a_count() {
        let tags = chips_of(&["invoice", "telekom", "2026"], 0);
        let room = HermesCell::<()>::name_room(600.0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, room - 6);
        assert!(fitted.chips.len() < 3);
        assert_eq!(fitted.chips.len() + fitted.more, 3);
        assert!(fitted.more > 0);
    }

    #[test]
    fn a_name_wider_than_the_room_has_no_chips() {
        let tags = chips_of(&["invoice", "2026"], 1);
        let room = HermesCell::<()>::name_room(600.0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, room + 5);
        assert_eq!(fitted, ShownTags::default());
    }
}
