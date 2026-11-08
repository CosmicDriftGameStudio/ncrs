//! The `[columns]` and `[appearance]` settings as the window uses them: what
//! the panels read each frame is kept ready here, and what the column header
//! changes is written back.

use crate::app::*;
use ncrs_config::config::FilingLocation;
use ncrs_config::panel_columns::{Appearance, FontChoice, FontSizeChoice, PanelColumns};
use ncrs_core::Column;
use ncrs_files::special_folders::system_folders;
use ncrs_ui_kit::file_icon::SpecialFolder;
use ncrs_ui_kit::font;

/// The folders with a symbol of their own: the ones Hermes reads and files
/// into first, so a home folder that is also one of them shows that role.
pub(in crate::app) fn special_folders_of(
    locations: &[FilingLocation],
    home: &Path,
) -> Vec<(PathBuf, SpecialFolder)> {
    let hermes = locations.iter().filter_map(|location| {
        if location.is_target {
            Some((location.path.clone(), SpecialFolder::WriteFolder))
        } else if location.mode == ncrs_intake::LocationMode::Inbox {
            Some((location.path.clone(), SpecialFolder::ReadFolder))
        } else {
            None
        }
    });
    hermes.chain(system_folders(home)).collect()
}

/// The setting as the panels read it. A function of the field rather than of
/// the app, so the panels can be updated while it is borrowed.
pub(in crate::app) fn column_settings_of(columns: &PanelColumns) -> ncrs_files::ColumnSettings<'_> {
    ncrs_files::ColumnSettings {
        same_in_both: columns.same_in_both,
        left: &columns.left,
        right: &columns.right,
        widths: &columns.widths,
        date_format: columns.date_format,
        size_format: columns.size_format,
    }
}

/// `columns` with the list of `side` replaced; with one list for both panels
/// that is the list of both.
pub(in crate::app) fn with_side_columns(
    columns: &PanelColumns,
    side: PanelSide,
    list: Vec<Column>,
) -> PanelColumns {
    let mut changed = columns.clone();
    match side {
        PanelSide::Right if !changed.same_in_both => changed.right = list,
        PanelSide::Left | PanelSide::Right => {
            changed.right.clone_from(&list);
            changed.left = list;
        }
    }
    changed
}

pub(in crate::app) fn with_column_width(
    columns: &PanelColumns,
    column: Column,
    width: u16,
) -> PanelColumns {
    let mut changed = columns.clone();
    match changed.widths.iter_mut().find(|(wide, _)| *wide == column) {
        Some((_, current)) => *current = width,
        None => changed.widths.push((column, width)),
    }
    changed
}

/// Both panels start from what the left one shows, whichever way it is turned.
pub(in crate::app) fn with_same_in_both(columns: &PanelColumns, same: bool) -> PanelColumns {
    let mut changed = columns.clone();
    changed.same_in_both = same;
    changed.right.clone_from(&changed.left);
    changed
}

/// Built-in lists and widths back; the formats stay.
pub(in crate::app) fn with_default_columns(columns: &PanelColumns) -> PanelColumns {
    PanelColumns {
        same_in_both: columns.same_in_both,
        date_format: columns.date_format,
        size_format: columns.size_format,
        ..PanelColumns::default()
    }
}

fn family_of(font: &FontChoice) -> Option<&str> {
    match font {
        FontChoice::System => None,
        FontChoice::Named(name) => Some(name),
    }
}

fn points_of(size: FontSizeChoice) -> Option<u8> {
    match size {
        FontSizeChoice::System => None,
        FontSizeChoice::Points(points) => Some(points),
    }
}

