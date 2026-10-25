//! Text from documents. A chain of sources: the first one that returns text
//! wins. A source that only finds page images (a scan) hands them on to the
//! image sources, which recognise the text in them.
//!
//! Text found here is data. Nothing in it triggers anything; it is only stored.

use std::fmt;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use pdfium_render::prelude::{PdfBitmapFormat, PdfRenderConfig, Pdfium};

const PDF_HEADER: &[u8] = b"%PDF-";

/// Resolution of a rendered scan. OCR reads small print well from here on.
const SCAN_DPI: f32 = 200.0;
const POINTS_PER_INCH: f32 = 72.0;
/// A scanned book must not fill the memory with page images: later pages are
/// left out. A single page is capped in size for the same reason.
const MAX_SCAN_PAGES: usize = 20;
const MAX_SCAN_SIDE_PIXELS: f32 = 4000.0;
/// pdfium's grey-only bitmap format stays blank for these pages, so they are
/// rendered as BGRx and reduced to grey here.
const BYTES_PER_RENDERED_PIXEL: usize = 4;

/// One rendered page: `pixels` holds `width * height` bytes, one per pixel, row
/// by row from the top left, without padding. 0 is black, 255 is white.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extracted {
    Text(String),
    /// The file has no text of its own but looks like a scan.
    PageImages(Vec<PageImage>),
    /// This source does not handle the file.
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextError {
    LibraryMissing { path: PathBuf },
    Unreadable { path: PathBuf, reason: String },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryMissing { path } => write!(
                f,
                "the PDF library (pdfium) was not found or could not be loaded from {}",
                path.display()
            ),
            Self::Unreadable { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for TextError {}

pub trait TextSource: Send + Sync + fmt::Debug {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError>;
}

/// Recognises text in page images, for example with OCR. `Ok(None)` when it
/// finds none.
pub trait ImageTextSource: Send + Sync + fmt::Debug {
    fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError>;
}

/// Asks `preferred` first; when it fails, `fallback` gets the same pages. The
/// chain itself stops at the first error, so this is where a failing source is
/// given a second chance.
#[derive(Debug)]
pub struct ImageTextWithFallback<P, F> {
    preferred: P,
    fallback: F,
}

impl<P, F> ImageTextWithFallback<P, F> {
    pub fn new(preferred: P, fallback: F) -> Self {
        Self {
            preferred,
            fallback,
        }
    }
}

impl<P: ImageTextSource, F: ImageTextSource> ImageTextSource for ImageTextWithFallback<P, F> {
    fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError> {
        self.preferred
            .recognise(images)
            .or_else(|_failed| self.fallback.recognise(images))
    }
}

#[derive(Debug, Default)]
pub struct TextChain {
    sources: Vec<Box<dyn TextSource>>,
    image_sources: Vec<Box<dyn ImageTextSource>>,
}

impl TextChain {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_source(mut self, source: impl TextSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    #[must_use]
    pub fn with_image_source(mut self, source: impl ImageTextSource + 'static) -> Self {
        self.image_sources.push(Box::new(source));
        self
    }

    /// The text of the file, `None` when no source finds any. The first error
    /// stops the chain.
    pub fn extract(&self, path: &Path) -> Result<Option<String>, TextError> {
        let mut images = Vec::new();
        for source in &self.sources {
            match source.extract(path)? {
                Extracted::Text(text) if !text.trim().is_empty() => return Ok(Some(text)),
                Extracted::Text(_) | Extracted::NotApplicable => {}
                Extracted::PageImages(found) => images.extend(found),
            }
        }
        if images.is_empty() {
            return Ok(None);
        }
        for source in &self.image_sources {
            if let Some(text) = source.recognise(&images)? {
                if !text.trim().is_empty() {
                    return Ok(Some(text));
                }
            }
        }
        Ok(None)
    }
}

/// The binding to pdfium can be made once per process; later loads share it.
static SHARED_PDFIUM: Mutex<Option<Arc<Pdfium>>> = Mutex::new(None);

/// Text layer of PDF files, or page images for a PDF without one.
#[derive(Debug)]
pub struct PdfText {
    pdfium: Arc<Pdfium>,
}

impl PdfText {
    /// Loads the pdfium library from `library_dir`.
    pub fn load(library_dir: &Path) -> Result<Self, TextError> {
        let library = Pdfium::pdfium_platform_library_name_at_path(library_dir);
        let missing = || TextError::LibraryMissing {
            path: library_dir.to_path_buf(),
        };
        let mut shared = SHARED_PDFIUM.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pdfium) = shared.as_ref() {
            return if library.is_file() {
                Ok(Self {
                    pdfium: Arc::clone(pdfium),
                })
            } else {
                Err(missing())
            };
        }
        if !library.is_file() {
            return Err(missing());
        }
        let bindings = Pdfium::bind_to_library(&library).map_err(|_not_loadable| missing())?;
        let pdfium = Arc::new(Pdfium::new(bindings));
        *shared = Some(Arc::clone(&pdfium));
        Ok(Self { pdfium })
    }

    /// Loads the library from the first of [`pdfium_library_candidates`] that
    /// holds it. When none does, the error names the folder next to the
    /// executable, where a user can put it.
    pub fn load_shipped() -> Result<Self, TextError> {
        let candidates = pdfium_library_candidates();
        match candidates.iter().find(|dir| holds_library(dir)) {
            Some(dir) => Self::load(dir),
            None => Err(TextError::LibraryMissing {
                path: candidates.into_iter().next().unwrap_or_default(),
            }),
        }
    }

    /// Every page of the file as page images, text layer or not.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn render_file(&self, path: &Path) -> Result<Vec<PageImage>, TextError> {
        let document =
            self.pdfium
                .load_pdf_from_file(path, None)
                .map_err(|err| TextError::Unreadable {
                    path: path.to_path_buf(),
                    reason: format!("cannot open the PDF: {err:?}"),
                })?;
        Self::render_pages(&document, path)
    }

    fn render_pages(
        document: &pdfium_render::prelude::PdfDocument<'_>,
        path: &Path,
    ) -> Result<Vec<PageImage>, TextError> {
        let unreadable = |reason: String| TextError::Unreadable {
            path: path.to_path_buf(),
            reason,
        };
        let mut images = Vec::new();
        for page in document.pages().iter().take(MAX_SCAN_PAGES) {
            let width_points = page.width().value;
            let height_points = page.height().value;
            let longest_side = width_points.max(height_points).max(1.0);
            let scale = (SCAN_DPI / POINTS_PER_INCH).min(MAX_SCAN_SIDE_PIXELS / longest_side);
            let config = PdfRenderConfig::new()
                .set_target_size(
                    scaled_pixels(width_points, scale),
                    scaled_pixels(height_points, scale),
                )
                .set_format(PdfBitmapFormat::BGRx);
            let bitmap = page
                .render_with_config(&config)
                .map_err(|err| unreadable(format!("cannot render a page: {err:?}")))?;
            let width = usize::try_from(bitmap.width()).unwrap_or(0);
            let height = usize::try_from(bitmap.height()).unwrap_or(0);
            let raw = bitmap.as_raw_bytes();
            let stride = raw.len().checked_div(height).unwrap_or(0);
            if width == 0 || stride < width * BYTES_PER_RENDERED_PIXEL {
                return Err(unreadable("a page rendered without pixels".to_owned()));
            }
            let pixels: Vec<u8> = raw
                .chunks_exact(stride)
                .flat_map(|row| {
                    row.as_chunks::<BYTES_PER_RENDERED_PIXEL>()
                        .0
                        .iter()
                        .take(width)
                })
                .map(grey_of_bgrx)
                .collect();
            images.push(PageImage {
                width: u32::try_from(width).unwrap_or(u32::MAX),
                height: u32::try_from(height).unwrap_or(u32::MAX),
                pixels,
            });
        }
        Ok(images)
    }
}

fn grey_of_bgrx(pixel: &[u8; BYTES_PER_RENDERED_PIXEL]) -> u8 {
    let [blue, green, red, _unused] = *pixel;
    // Rec. 601 luma with the weights scaled to sum to 1024.
    let weighted = u32::from(red) * 306 + u32::from(green) * 601 + u32::from(blue) * 117;
    u8::try_from(weighted >> 10).unwrap_or(u8::MAX)
}

fn scaled_pixels(points: f32, scale: f32) -> i32 {
    ((points * scale).round().max(1.0)) as i32
}

impl TextSource for PdfText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        if !is_pdf(path)? {
            return Ok(Extracted::NotApplicable);
        }
        let unreadable = |reason: String| TextError::Unreadable {
            path: path.to_path_buf(),
            reason,
        };
        let document = self
            .pdfium
            .load_pdf_from_file(path, None)
            .map_err(|err| unreadable(format!("cannot open the PDF: {err:?}")))?;
        let mut pages = Vec::new();
        for page in document.pages().iter() {
            let text = page
                .text()
                .map_err(|err| unreadable(format!("cannot read a page: {err:?}")))?;
            pages.push(text.all());
        }
        let joined = pages.join("\n");
        let text = joined.trim();
        if text.is_empty() {
            Self::render_pages(&document, path).map(Extracted::PageImages)
        } else {
            Ok(Extracted::Text(text.to_owned()))
        }
    }
}

