//! Layout metrics the kit components and the app share.
use iced::Size;

use super::theme::size;

pub const INITIAL_WINDOW_SIZE: Size = Size::new(1200.0, 760.0);

pub const FKEYBAR_HEIGHT: f32 = size::FKEY_BAR;

/// Slots in the function key bar: F1 to F10.
pub const FUNCTION_KEY_COUNT: usize = 10;

/// A row of a file list. Fixed, so the number of visible rows follows from the
/// window height.
pub const ROW_HEIGHT: f32 = 22.0;

/// The star column, left of the name. Its own button so a click there tags the
/// row instead of moving the cursor.
pub const TAG_COLUMN_WIDTH: f32 = 22.0;

pub const SIZE_COLUMN_WIDTH: f32 = 90.0;
pub const DATE_COLUMN_WIDTH: f32 = 150.0;
/// The Hermes status column, after the date: an icon for a file, a summary
/// of icons and counts for a folder.
pub const HERMES_COLUMN_WIDTH: f32 = 124.0;

/// Space between the date and the Hermes column.
pub const HERMES_CELL_GAP: f32 = 12.0;
