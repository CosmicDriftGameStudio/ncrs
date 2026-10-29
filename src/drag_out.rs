//! Starts the system drag that carries files out of the window.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::clipboard_paste::{encode_png, RgbaImage};

/// Must run on the main thread: iced's `window::run` does.
pub fn start(window: &dyn iced::window::Window, files: Vec<PathBuf>) -> Result<(), String> {
    let png = drag_image().ok_or_else(|| "the drag image could not be encoded".to_owned())?;
    // Copy only: the files stay in ncrs's panel whatever the drop target does.
    let options = drag::Options {
        mode: drag::DragMode::Copy,
        ..drag::Options::default()
    };
    drag::start_drag(
        &window,
        drag::DragItem::Files(files),
        drag::Image::Raw(png.to_vec()),
        |_result, _position| {},
        options,
    )
    .map_err(|error| error.to_string())
}

/// macOS builds an `NSImage` from these bytes and panics on raw pixels, so
/// they have to be an encoded picture.
fn drag_image() -> Option<&'static [u8]> {
    static PNG: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    PNG.get_or_init(|| {
        encode_png(&RgbaImage {
            width: crate::WINDOW_ICON_SIZE,
            height: crate::WINDOW_ICON_SIZE,
            rgba: crate::WINDOW_ICON_RGBA.to_vec(),
        })
    })
    .as_deref()
}
