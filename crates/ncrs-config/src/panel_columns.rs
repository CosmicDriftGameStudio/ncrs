//! Which columns the file panels show and how they are written, and the font
//! of the whole window: the settings as they are in effect. Pure; the panels
//! draw the result.

use ncrs_core::{Column, PanelSide};
pub use ncrs_core::{DateFormat, SizeFormat};

/// The smallest and the largest font size a config may ask for, in points.
pub const FONT_POINTS: std::ops::RangeInclusive<u8> = 10..=20;

/// The `[columns]` setting as it is in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelColumns {
    pub same_in_both: bool,
    /// In display order; always holds `Column::Name`. The columns this system
    /// cannot show stay in the list, the panel leaves them out.
    pub left: Vec<Column>,
    pub right: Vec<Column>,
    /// Pixels; a column without an entry has its standard width.
    pub widths: Vec<(Column, u16)>,
    pub date_format: DateFormat,
    pub size_format: SizeFormat,
}

impl Default for PanelColumns {
    fn default() -> Self {
        Self {
            same_in_both: true,
            left: Column::DEFAULT.to_vec(),
            right: Column::DEFAULT.to_vec(),
            widths: Vec::new(),
            date_format: DateFormat::default(),
            size_format: SizeFormat::default(),
        }
    }
}

impl PanelColumns {
    /// The columns of the panel on `side`; the left list for both when the
    /// panels are the same.
    pub fn of(&self, side: PanelSide) -> &[Column] {
        match side {
            PanelSide::Right if !self.same_in_both => &self.right,
            PanelSide::Left | PanelSide::Right => &self.left,
        }
    }

    pub fn width_of(&self, column: Column) -> Option<u16> {
        self.widths
            .iter()
            .find(|(wide, _)| *wide == column)
            .map(|(_, width)| *width)
    }
}

/// The font family of the window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FontChoice {
    /// The monospace font of the system.
    #[default]
    System,
    Named(String),
}

/// The font size of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontSizeChoice {
    /// The size of the system times its text scaling.
    #[default]
    System,
    /// Always within [`FONT_POINTS`].
    Points(u8),
}

impl FontSizeChoice {
    /// The size `points` names; a number outside [`FONT_POINTS`] is the system's.
    pub fn from_points(points: i64) -> Self {
        u8::try_from(points)
            .ok()
            .filter(|points| FONT_POINTS.contains(points))
            .map_or(Self::System, Self::Points)
    }
}

/// The `[appearance]` setting as it is in effect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Appearance {
    pub font: FontChoice,
    pub font_size: FontSizeChoice,
}

/// `keys` as columns: names that are no column and repeats are dropped, and
/// the name column is there, after a leading icon.
pub fn columns_from_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<Column> {
    let mut columns: Vec<Column> = Vec::new();
    for column in keys
        .into_iter()
        .filter_map(|key| Column::from_key(key.trim()))
    {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    if !columns.contains(&Column::Name) {
        let after_icon = usize::from(columns.first() == Some(&Column::Icon));
        columns.insert(after_icon, Column::Name);
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_columns_without_strangers_and_repeats() {
        let columns = columns_from_keys(["size", "bogus", " size ", "name", "owner"]);
        assert_eq!(columns, [Column::Size, Column::Name, Column::Owner]);
    }

    #[test]
    fn the_name_column_is_added_after_a_leading_icon() {
        assert_eq!(
            columns_from_keys(["icon", "size"]),
            [Column::Icon, Column::Name, Column::Size]
        );
        assert_eq!(columns_from_keys(["size"]), [Column::Name, Column::Size]);
        assert_eq!(columns_from_keys([]), [Column::Name]);
    }

    #[test]
    fn a_font_size_outside_the_range_is_the_systems() {
        assert_eq!(FontSizeChoice::from_points(9), FontSizeChoice::System);
        assert_eq!(FontSizeChoice::from_points(21), FontSizeChoice::System);
        assert_eq!(FontSizeChoice::from_points(-1), FontSizeChoice::System);
        assert_eq!(FontSizeChoice::from_points(10), FontSizeChoice::Points(10));
        assert_eq!(FontSizeChoice::from_points(20), FontSizeChoice::Points(20));
    }
}
