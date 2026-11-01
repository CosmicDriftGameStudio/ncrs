//! The preview of a document: a rendered PDF page, a picture or the start of
//! a text. The state is plain data; the work is [`load`], which blocks, so
//! the app runs it through [`load_off_thread`].
//!
//! What a document contains is only ever shown, never acted on.

use std::io::{Cursor, Read as _};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::image::Handle;
use image::{DynamicImage, ImageError, ImageReader, Limits};
use ncrs_intake::{extract_text_from_bytes, open_without_following, Extracted, PdfText, TextError};

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
        /// Where the excerpt holds a passage, sorted and apart from each other.
        highlights: Vec<Range<usize>>,
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
    /// The pieces of the text to mark.
    pub passages: Vec<String>,
}

/// What the preview shows for the document under the cursor.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentPreview {
    path: Option<PathBuf>,
    passages: Vec<String>,
    serial: u64,
    /// `None` while the work runs.
    content: Option<PreviewContent>,
}

impl DocumentPreview {
    pub fn content(&self) -> Option<&PreviewContent> {
        self.content.as_ref()
    }

    /// The request for `path` with `passages` marked, or `None` when the
    /// preview already shows or waits for exactly that.
    pub fn request(&mut self, path: &Path, passages: &[String]) -> Option<PreviewRequest> {
        if self.path.as_deref() == Some(path) && self.passages == passages {
            return None;
        }
        self.path = Some(path.to_path_buf());
        self.passages = passages.to_vec();
        self.serial = self.serial.wrapping_add(1);
        self.content = None;
        Some(PreviewRequest {
            path: path.to_path_buf(),
            serial: self.serial,
            passages: passages.to_vec(),
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn showing(content: PreviewContent) -> Self {
        Self {
            path: Some(PathBuf::from("shown")),
            passages: Vec::new(),
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
pub async fn load_off_thread(path: PathBuf, passages: Vec<String>) -> PreviewContent {
    let work = tokio::task::spawn_blocking(move || load(&path, &passages));
    match tokio::time::timeout(PREVIEW_TIMEOUT, work).await {
        Ok(Ok(content)) => content,
        Ok(Err(join)) => PreviewContent::Failed(join.to_string()),
        Err(_elapsed) => PreviewContent::TimedOut,
    }
}

/// Reads `path` and makes its preview. Blocks for as long as pdfium or the
/// decoder takes. The `passages` are marked in a page and in a text; a
/// picture shows none.
pub fn load(path: &Path, passages: &[String]) -> PreviewContent {
    load_within(path, passages, LIMITS)
}

fn load_within(path: &Path, passages: &[String], limits: PreviewLimits) -> PreviewContent {
    let bytes = match read_regular_file(path, limits) {
        Ok(bytes) => bytes,
        Err(refused) => return refused,
    };
    match kind_of(path, &bytes) {
        Some(Kind::Pdf) => load_pdf_page(path, &bytes, passages),
        Some(Kind::Image) => load_image(&bytes),
        Some(Kind::Text) => load_text(path, &bytes, passages),
        None => PreviewContent::Unsupported,
    }
}

/// The content of the regular file at `path`, from one open that neither
/// follows a link nor blocks on a FIFO. Everything after works on these bytes,
/// so a path swapped in the meantime changes nothing.
fn read_regular_file(path: &Path, limits: PreviewLimits) -> Result<Vec<u8>, PreviewContent> {
    let unreadable = || PreviewContent::Failed("the file cannot be read".to_owned());
    // Without O_NOFOLLOW the open below follows a link, so a link is refused
    // up front; Windows has no FIFO to block on in between.
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink()) {
        return Err(PreviewContent::Unsupported);
    }
    // The open fails for a link (ELOOP under O_NOFOLLOW) and, on Windows, for
    // a folder; the lookup only names the refusal and opens nothing.
    let file =
        open_without_following(path).map_err(|_error| match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() => PreviewContent::Unsupported,
            _ => unreadable(),
        })?;
    let metadata = file.metadata().map_err(|_error| unreadable())?;
    if !metadata.is_file() {
        return Err(PreviewContent::Unsupported);
    }
    if metadata.len() > limits.file_bytes {
        return Err(PreviewContent::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(limits.file_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_error| unreadable())?;
    if bytes.len() as u64 > limits.file_bytes {
        return Err(PreviewContent::TooLarge);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pdf,
    Image,
    Text,
}

fn kind_of(path: &Path, bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(PDF_HEADER) {
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

fn load_pdf_page(path: &Path, bytes: &[u8], passages: &[String]) -> PreviewContent {
    let pdf = match PdfText::load_shipped() {
        Ok(pdf) => pdf,
        Err(TextError::LibraryMissing { .. }) => return PreviewContent::LibraryMissing,
        Err(other) => return failed_from(&other),
    };
    match pdf.render_preview_page(path, bytes, PREVIEW_SIDE_PIXELS, passages) {
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

fn load_image(bytes: &[u8]) -> PreviewContent {
    match decode_scaled(bytes) {
        Ok(image) => PreviewContent::Image(image),
        Err(ImageError::Limits(_)) => PreviewContent::TooLarge,
        Err(error) => PreviewContent::Failed(error.to_string()),
    }
}

fn decode_scaled(bytes: &[u8]) -> Result<PreviewImage, ImageError> {
    let mut limits = Limits::no_limits();
    limits.max_image_width = Some(MAX_IMAGE_SIDE_PIXELS);
    limits.max_image_height = Some(MAX_IMAGE_SIDE_PIXELS);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
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

fn load_text(path: &Path, bytes: &[u8], passages: &[String]) -> PreviewContent {
    match extract_text_from_bytes(path, bytes) {
        Ok(Extracted::Text(text)) => excerpt_of(&text, passages),
        Ok(Extracted::NotApplicable | Extracted::PageImages(_)) => PreviewContent::Unsupported,
        Err(error) => failed_from(&error),
    }
}

/// The first lines of `text`, up to the character limit.
fn excerpt_of(text: &str, passages: &[String]) -> PreviewContent {
    let mut excerpt = String::new();
    let mut chars = 0;
    let mut lines = text.lines();
    for line in lines.by_ref().take(EXCERPT_LINES) {
        let room = EXCERPT_CHARS - chars;
        let line_chars = line.chars().count();
        if line_chars >= room {
            excerpt.extend(line.chars().take(room));
            return text_content(excerpt, true, passages);
        }
        excerpt.push_str(line);
        excerpt.push('\n');
        chars += line_chars + 1;
    }
    text_content(
        excerpt.trim_end().to_owned(),
        lines.next().is_some(),
        passages,
    )
}

fn text_content(excerpt: String, truncated: bool, passages: &[String]) -> PreviewContent {
    let highlights = highlight_ranges(&excerpt, passages);
    PreviewContent::Text {
        excerpt,
        truncated,
        highlights,
    }
}

/// Where `text` holds one of the passages, ignoring case: sorted by start,
/// overlapping and touching places joined, all on character boundaries.
fn highlight_ranges(text: &str, passages: &[String]) -> Vec<Range<usize>> {
    let mut found: Vec<Range<usize>> = passages
        .iter()
        .filter(|passage| !passage.is_empty())
        .flat_map(|passage| occurrences_ignoring_case(text, passage))
        .collect();
    found.sort_by_key(|range| (range.start, range.end));
    let mut joined: Vec<Range<usize>> = Vec::new();
    for range in found {
        match joined.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => joined.push(range),
        }
    }
    joined
}

fn occurrences_ignoring_case(text: &str, needle: &str) -> Vec<Range<usize>> {
    let same_char = |a: char, b: char| a.to_lowercase().eq(b.to_lowercase());
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rest) = text.get(from..).filter(|rest| !rest.is_empty()) {
        let mut candidate = rest.char_indices();
        let mut end = 0;
        let matches = needle.chars().all(|wanted| match candidate.next() {
            Some((at, ch)) if same_char(ch, wanted) => {
                end = at + ch.len_utf8();
                true
            }
            _ => false,
        });
        if matches {
            found.push(from..from + end);
            from += end;
        } else {
            from += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    found
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
            load(&path, &[]),
            PreviewContent::Text {
                excerpt: "Invoice 4711\nCompany A".to_owned(),
                truncated: false,
                highlights: Vec::new(),
            }
        );
    }

    #[test]
    fn a_long_text_file_is_cut_by_lines_and_marked() {
        let dir = TempDir::new().unwrap();
        let body: String = (0..100).map(|line| format!("line {line}\n")).collect();
        let path = write(&dir, "long.md", body.as_bytes());
        let PreviewContent::Text {
            excerpt, truncated, ..
        } = load(&path, &[])
        else {
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
        let PreviewContent::Text {
            excerpt, truncated, ..
        } = load(&path, &[])
        else {
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
        let PreviewContent::Image(image) = load(&path, &[]) else {
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
        let PreviewContent::Image(image) = load(&path, &[]) else {
            panic!("expected an image");
        };
        assert_eq!((image.width, image.height), (40, 30));
    }

    #[test]
    fn a_file_over_the_limit_is_too_large() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "big.txt", &[b'a'; 101]);
        assert_eq!(
            load_within(&path, &[], tiny_limit()),
            PreviewContent::TooLarge
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_followed() {
        let dir = TempDir::new().unwrap();
        let target = write(&dir, "real.txt", b"secret");
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(load(&link, &[]), PreviewContent::Unsupported);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_returns_at_once_instead_of_blocking() {
        let dir = TempDir::new().unwrap();
        let fifo = dir.path().join("pipe.txt");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(load(&fifo, &[]), PreviewContent::Unsupported);
    }

    #[test]
    fn a_folder_has_no_preview() {
        let dir = TempDir::new().unwrap();
        assert_eq!(load(dir.path(), &[]), PreviewContent::Unsupported);
    }

    #[test]
    fn heic_and_unknown_types_have_no_preview() {
        let dir = TempDir::new().unwrap();
        for name in ["photo.heic", "data.xyz"] {
            let path = write(&dir, name, b"not decoded");
            assert_eq!(load(&path, &[]), PreviewContent::Unsupported, "{name}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_picture_but_named_like_one_fails() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "broken.png", b"not a png");
        assert!(matches!(load(&path, &[]), PreviewContent::Failed(_)));
    }

    #[test]
    fn a_pdf_shows_its_first_page() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "filled.pdf",
            &pdf_bytes("0 0 1 rg 20 20 100 100 re f", "", &[]),
        );
        let PreviewContent::Page { image, page_count } = load(&path, &[]) else {
            panic!("expected a page");
        };
        assert_eq!(page_count, 1);
        assert!(image.width <= 1200 && image.height <= 1200);
    }

    #[test]
    fn an_answer_for_an_older_request_is_dropped() {
        let mut preview = DocumentPreview::default();
        let first = preview.request(Path::new("a.txt"), &[]).unwrap();
        let second = preview.request(Path::new("b.txt"), &[]).unwrap();
        preview.loaded(first.serial, PreviewContent::Unsupported);
        assert_eq!(preview.content(), None);
        preview.loaded(second.serial, PreviewContent::TooLarge);
        assert_eq!(preview.content(), Some(&PreviewContent::TooLarge));
    }

    #[test]
    fn the_same_path_makes_no_new_request() {
        let mut preview = DocumentPreview::default();
        assert!(preview.request(Path::new("a.txt"), &[]).is_some());
        assert!(preview.request(Path::new("a.txt"), &[]).is_none());
        assert!(preview.request(Path::new("b.txt"), &[]).is_some());
    }

    #[test]
    fn other_passages_for_the_same_path_make_a_new_request() {
        let mut preview = DocumentPreview::default();
        let company = vec!["Company A".to_owned()];
        assert!(preview.request(Path::new("a.txt"), &company).is_some());
        assert!(preview.request(Path::new("a.txt"), &company).is_none());
        let request = preview.request(Path::new("a.txt"), &[]).unwrap();
        assert!(request.passages.is_empty());
    }

    #[test]
    fn a_text_marks_the_passages_ignoring_case_and_joins_what_touches() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "note.txt",
            "Rechnung 4711\nFirma Müller und müller".as_bytes(),
        );
        let passages = ["MÜLLER".to_owned(), "4711".to_owned(), "nothing".to_owned()];
        let PreviewContent::Text {
            excerpt,
            highlights,
            ..
        } = load(&path, &passages)
        else {
            panic!("expected a text");
        };
        let marked: Vec<&str> = highlights
            .iter()
            .map(|range| &excerpt[range.clone()])
            .collect();
        assert_eq!(marked, ["4711", "Müller", "müller"]);
        let overlapping = highlight_ranges("aaaa", &["aa".to_owned(), "aaa".to_owned()]);
        assert_eq!(overlapping.len(), 1);
        assert_eq!(overlapping.first(), Some(&(0..4)));
    }

    #[test]
    fn clearing_drops_the_answer_on_its_way() {
        let mut preview = DocumentPreview::default();
        let request = preview.request(Path::new("a.txt"), &[]).unwrap();
        preview.clear();
        preview.loaded(request.serial, PreviewContent::TooLarge);
        assert_eq!(preview.content(), None);
        assert!(preview.request(Path::new("a.txt"), &[]).is_some());
    }

    #[tokio::test]
    async fn the_off_thread_load_returns_the_content() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"hello");
        assert_eq!(
            load_off_thread(path, Vec::new()).await,
            PreviewContent::Text {
                excerpt: "hello".to_owned(),
                truncated: false,
                highlights: Vec::new(),
            }
        );
    }
}
