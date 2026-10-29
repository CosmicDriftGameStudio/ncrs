//! What Cmd+V / Ctrl+V takes from the system clipboard, and the clipboard
//! itself. The choice and the file name are plain functions; only
//! `read_system_clipboard` and `FileListWriter` touch the system.

use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

use crate::i18n::{Language, Msg};

/// What is pasted. Files are copied as they are; a picture and text have no
/// file yet and get a name from the user first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PastedContent {
    Files(Vec<PathBuf>),
    Image { png: Vec<u8> },
    Text(String),
}

/// A picture as the clipboard hands it out: 8 bits per channel, RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The kinds of pasted content that become a new file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PastedKind {
    Image,
    Text,
}

impl PastedKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Image => "png",
            Self::Text => "txt",
        }
    }
}

impl PastedContent {
    pub fn kind(&self) -> Option<PastedKind> {
        match self {
            Self::Files(_) => None,
            Self::Image { .. } => Some(PastedKind::Image),
            Self::Text(_) => Some(PastedKind::Text),
        }
    }

    /// The bytes written to the new file; `None` for files, which are copied.
    pub fn file_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Files(_) => None,
            Self::Image { png } => Some(png),
            Self::Text(text) => Some(text.as_bytes()),
        }
    }
}

/// Files win over a picture, a picture over text; an empty list or empty text
/// counts as nothing. A picture that cannot be encoded is skipped.
pub fn choose(
    files: Option<Vec<PathBuf>>,
    image: Option<RgbaImage>,
    text: Option<String>,
) -> Option<PastedContent> {
    if let Some(listed) = files.filter(|paths| !paths.is_empty()) {
        return Some(PastedContent::Files(listed));
    }
    if let Some(png) = image.as_ref().and_then(encode_png) {
        return Some(PastedContent::Image { png });
    }
    text.filter(|typed| !typed.is_empty())
        .map(PastedContent::Text)
}

fn encode_png(image: &RgbaImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&image.rgba).ok()?;
    writer.finish().ok()?;
    Some(png)
}

/// `Pasted 2026-10-28 08.15.txt`; when `taken` knows the name, `… 2.txt`,
/// `… 3.txt` and so on. No colon in the time: Windows file names cannot hold it.
pub fn pasted_name(
    kind: PastedKind,
    now: NaiveDateTime,
    lang: Language,
    taken: impl Fn(&str) -> bool,
) -> String {
    let stem = format!(
        "{} {}",
        lang.text(Msg::ClipboardPastedNamePrefix),
        now.format("%Y-%m-%d %H.%M")
    );
    let extension = kind.extension();
    let first = format!("{stem}.{extension}");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|number: u32| format!("{stem} {number}.{extension}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(first)
}

/// Reads the system clipboard. Blocking, and on X11 it may wait for the
/// program that owns the content: call it off the UI thread.
pub fn read_system_clipboard() -> Option<PastedContent> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let files = clipboard.get().file_list().ok();
    let image = clipboard.get_image().ok().and_then(|image| {
        Some(RgbaImage {
            width: u32::try_from(image.width).ok()?,
            height: u32::try_from(image.height).ok()?,
            rgba: image.bytes.into_owned(),
        })
    });
    let text = clipboard.get_text().ok();
    choose(files, image, text)
}

/// Puts a file list on the system clipboard. The clipboard is kept between
/// calls: on X11 the content belongs to the `Clipboard` object and is gone
/// when it is dropped.
#[derive(Default)]
pub struct FileListWriter {
    clipboard: Option<arboard::Clipboard>,
}

impl std::fmt::Debug for FileListWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileListWriter")
            .field("open", &self.clipboard.is_some())
            .finish()
    }
}

impl FileListWriter {
    pub fn write(&mut self, files: &[PathBuf]) -> Result<(), String> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(|error| error.to_string())?);
        }
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Err(String::new());
        };
        let paths: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        clipboard
            .set()
            .file_list(&paths)
            .map_err(|error| error.to_string())
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn moment() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 28)
            .unwrap()
            .and_hms_opt(8, 15, 42)
            .unwrap()
    }

    fn one_pixel() -> RgbaImage {
        RgbaImage {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        }
    }

    fn files() -> Option<Vec<PathBuf>> {
        Some(vec![PathBuf::from("/a/b.txt")])
    }

    #[test]
    fn files_win_over_image_and_text() {
        let chosen = choose(files(), Some(one_pixel()), Some("hello".into()));
        assert_eq!(chosen, Some(PastedContent::Files(files().unwrap())));
    }

    #[test]
    fn an_image_wins_over_text_and_is_a_png() {
        let Some(PastedContent::Image { png }) = choose(None, Some(one_pixel()), Some("t".into()))
        else {
            panic!("no image chosen");
        };
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]));
    }

    #[test]
    fn text_is_the_last_choice() {
        assert_eq!(
            choose(None, None, Some("hello".into())),
            Some(PastedContent::Text("hello".into()))
        );
    }

    #[test]
    fn empty_values_count_for_nothing() {
        assert_eq!(choose(Some(Vec::new()), None, Some(String::new())), None);
        assert_eq!(
            choose(Some(Vec::new()), None, Some("t".into())),
            Some(PastedContent::Text("t".into()))
        );
        assert_eq!(choose(None, None, None), None);
    }

    #[test]
    fn an_image_that_cannot_be_encoded_falls_back_to_text() {
        let broken = RgbaImage {
            width: 4,
            height: 4,
            rgba: vec![0; 3],
        };
        assert_eq!(
            choose(None, Some(broken), Some("t".into())),
            Some(PastedContent::Text("t".into()))
        );
    }

    #[test]
    fn the_name_carries_date_time_and_ending_in_both_languages() {
        let free = |_: &str| false;
        assert_eq!(
            pasted_name(PastedKind::Text, moment(), Language::English, free),
            "Pasted 2026-10-28 08.15.txt"
        );
        assert_eq!(
            pasted_name(PastedKind::Image, moment(), Language::German, free),
            "Eingefügt 2026-10-28 08.15.png"
        );
    }

    #[test]
    fn a_taken_name_gets_the_next_free_number() {
        let taken = |name: &str| {
            [
                "Pasted 2026-10-28 08.15.txt",
                "Pasted 2026-10-28 08.15 2.txt",
            ]
            .contains(&name)
        };
        assert_eq!(
            pasted_name(PastedKind::Text, moment(), Language::English, taken),
            "Pasted 2026-10-28 08.15 3.txt"
        );
    }
}
