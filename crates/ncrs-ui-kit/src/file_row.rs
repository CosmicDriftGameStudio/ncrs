//! One row of a file list: the star, the cells of the shown columns and,
//! in a panel, the Hermes cell. Also the rows of a hit list.
use std::path::Path;

use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Row, Space};
use iced::{alignment, Element, Font, Length};
use ncrs_core::{Column, FileEntry, Selection};
use ncrs_i18n::{Language, Msg};

use crate::column_header::{column_alignment, ColumnLayout};
use crate::compare_cell::{blank_compare_cell, compare_cell, CompareCell};
use crate::file_icon::{file_icon, icon_color, icon_kind, note_marker, IconKind, SpecialFolder};
use crate::format;
use crate::hermes_state::{
    abbreviated, role_message, state_label, summary_items, FolderRole, FolderSummary,
    HermesFileState,
};
use crate::kit::{self, Icon};
use crate::layout::{
    FILE_ICON_SIZE, HERMES_CELL_GAP, HERMES_COLUMN_WIDTH, ROW_HEIGHT, TAG_COLUMN_WIDTH,
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
            .font(crate::APP_FONT)
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
    pub lang: Language,
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
        lang,
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
            compare: CompareCell::Off,
            hit_scope: Some((scope, height)),
            third_line,
            has_note: false,
            columns: ColumnLayout::standard(lang),
            special: None,
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

/// What the rows of a list differ in: the columns, the filing cell, the
/// symbol of a folder with a place of its own, and for a hit list the folder
/// the scope is measured from with the row height.
pub struct RowExtras<'a, M> {
    pub columns: ColumnLayout<'a>,
    /// What the caller knows about this row's folder from its path.
    pub special: Option<SpecialFolder>,
    pub hermes: Option<HermesCell<'a, M>>,
    /// The Compare column, which replaces the Hermes column.
    pub compare: CompareCell,
    pub hit_scope: Option<(&'a Path, f32)>,
    /// Under the folder of a hit.
    pub third_line: Option<Element<'a, M>>,
    /// A note belongs to this file: a small marker follows the name.
    pub has_note: bool,
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
        columns,
        special,
        hermes,
        compare,
        hit_scope,
        third_line,
        has_note,
    } = extras;
    let kind = icon_kind(entry, special);
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
            ..crate::APP_FONT
        }
    } else {
        crate::APP_FONT
    };

    let name = display_name(entry);

    let cell = |content: String| {
        text(content)
            .size(font_size::FILE)
            .font(font)
            .color(color)
            .wrapping(text::Wrapping::None)
    };

    let fitted = hermes.as_ref().map(|cell_of| {
        HermesCell::<M>::fit_tags(
            cell_of.tags(),
            cell_of.panel_width(),
            columns.fixed_width(),
            name.chars().count(),
        )
    });
    let shown_name = match hermes.as_ref() {
        Some(cell_of) if cell_of.has_tags() => format::ellipsize_middle(
            &name,
            HermesCell::<M>::name_room(cell_of.panel_width(), columns.fixed_width()),
        ),
        _ => name,
    };
    let note_color = if highlighted { color } else { palette.accent };
    let name_cell = row![cell(shown_name)]
        .extend(has_note.then(|| note_marker(note_color, font_size::FILE)))
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
                .font(crate::APP_FONT)
                .color(if highlighted { color } else { palette.dim_text })
                .wrapping(text::Wrapping::None),
        ]
        .into(),
        None => name_cell.into(),
    };
    let mut name_element: Option<Element<'a, M>> =
        Some(container(name_column).width(Length::Fill).clip(true).into());
    // On the cursor row and on a tagged row the symbol has the color of the name.
    let symbol_color = icon_color(
        kind,
        palette,
        color,
        highlighted || (mark.tagged && panel_active),
    );
    let cells: Vec<Element<'a, M>> = columns
        .shown()
        .map(|shown| match shown {
            Column::Name => name_element.take().unwrap_or_else(|| Space::new().into()),
            Column::Icon => container(file_icon(kind, symbol_color, FILE_ICON_SIZE))
                .width(columns.width_of(Column::Icon))
                .into(),
            other => container(
                cell(cell_text(entry, other, &columns, kind))
                    .width(Length::Fill)
                    .align_x(column_alignment(other)),
            )
            .width(columns.width_of(other))
            .clip(true)
            .into(),
        })
        .collect();
    // The third line runs under the cells too, beside the tag column.
    let mut content = match third_line {
        Some(third) => row![
            prefix,
            column![
                Row::with_children(cells).align_y(alignment::Vertical::Center),
                container(third).width(Length::Fill).clip(true),
            ]
            .width(Length::Fill),
        ],
        None => row![prefix].extend(cells),
    }
    .align_y(alignment::Vertical::Center);
    if let Some(status_cell) = hermes {
        content = content.push(status_cell.status());
    }
    match compare {
        CompareCell::Off => {}
        CompareCell::Blank => content = content.push(blank_compare_cell()),
        CompareCell::Mark(compared) => {
            let on_cursor = highlighted.then_some(color);
            content = content.push(compare_cell(compared, on_cursor, columns.lang, palette));
        }
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

/// What a cell besides the name and the symbol says. Unknown is empty.
pub fn cell_text(
    entry: &FileEntry,
    column: Column,
    layout: &ColumnLayout<'_>,
    kind: IconKind,
) -> String {
    let date = |time| format::time_in(time, layout.date_format, layout.lang);
    // `..` is only a way up: it has a size marker and nothing else.
    if entry.is_parent && column != Column::Size {
        return String::new();
    }
    match column {
        Column::Icon | Column::Name => String::new(),
        Column::Size => format::entry_size_in(entry, layout.size_format),
        Column::Modified => date(entry.modified),
        Column::Created => date(entry.details.created),
        Column::Accessed => date(entry.details.accessed),
        Column::Type => layout.lang.text(kind.type_message()).to_owned(),
        Column::Extension => format::extension(entry),
        Column::Permissions => format::permissions(entry),
        Column::Owner => entry.details.owner.clone().unwrap_or_default(),
        Column::Group => entry.details.group.clone().unwrap_or_default(),
        Column::LinkTarget => entry
            .details
            .link_target
            .as_ref()
            .map(|target| target.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
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
    fn name_room(panel_width: f32, columns_width: f32) -> usize {
        (Self::room_beside_name(panel_width, columns_width) / Self::name_char_width())
            .max(MIN_NAME_CHARS) as usize
    }

    const fn name_char_width() -> f32 {
        font_size::FILE * DIGIT_WIDTH_PER_SIZE
    }

    /// `columns_width` is what the columns besides the name take.
    fn room_beside_name(panel_width: f32, columns_width: f32) -> f32 {
        let fixed = TAG_COLUMN_WIDTH
            + columns_width
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
    fn fit_tags(
        tags: &ShownTags,
        panel_width: f32,
        columns_width: f32,
        name_chars: usize,
    ) -> ShownTags {
        let left = Self::room_beside_name(panel_width, columns_width)
            - name_chars as f32 * Self::name_char_width();
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

/// Display only. The identity is `entry.name` as the filesystem spells it;
/// this is the lossy form, computed here and thrown away. A name that is not
/// valid UTF-8 shows as U+FFFD, which is what the platform's own file
/// managers do. A link is checked first: a link to a folder is a folder too,
/// and would otherwise look like a real one.
fn display_name(entry: &FileEntry) -> String {
    let name = entry.name.to_string_lossy();
    if entry.is_symlink {
        format!("~{name}")
    } else if entry.is_dir && !entry.is_parent {
        format!("/{name}")
    } else {
        name.into_owned()
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
    use crate::layout::{DATE_COLUMN_WIDTH, SIZE_COLUMN_WIDTH};
    use crate::tags::FileChip;

    const STANDARD_COLUMNS: f32 = SIZE_COLUMN_WIDTH + DATE_COLUMN_WIDTH;

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
        let fitted = HermesCell::<()>::fit_tags(&tags, 1200.0, STANDARD_COLUMNS, 10);
        assert_eq!(fitted, tags);
    }

    #[test]
    fn a_long_name_keeps_its_text_and_the_chips_turn_into_a_count() {
        let tags = chips_of(&["invoice", "telekom", "2026"], 0);
        let room = HermesCell::<()>::name_room(600.0, STANDARD_COLUMNS);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, STANDARD_COLUMNS, room - 6);
        assert!(fitted.chips.len() < 3);
        assert_eq!(fitted.chips.len() + fitted.more, 3);
        assert!(fitted.more > 0);
    }

    #[test]
    fn a_link_to_a_folder_shows_as_a_link_not_as_a_folder() {
        let mut entry = FileEntry {
            name: "Dokumente".into(),
            path: "/home/marc/Dokumente".into(),
            is_dir: true,
            is_symlink: true,
            is_parent: false,
            size: 0,
            modified: None,
            folder_size: None,
            details: Default::default(),
        };
        assert_eq!(display_name(&entry), "~Dokumente");
        entry.is_symlink = false;
        assert_eq!(display_name(&entry), "/Dokumente");
    }

    #[test]
    fn the_cells_of_a_row_follow_the_columns() {
        let layout = ColumnLayout::standard(Language::English);
        let mut entry = FileEntry {
            name: "report.pdf".into(),
            path: "/r/report.pdf".into(),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 2048,
            modified: None,
            folder_size: None,
            details: Default::default(),
        };
        entry.details.owner = Some("marc".to_owned());
        entry.details.link_target = Some("/elsewhere".into());
        let text_of = |column| cell_text(&entry, column, &layout, IconKind::Pdf);
        assert_eq!(text_of(Column::Size), "2.0 KB");
        assert_eq!(text_of(Column::Modified), "");
        assert_eq!(text_of(Column::Type), "PDF");
        assert_eq!(text_of(Column::Extension), "pdf");
        assert_eq!(text_of(Column::Owner), "marc");
        assert_eq!(text_of(Column::Group), "");
        assert_eq!(text_of(Column::LinkTarget), "/elsewhere");

        entry.is_parent = true;
        let parent_text_of = |column| cell_text(&entry, column, &layout, IconKind::Pdf);
        assert_eq!(parent_text_of(Column::Owner), "");
        assert_eq!(parent_text_of(Column::Size), Msg::MarkerUp.text());
    }

    #[test]
    fn a_name_wider_than_the_room_has_no_chips() {
        let tags = chips_of(&["invoice", "2026"], 1);
        let room = HermesCell::<()>::name_room(600.0, STANDARD_COLUMNS);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, STANDARD_COLUMNS, room + 5);
        assert_eq!(fitted, ShownTags::default());
    }
}