impl App {
    /// What the panels read of `[columns]`.
    pub(in crate::app) fn column_settings(&self) -> ncrs_files::ColumnSettings<'_> {
        column_settings_of(&self.columns)
    }

    /// Takes the `[appearance]` of the config: the font now, the size as the
    /// scale of the next frame.
    pub(in crate::app) fn take_over_appearance(&mut self, appearance: Appearance) {
        if appearance.font != self.appearance.font {
            font::apply_font(family_of(&appearance.font));
        }
        let scale = font::ui_scale(
            points_of(appearance.font_size),
            font::system_font().as_ref(),
        );
        // iced reports the window size in units of the scale and sends no
        // resize when only the scale changes, so the rows that fit are
        // counted again here.
        if scale != self.ui_scale {
            let size = self.window_size * (self.ui_scale / scale);
            self.ui_scale = scale;
            self.apply_window_size(size);
        }
        self.appearance = appearance;
    }

    /// The font at start; the same call as after a change, so "System" looks the
    /// same either way.
    pub(in crate::app) fn start_appearance(&mut self, appearance: Appearance) {
        font::apply_font(family_of(&appearance.font));
        self.ui_scale = font::ui_scale(
            points_of(appearance.font_size),
            font::system_font().as_ref(),
        );
        self.appearance = appearance;
    }

    /// The factor the whole window is drawn with.
    pub fn scale_factor(&self) -> f32 {
        self.ui_scale
    }

    /// The folders with a symbol of their own are read again when the Hermes
    /// folders changed; checked after every message, drawn from the result.
    pub(in crate::app) fn refresh_special_folders(&mut self) {
        let locations = &self.hermes.filing_config.locations;
        if *locations == self.special_folders_source {
            return;
        }
        self.special_folders = special_folders_of(locations, &self.home);
        self.special_folders_source.clone_from(locations);
    }

    /// Writes `columns` to the config and takes the written result over; the
    /// panels are drawn again with it.
    fn save_columns(&mut self, columns: PanelColumns) -> Task<Message> {
        if columns == self.columns {
            return Task::none();
        }
        let Some(path) = self.config_path.clone() else {
            self.columns = columns;
            return Task::none();
        };
        match config::write_columns(&path, &columns) {
            Ok(written) => self.columns = written.columns,
            Err(err) => self.set_status_error(&err.to_string()),
        }
        Task::none()
    }

    pub(in crate::app) fn panel_columns_changed(
        &mut self,
        side: PanelSide,
        list: Vec<Column>,
    ) -> Task<Message> {
        let columns = with_side_columns(&self.columns, side, list);
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_column_resized(
        &mut self,
        column: Column,
        width: u16,
    ) -> Task<Message> {
        let columns = with_column_width(&self.columns, column, width);
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_columns_same_in_both(&mut self, same: bool) -> Task<Message> {
        let columns = with_same_in_both(&self.columns, same);
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_column_defaults_restored(&mut self) -> Task<Message> {
        let columns = with_default_columns(&self.columns);
        self.save_columns(columns)
    }

    pub(in crate::app) fn open_column_settings(&mut self) -> Task<Message> {
        self.show_setting(crate::messages::SettingsFeature::Columns, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_list_for_both_panels_changes_both() {
        let start = PanelColumns::default();
        let list = vec![Column::Icon, Column::Name, Column::Owner];
        let changed = with_side_columns(&start, PanelSide::Right, list.clone());
        assert_eq!((changed.left, changed.right), (list.clone(), list));
    }

    #[test]
    fn per_panel_lists_change_one_side() {
        let start = with_same_in_both(&PanelColumns::default(), false);
        let list = vec![Column::Name, Column::Type];
        let changed = with_side_columns(&start, PanelSide::Right, list.clone());
        assert_eq!(changed.right, list);
        assert_eq!(changed.left, Column::DEFAULT.to_vec());
    }

    #[test]
    fn a_width_is_set_once_per_column() {
        let start = PanelColumns::default();
        let once = with_column_width(&start, Column::Size, 90);
        let twice = with_column_width(&once, Column::Size, 120);
        assert_eq!(twice.widths, vec![(Column::Size, 120)]);
    }

    #[test]
    fn restoring_keeps_the_formats() {
        let mut start = PanelColumns::default();
        start.left.push(Column::Owner);
        start.widths.push((Column::Size, 90));
        start.date_format = ncrs_core::DateFormat::Short;
        let restored = with_default_columns(&start);
        assert_eq!(restored.left, Column::DEFAULT.to_vec());
        assert!(restored.widths.is_empty());
        assert_eq!(restored.date_format, ncrs_core::DateFormat::Short);
    }
}
