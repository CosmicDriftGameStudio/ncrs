//! The preview of a document: a rendered PDF page, a picture or the start of
//! a text. The state is plain data; the work is [`load`], which blocks, so
//! the app runs it through [`load_off_thread`].
//!
//! What a document contains is only ever shown, never acted on.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::image::Handle;
use image::{DynamicImage, ImageError, ImageReader, Limits};
use ncrs_intake::{Extracted, OfficeText, PdfText, PlainText, TextError, TextSource};

/// The longer side of a rendered or scaled preview, in pixels.
const PREVIEW_SIDE_PIXELS: u16 = 1200;
/// Larger files are not opened for a preview.
const MAX_FILE_BYTES: u64 = 64 << 20;
/// The picture decoder refuses larger pictures than this, in either direction.
const MAX_IMAGE_SIDE_PIXELS: u32 = 12_000;
const MAX_IMAGE_ALLOC_BYTES: u64 = 256 << 20;
/// A text excerpt ends at the first of these limits.
const EXCERPT_LINES: usize = 40;
const EXCERPT_CHARS: usize = 4000;
/// How long the work may take before the preview gives up on it.
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(10);

const PDF_HEADER: &[u8] = b"%PDF-";
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "tif", "tiff"];
const TEXT_EXTENSIONS: [&str; 4] = ["txt", "md", "docx", "odt"];

/// A decoded picture, uploaded once: the handle is made here and not in the
/// view, which would make a new texture every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewImage {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

impl PreviewImage {
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            handle: Handle::from_rgba(width, height, rgba),
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewContent {
    /// The first page of a PDF.
    Page {
        image: PreviewImage,
        page_count: usize,
    },
    Image(PreviewImage),
    Text {
        excerpt: String,
        /// The document has more text than the excerpt.
        truncated: bool,
    },
    /// Not a regular file, or a type without a preview.
    Unsupported,
    /// A PDF, and pdfium is not there.
    LibraryMissing,
    TooLarge,
    TimedOut,
    /// A short technical reason, without a path or any content.
    Failed(String),
}

/// Asks the work for the preview of `path`; `serial` tells the answer from an
/// older one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub serial: u64,
}

/// What the preview shows for the document under the cursor.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentPreview {
    path: Option<PathBuf>,
    serial: u64,
    /// `None` while the work runs.
    content: Option<PreviewContent>,
}

impl DocumentPreview {
    pub fn content(&self) -> Option<&PreviewContent> {
        self.content.as_ref()
    }

    /// The request for `path`, or `None` when it is the document the preview
    /// already shows or waits for.
    pub fn request(&mut self, path: &Path) -> Option<PreviewRequest> {
        if self.path.as_deref() == Some(path) {
            return None;
        }
        self.path = Some(path.to_path_buf());
        self.serial = self.serial.wrapping_add(1);
        self.content = None;
        Some(PreviewRequest {
            path: path.to_path_buf(),
            serial: self.serial,
        })
    }

    /// The answer of the work. One for an older request is dropped.
    pub fn loaded(&mut self, serial: u64, content: PreviewContent) {
        if self.path.is_some() && serial == self.serial {
            self.content = Some(content);
        }
    }

    /// No document is selected: answers still on their way are dropped.
    pub fn clear(&mut self) {
        if self.path.take().is_some() {
            self.serial = self.serial.wrapping_add(1);
        }
        self.content = None;
    }

    #[cfg(test)]
    pub fn showing(content: PreviewContent) -> Self {
        Self {
            path: Some(PathBuf::from("shown")),
            serial: 1,
            content: Some(content),
        }
    }
}

/// The limits [`load_within`] holds the work to.
#[derive(Debug, Clone, Copy)]
struct PreviewLimits {
    file_bytes: u64,
}

const LIMITS: PreviewLimits = PreviewLimits {
    file_bytes: MAX_FILE_BYTES,
};

/// [`load`] on a blocking thread, given up on after [`PREVIEW_TIMEOUT`]. The
/// thread itself cannot be stopped; its answer is dropped.
pub async fn load_off_thread(path: PathBuf) -> PreviewContent {
    let work = tokio::task::spawn_blocking(move || load(&path));
    match tokio::time::timeout(PREVIEW_TIMEOUT, work).await {
        Ok(Ok(content)) => content,
        Ok(Err(join)) => PreviewContent::Failed(join.to_string()),
        Err(_elapsed) => PreviewContent::TimedOut,
    }
}

/// Reads `path` and makes its preview. Blocks for as long as pdfium or the
/// decoder takes.
pub fn load(path: &Path) -> PreviewContent {
    load_within(path, LIMITS)
}

