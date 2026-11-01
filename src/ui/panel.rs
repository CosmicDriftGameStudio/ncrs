//! The file panel lives in `ncrs-files`; what stays here is the Hermes cell,
//! which needs the app's Hermes column.
use ncrs_ui_kit::file_row::{HermesCell, HermesContent};

use crate::file_list_tags::ShownTags;
use crate::fs::FileEntry;
use crate::hermes_column::ColumnView;
use crate::i18n::Language;
use crate::palette::Palette;

pub use ncrs_files::column_header::plain_column_header;
pub use ncrs_files::hit_list::HitList;
pub use ncrs_files::panel::PanelState;

/// What the Hermes column shows for `entry`, from the lookups of `column`.
pub fn hermes_cell_of<'a, M: 'a>(
    entry: &FileEntry,
    column: ColumnView<'a>,
    lang: Language,
    palette: &Palette,
) -> HermesCell<'a, M> {
    let is_folder = entry.is_dir && !entry.is_parent;
    let content = if is_folder {
        HermesContent::Folder(column.summary_of(&entry.path).cloned())
    } else if entry.is_dir {
        HermesContent::None
    } else {
        column
            .state_of(&entry.path)
            .map_or(HermesContent::None, HermesContent::File)
    };
    HermesCell::new(
        content,
        is_folder.then(|| column.role_of(&entry.path)).flatten(),
        lang,
        palette,
        column.angle(),
        if is_folder || entry.is_dir {
            ShownTags::default()
        } else {
            column.tags_after(&entry.path)
        },
        column.panel_width(),
    )
}
