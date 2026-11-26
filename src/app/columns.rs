//! The `[columns]` and `[appearance]` settings as the window uses them: what
//! the panels read each frame is kept ready here, and what the column header
//! changes is written back.

use crate::app::*;
use ncrs_config::panel_columns::{Appearance, FontChoice, FontSizeChoice, PanelColumns};
use ncrs_core::Column;
use ncrs_files::special_folders::special_folders_of;
use ncrs_settings::message::ColumnsEdit;
use ncrs_settings::settings::columns as columns_page;
use ncrs_ui_kit::font;

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

    /// The folders with a symbol of their own are read again when the
    /// sources or targets changed; checked after every message, drawn from the result.
    pub(in crate::app) fn refresh_special_folders(&mut self) {
        let filing = &self.hermes.filing_config;
        let targets = filing
            .targets
            .iter()
            .map(|known| known.target.path.as_path());
        let sources = filing
            .sources
            .iter()
            .map(|known| known.source.path.as_path());
        let (last_targets, last_sources) = &self.special_folders_source;
        if targets
            .clone()
            .eq(last_targets.iter().map(PathBuf::as_path))
            && sources
                .clone()
                .eq(last_sources.iter().map(PathBuf::as_path))
        {
            return;
        }
        let special_folders = special_folders_of(targets.clone(), sources.clone(), &self.home);
        let source = (
            targets.map(Path::to_path_buf).collect(),
            sources.map(Path::to_path_buf).collect(),
        );
        self.special_folders = special_folders;
        self.special_folders_source = source;
    }

    /// Writes `columns` to the config on a blocking thread, one write after
    /// the other with the other settings; the panels are drawn again with the
    /// result.
    fn save_columns(&mut self, columns: PanelColumns) -> Task<Message> {
        if columns == self.columns {
            return Task::none();
        }
        if self.config_path.is_none() {
            self.columns = columns;
            return Task::none();
        }
        self.update_settings(ncrs_settings::message::SettingsMessage::Change(
            ncrs_settings::message::SettingChange::Columns(columns),
        ))
    }

    pub(in crate::app) fn panel_columns_changed(
        &mut self,
        side: PanelSide,
        list: Vec<Column>,
    ) -> Task<Message> {
        let columns = columns_page::with_list(&self.columns, side, list);
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_column_resized(
        &mut self,
        column: Column,
        width: u16,
    ) -> Task<Message> {
        let columns = columns_page::with_column_width(&self.columns, column, width);
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_columns_same_in_both(&mut self, same: bool) -> Task<Message> {
        let columns = columns_page::edited(
            &self.columns,
            PanelSide::Left,
            ColumnsEdit::SameInBoth(same),
        );
        self.save_columns(columns)
    }

    pub(in crate::app) fn panel_column_defaults_restored(
        &mut self,
        side: PanelSide,
    ) -> Task<Message> {
        let columns = columns_page::edited(&self.columns, side, ColumnsEdit::Restore);
        self.save_columns(columns)
    }

    pub(in crate::app) fn open_column_settings(&mut self) -> Task<Message> {
        self.show_setting(crate::messages::SettingsFeature::Columns, None)
    }
}