fn load_within(path: &Path, limits: PreviewLimits) -> PreviewContent {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return PreviewContent::Failed("the file cannot be read".to_owned());
    };
    if !metadata.is_file() {
        return PreviewContent::Unsupported;
    }
    if metadata.len() > limits.file_bytes {
        return PreviewContent::TooLarge;
    }
    match kind_of(path) {
        Some(Kind::Pdf) => load_pdf_page(path),
        Some(Kind::Image) => load_image(path),
        Some(Kind::Text) => load_text(path),
        None => PreviewContent::Unsupported,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pdf,
    Image,
    Text,
}

fn kind_of(path: &Path) -> Option<Kind> {
    if has_pdf_header(path) {
        return Some(Kind::Pdf);
    }
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        Some(Kind::Image)
    } else if TEXT_EXTENSIONS.contains(&extension.as_str()) {
        Some(Kind::Text)
    } else {
        None
    }
}

fn has_pdf_header(path: &Path) -> bool {
    let mut header = Vec::with_capacity(PDF_HEADER.len());
    File::open(path)
        .and_then(|file| file.take(PDF_HEADER.len() as u64).read_to_end(&mut header))
        .is_ok()
        && header == PDF_HEADER
}

fn load_pdf_page(path: &Path) -> PreviewContent {
    let pdf = match PdfText::load_shipped() {
        Ok(pdf) => pdf,
        Err(TextError::LibraryMissing { .. }) => return PreviewContent::LibraryMissing,
        Err(other) => return failed_from(&other),
    };
    match pdf.render_preview_page(path, PREVIEW_SIDE_PIXELS) {
        Ok(page) => PreviewContent::Page {
            image: PreviewImage::from_rgba(page.width, page.height, page.rgba),
            page_count: page.page_count,
        },
        Err(error) => failed_from(&error),
    }
}

fn failed_from(error: &TextError) -> PreviewContent {
    match error {
        TextError::Unreadable { reason, .. } => PreviewContent::Failed(reason.clone()),
        TextError::LibraryMissing { .. } => PreviewContent::LibraryMissing,
        _ => PreviewContent::Failed("the file cannot be read".to_owned()),
    }
}

fn load_image(path: &Path) -> PreviewContent {
    match decode_scaled(path) {
        Ok(image) => PreviewContent::Image(image),
        Err(ImageError::Limits(_)) => PreviewContent::TooLarge,
        Err(error) => PreviewContent::Failed(error.to_string()),
    }
}

fn decode_scaled(path: &Path) -> Result<PreviewImage, ImageError> {
    let mut limits = Limits::no_limits();
    limits.max_image_width = Some(MAX_IMAGE_SIDE_PIXELS);
    limits.max_image_height = Some(MAX_IMAGE_SIDE_PIXELS);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(limits);
    let decoded = reader.decode()?;
    let side = u32::from(PREVIEW_SIDE_PIXELS);
    let scaled = if decoded.width() > side || decoded.height() > side {
        decoded.thumbnail(side, side)
    } else {
        decoded
    };
    Ok(rgba_image(scaled))
}

fn rgba_image(image: DynamicImage) -> PreviewImage {
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    PreviewImage::from_rgba(width, height, rgba.into_raw())
}

fn load_text(path: &Path) -> PreviewContent {
    let sources: [&dyn TextSource; 2] = [&PlainText, &OfficeText];
    for source in sources {
        match source.extract(path) {
            Ok(Extracted::Text(text)) => return excerpt_of(&text),
            Ok(Extracted::NotApplicable | Extracted::PageImages(_)) => {}
            Err(error) => return failed_from(&error),
        }
    }
    PreviewContent::Unsupported
}