/// Stands in for [`PdfText`] when the library cannot be loaded: every PDF
/// reports the reason, other files go on to the next source.
#[derive(Debug)]
pub struct PdfUnavailable {
    reason: TextError,
}

impl PdfUnavailable {
    pub fn new(reason: TextError) -> Self {
        Self { reason }
    }
}

impl TextSource for PdfUnavailable {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        if is_pdf(path)? {
            Err(self.reason.clone())
        } else {
            Ok(Extracted::NotApplicable)
        }
    }
}

/// By the first bytes, not the extension.
fn is_pdf(path: &Path) -> Result<bool, TextError> {
    let unreadable = |err: std::io::Error| TextError::Unreadable {
        path: path.to_path_buf(),
        reason: err.to_string(),
    };
    let mut header = Vec::with_capacity(PDF_HEADER.len());
    File::open(path)
        .map_err(unreadable)?
        .take(PDF_HEADER.len() as u64)
        .read_to_end(&mut header)
        .map_err(unreadable)?;
    Ok(header == PDF_HEADER)
}

fn holds_library(dir: &Path) -> bool {
    Pdfium::pdfium_platform_library_name_at_path(dir).is_file()
}

const LIBRARY_DIR_VARIABLE: &str = "NCRS_PDFIUM_DIR";

