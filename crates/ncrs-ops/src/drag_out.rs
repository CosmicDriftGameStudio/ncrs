//! Starts the system drag that carries files out of the window.

use std::path::PathBuf;
use std::sync::OnceLock;

use iced::{window, Task};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::clipboard_paste::{encode_png, RgbaImage};

/// The picture that hangs on the pointer during the drag: square, RGBA.
#[derive(Debug, Clone, Copy)]
pub struct DragIcon {
    pub size: u32,
    pub rgba: &'static [u8],
}

/// Must run on the main thread: iced's `window::run` does.
pub fn start(
    window: &dyn iced::window::Window,
    files: Vec<PathBuf>,
    icon: DragIcon,
) -> Result<(), String> {
    let png = drag_image(icon).ok_or_else(|| "the drag image could not be encoded".to_owned())?;
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
fn drag_image(icon: DragIcon) -> Option<&'static [u8]> {
    static PNG: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    PNG.get_or_init(|| {
        encode_png(&RgbaImage {
            width: icon.size,
            height: icon.size,
            rgba: icon.rgba.to_vec(),
        })
    })
    .as_deref()
}

/// The pointer left the window with files held: hands `sources` to the system
/// drag, which takes over the mouse until the drop. Only files of this
/// computer's disk can leave (`all_on_local_disk`).
pub fn start_drag_out(
    lang: Language,
    sources: Vec<PathBuf>,
    all_on_local_disk: bool,
    icon: DragIcon,
) -> (Task<Result<(), String>>, Vec<Action>) {
    if sources.is_empty() {
        return (Task::none(), Vec::new());
    }
    if !all_on_local_disk {
        let text = lang.text(Msg::StatusDragOutLocalOnly).to_owned();
        return (Task::none(), vec![Action::ShowNotice(text)]);
    }
    let started = window::latest().and_then(move |id| {
        let files = sources.clone();
        window::run(id, move |window| start(window, files, icon))
    });
    (started, Vec::new())
}

/// What a system drag that could not start says.
pub fn drag_out_failure(lang: Language, started: Result<(), String>) -> Option<Action> {
    let reason = started.err()?;
    let text = fill_template(lang.text(Msg::ErrorDragOutFailed), &[("{reason}", &reason)]);
    Some(Action::ShowError(text))
}