/// The first lines of `text`, up to the character limit.
fn excerpt_of(text: &str) -> PreviewContent {
    let mut excerpt = String::new();
    let mut chars = 0;
    let mut lines = text.lines();
    for line in lines.by_ref().take(EXCERPT_LINES) {
        let room = EXCERPT_CHARS - chars;
        let line_chars = line.chars().count();
        if line_chars >= room {
            excerpt.extend(line.chars().take(room));
            return PreviewContent::Text {
                excerpt,
                truncated: true,
            };
        }
        excerpt.push_str(line);
        excerpt.push('\n');
        chars += line_chars + 1;
    }
    PreviewContent::Text {
        excerpt: excerpt.trim_end().to_owned(),
        truncated: lines.next().is_some(),
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
    use ncrs_intake::test_support::{pdf_bytes, write};
    use tempfile::TempDir;

    use super::*;

    fn tiny_limit() -> PreviewLimits {
        PreviewLimits { file_bytes: 100 }
    }

    #[test]
    fn a_short_text_file_shows_in_full() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"Invoice 4711\nCompany A\n");
        assert_eq!(
            load(&path),
            PreviewContent::Text {
                excerpt: "Invoice 4711\nCompany A".to_owned(),
                truncated: false,
            }
        );
    }

    #[test]
    fn a_long_text_file_is_cut_by_lines_and_marked() {
        let dir = TempDir::new().unwrap();
        let body: String = (0..100).map(|line| format!("line {line}\n")).collect();
        let path = write(&dir, "long.md", body.as_bytes());
        let PreviewContent::Text { excerpt, truncated } = load(&path) else {
            panic!("expected a text");
        };
        assert!(truncated);
        assert_eq!(excerpt.lines().count(), EXCERPT_LINES);
        assert!(excerpt.ends_with("line 39"), "{excerpt}");
    }

    #[test]
    fn one_very_long_line_is_cut_by_characters_and_marked() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "wide.txt", "ä".repeat(10_000).as_bytes());
        let PreviewContent::Text { excerpt, truncated } = load(&path) else {
            panic!("expected a text");
        };
        assert!(truncated);
        assert_eq!(excerpt.chars().count(), EXCERPT_CHARS);
    }

    #[test]
    fn a_large_picture_is_scaled_down_to_the_preview_side() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wide.png");
        image::RgbaImage::from_pixel(2400, 1200, image::Rgba([10, 120, 200, 255]))
            .save(&path)
            .unwrap();
        let PreviewContent::Image(image) = load(&path) else {
            panic!("expected an image");
        };
        assert_eq!((image.width, image.height), (1200, 600));
    }

    #[test]
    fn a_small_picture_keeps_its_size() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("small.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let PreviewContent::Image(image) = load(&path) else {
            panic!("expected an image");
        };
        assert_eq!((image.width, image.height), (40, 30));
    }

    #[test]
    fn a_file_over_the_limit_is_too_large() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "big.txt", &[b'a'; 101]);
        assert_eq!(load_within(&path, tiny_limit()), PreviewContent::TooLarge);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_followed() {
        let dir = TempDir::new().unwrap();
        let target = write(&dir, "real.txt", b"secret");
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(load(&link), PreviewContent::Unsupported);
    }

    #[test]
    fn a_folder_has_no_preview() {
        let dir = TempDir::new().unwrap();
        assert_eq!(load(dir.path()), PreviewContent::Unsupported);
    }

    #[test]
    fn heic_and_unknown_types_have_no_preview() {
        let dir = TempDir::new().unwrap();
        for name in ["photo.heic", "data.xyz"] {
            let path = write(&dir, name, b"not decoded");
            assert_eq!(load(&path), PreviewContent::Unsupported, "{name}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_picture_but_named_like_one_fails() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "broken.png", b"not a png");
        assert!(matches!(load(&path), PreviewContent::Failed(_)));
    }

    #[test]
    fn a_pdf_shows_its_first_page() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "filled.pdf",
            &pdf_bytes("0 0 1 rg 20 20 100 100 re f", "", &[]),
        );
        let PreviewContent::Page { image, page_count } = load(&path) else {
            panic!("expected a page");
        };
        assert_eq!(page_count, 1);
        assert!(image.width <= 1200 && image.height <= 1200);
    }

    #[test]
    fn an_answer_for_an_older_request_is_dropped() {
        let mut preview = DocumentPreview::default();
        let first = preview.request(Path::new("a.txt")).unwrap();
        let second = preview.request(Path::new("b.txt")).unwrap();
        preview.loaded(first.serial, PreviewContent::Unsupported);
        assert_eq!(preview.content(), None);
        preview.loaded(second.serial, PreviewContent::TooLarge);
        assert_eq!(preview.content(), Some(&PreviewContent::TooLarge));
    }

    #[test]
    fn the_same_path_makes_no_new_request() {
        let mut preview = DocumentPreview::default();
        assert!(preview.request(Path::new("a.txt")).is_some());
        assert!(preview.request(Path::new("a.txt")).is_none());
        assert!(preview.request(Path::new("b.txt")).is_some());
    }

    #[test]
    fn clearing_drops_the_answer_on_its_way() {
        let mut preview = DocumentPreview::default();
        let request = preview.request(Path::new("a.txt")).unwrap();
        preview.clear();
        preview.loaded(request.serial, PreviewContent::TooLarge);
        assert_eq!(preview.content(), None);
        assert!(preview.request(Path::new("a.txt")).is_some());
    }

    #[tokio::test]
    async fn the_off_thread_load_returns_the_content() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"hello");
        assert_eq!(
            load_off_thread(path).await,
            PreviewContent::Text {
                excerpt: "hello".to_owned(),
                truncated: false,
            }
        );
    }
}