/// Where the app looks for the pdfium library, best place first: next to the
/// executable, on macOS also the `Frameworks` folder of the app bundle. Debug
/// builds add `NCRS_PDFIUM_DIR` and the folder the fetch script fills; release
/// builds read no environment variable, so nobody can make them load another
/// library.
pub fn pdfium_library_candidates() -> Vec<PathBuf> {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let development = cfg!(debug_assertions).then(|| std::env::var_os(LIBRARY_DIR_VARIABLE));
    candidates_in(executable_dir, development)
}

/// `development` is `None` in release builds, otherwise the value of the
/// override variable.
fn candidates_in(
    executable_dir: Option<PathBuf>,
    development: Option<Option<std::ffi::OsString>>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = executable_dir {
        if cfg!(target_os = "macos") {
            candidates.push(dir.clone());
            candidates.push(dir.join("..").join("Frameworks"));
        } else {
            candidates.push(dir);
        }
    }
    if let Some(override_dir) = development {
        candidates.extend(override_dir.map(PathBuf::from));
        let sub = if cfg!(windows) { "bin" } else { "lib" };
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/pdfium")
                .join(sub),
        );
    }
    candidates
}

/// The first of [`pdfium_library_candidates`] that holds the library, else the
/// folder next to the executable.
pub fn pdfium_library_dir() -> Option<PathBuf> {
    let candidates = pdfium_library_candidates();
    candidates
        .iter()
        .find(|dir| holds_library(dir))
        .or(candidates.first())
        .cloned()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::test_support::{pdf_bytes, pdf_text, text_pdf, write};

    fn image_only_pdf() -> Vec<u8> {
        pdf_bytes(
            "q 160 0 0 160 20 20 cm /Im1 Do Q",
            "/XObject<</Im1 5 0 R>>",
            &[
                "<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceGray\
               /BitsPerComponent 8/Filter/ASCIIHexDecode/Length 10>>\nstream\n00FFFF00>\nendstream"
                    .to_owned(),
            ],
        )
    }

    #[test]
    fn a_pdf_with_a_text_layer_yields_its_text() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "invoice.pdf", &text_pdf("Invoice 4711 Company A"));
        let extracted = pdf_text().extract(&path).unwrap();
        let Extracted::Text(text) = extracted else {
            panic!("expected text, got {extracted:?}");
        };
        assert!(text.contains("Invoice 4711 Company A"), "{text}");
    }

    #[test]
    fn a_pdf_without_a_text_layer_yields_page_images() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "scan.pdf", &image_only_pdf());
        let extracted = pdf_text().extract(&path).unwrap();
        let Extracted::PageImages(images) = extracted else {
            panic!("expected page images, got {extracted:?}");
        };
        assert_eq!(images.len(), 1);
        let image = &images[0];
        // 200 points at 200 dpi.
        assert!((540..=570).contains(&image.width), "{}", image.width);
        assert!((540..=570).contains(&image.height), "{}", image.height);
        assert_eq!(image.pixels.len(), (image.width * image.height) as usize);
        assert!(
            image.pixels.iter().any(|&pixel| pixel < 64),
            "min {:?} max {:?}",
            image.pixels.iter().min(),
            image.pixels.iter().max()
        );
        assert!(
            image.pixels.iter().any(|&pixel| pixel > 192),
            "no light pixels"
        );
    }

    #[test]
    fn a_missing_library_is_an_error_naming_the_folder() {
        let dir = TempDir::new().unwrap();
        let err = PdfText::load(dir.path()).unwrap_err();
        assert_eq!(
            err,
            TextError::LibraryMissing {
                path: dir.path().to_path_buf()
            }
        );
        assert!(err.to_string().contains(&dir.path().display().to_string()));
    }

    #[derive(Debug)]
    struct FixedImageSource(Result<Option<String>, TextError>);

    impl ImageTextSource for FixedImageSource {
        fn recognise(&self, _images: &[PageImage]) -> Result<Option<String>, TextError> {
            self.0.clone()
        }
    }

    fn failure() -> TextError {
        TextError::Unreadable {
            path: PathBuf::new(),
            reason: "unavailable".to_owned(),
        }
    }

    fn one_page() -> [PageImage; 1] {
        [PageImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        }]
    }

    #[test]
    fn the_preferred_source_wins_when_it_finds_text() {
        let source = ImageTextWithFallback::new(
            FixedImageSource(Ok(Some("preferred".to_owned()))),
            FixedImageSource(Ok(Some("fallback".to_owned()))),
        );
        assert_eq!(
            source.recognise(&one_page()),
            Ok(Some("preferred".to_owned()))
        );
    }

    #[test]
    fn the_fallback_is_asked_when_the_preferred_source_fails() {
        let source = ImageTextWithFallback::new(
            FixedImageSource(Err(failure())),
            FixedImageSource(Ok(Some("fallback".to_owned()))),
        );
        assert_eq!(
            source.recognise(&one_page()),
            Ok(Some("fallback".to_owned()))
        );
    }

    #[test]
    fn the_error_of_the_fallback_is_reported_when_both_fail() {
        let source = ImageTextWithFallback::new(
            FixedImageSource(Err(TextError::LibraryMissing {
                path: PathBuf::new(),
            })),
            FixedImageSource(Err(failure())),
        );
        assert_eq!(source.recognise(&one_page()), Err(failure()));
    }

    #[test]
    fn a_failing_source_in_a_chain_does_not_stop_the_fallback() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "scan.pdf", &image_only_pdf());
        let chain =
            TextChain::new()
                .with_source(pdf_text())
                .with_image_source(ImageTextWithFallback::new(
                    FixedImageSource(Err(failure())),
                    FixedImageSource(Ok(Some("fallback".to_owned()))),
                ));
        assert_eq!(chain.extract(&path), Ok(Some("fallback".to_owned())));
    }

    #[test]
    fn candidates_start_at_the_executable_and_development_ones_follow() {
        let executable = PathBuf::from("/app/bin");
        let release = candidates_in(Some(executable.clone()), None);
        assert_eq!(release.first(), Some(&executable));
        assert!(release
            .iter()
            .all(|dir| !dir.ends_with("target/pdfium/lib") && !dir.ends_with("target/pdfium/bin")));

        let overridden = PathBuf::from("/elsewhere");
        let debug = candidates_in(
            Some(executable.clone()),
            Some(Some(overridden.clone().into_os_string())),
        );
        let override_at = debug.iter().position(|dir| *dir == overridden).unwrap();
        let fetched_at = debug
            .iter()
            .position(|dir| dir.to_string_lossy().contains("target/pdfium"))
            .unwrap();
        assert_eq!(debug.first(), Some(&executable));
        assert!(override_at < fetched_at);
        let unset = candidates_in(Some(executable), Some(None));
        assert!(unset
            .iter()
            .any(|dir| dir.to_string_lossy().contains("target/pdfium")));
    }

    #[test]
    fn a_folder_without_the_library_reports_that_folder() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            PdfText::load(dir.path()).unwrap_err(),
            TextError::LibraryMissing {
                path: dir.path().to_path_buf()
            }
        );
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_not_applicable() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.pdf", b"just a note, whatever the extension");
        assert_eq!(pdf_text().extract(&path).unwrap(), Extracted::NotApplicable);
    }

    #[test]
    fn a_broken_pdf_is_an_error() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "broken.bin", b"%PDF-1.4\nthis is not a document");
        assert!(matches!(
            pdf_text().extract(&path),
            Err(TextError::Unreadable { .. })
        ));
    }

    #[test]
    fn an_unavailable_pdf_source_reports_the_reason_for_pdfs_only() {
        let dir = TempDir::new().unwrap();
        let reason = TextError::LibraryMissing {
            path: dir.path().to_path_buf(),
        };
        let source = PdfUnavailable::new(reason.clone());
        let pdf = write(&dir, "a.pdf", b"%PDF-1.4");
        let note = write(&dir, "a.txt", b"text");
        assert_eq!(source.extract(&pdf), Err(reason));
        assert_eq!(source.extract(&note), Ok(Extracted::NotApplicable));
    }

    #[derive(Debug)]
    struct Fixed(Result<Extracted, TextError>);

    impl TextSource for Fixed {
        fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
            self.0.clone()
        }
    }

    #[derive(Debug)]
    struct Recognises(Option<&'static str>);

    impl ImageTextSource for Recognises {
        fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError> {
            Ok(self.0.filter(|_| !images.is_empty()).map(str::to_owned))
        }
    }

    fn some_image() -> PageImage {
        PageImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        }
    }

    fn text(value: &str) -> Fixed {
        Fixed(Ok(Extracted::Text(value.to_owned())))
    }

    #[test]
    fn the_first_source_with_text_wins() {
        let chain = TextChain::new()
            .with_source(Fixed(Ok(Extracted::NotApplicable)))
            .with_source(text("  "))
            .with_source(text("first"))
            .with_source(text("second"));
        assert_eq!(chain.extract(Path::new("x")), Ok(Some("first".to_owned())));
    }

    #[test]
    fn a_source_error_stops_the_chain() {
        let reason = TextError::LibraryMissing {
            path: PathBuf::from("lib"),
        };
        let chain = TextChain::new()
            .with_source(Fixed(Err(reason.clone())))
            .with_source(text("never reached"));
        assert_eq!(chain.extract(Path::new("x")), Err(reason));
    }

    #[test]
    fn page_images_go_to_the_image_sources_in_order() {
        let chain = TextChain::new()
            .with_source(Fixed(Ok(Extracted::PageImages(vec![some_image()]))))
            .with_image_source(Recognises(None))
            .with_image_source(Recognises(Some("read from the scan")));
        assert_eq!(
            chain.extract(Path::new("x")),
            Ok(Some("read from the scan".to_owned()))
        );
    }

    #[test]
    fn page_images_without_an_image_source_yield_no_text() {
        let chain =
            TextChain::new().with_source(Fixed(Ok(Extracted::PageImages(vec![some_image()]))));
        assert_eq!(chain.extract(Path::new("x")), Ok(None));
    }

    #[test]
    fn an_empty_chain_yields_no_text() {
        assert_eq!(TextChain::new().extract(Path::new("x")), Ok(None));
    }
}
