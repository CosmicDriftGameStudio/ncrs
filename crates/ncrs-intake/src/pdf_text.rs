//! PDF files as a text source: the text layer, or page images for a PDF
//! without one, and the search for the pdfium library.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use pdfium_render::prelude::{PdfBitmapFormat, PdfRenderConfig, Pdfium};

use crate::location::without_verbatim_prefix;
use crate::page_budget::{collect_within_budget, MAX_SCAN_TOTAL_BYTES};
use crate::page_trim::trim_to_content;
use crate::text::{Extracted, PageImage, TextError, TextSource, MAX_TEXT_BYTES};

const PDF_HEADER: &[u8] = b"%PDF-";
/// The header may follow some leading bytes, as pdfium accepts it.
const PDF_HEADER_WINDOW: u64 = 1024;

/// Resolution of a rendered scan. OCR reads small print well from here on.
const SCAN_DPI: f32 = 200.0;
const POINTS_PER_INCH: f32 = 72.0;
/// A scanned book must not fill the memory with page images: later pages are
/// left out. A single page is capped in size for the same reason.
pub const MAX_SCAN_PAGES: usize = 20;
pub const MAX_SCAN_SIDE_PIXELS: f32 = 4000.0;
/// pdfium's grey-only bitmap format stays blank for these pages, so they are
/// rendered as BGRx and reduced to grey here.
const BYTES_PER_RENDERED_PIXEL: usize = 4;

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
        collect_within_budget(
            document.pages().iter().take(MAX_SCAN_PAGES),
            MAX_SCAN_TOTAL_BYTES,
            |page| Ok(trim_to_content(render_page(&page, path)?)),
        )
    }
}

/// Renders `page` as BGRx at the scan resolution, but at most
/// `MAX_SCAN_SIDE_PIXELS` on the longer side, and takes the grey of every pixel
/// row by row from the top left, without the padding at the end of a row.
fn render_page(
    page: &pdfium_render::prelude::PdfPage<'_>,
    path: &Path,
) -> Result<PageImage, TextError> {
    let unreadable = |reason: String| TextError::Unreadable {
        path: path.to_path_buf(),
        reason,
    };
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
    let pixels = raw
        .chunks_exact(stride)
        .flat_map(|row| {
            row.as_chunks::<BYTES_PER_RENDERED_PIXEL>()
                .0
                .iter()
                .take(width)
        })
        .map(grey_of_rendered)
        .collect();
    Ok(PageImage {
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        pixels,
    })
}

/// pdfium-render reverses the byte order of a BGRx bitmap by default, so the
/// bytes of a rendered pixel are red, green, blue, unused.
fn grey_of_rendered(pixel: &[u8; BYTES_PER_RENDERED_PIXEL]) -> u8 {
    let [red, green, blue, _unused] = *pixel;
    // Rec. 601 luma with the weights scaled to sum to 1024.
    let weighted = u32::from(red) * 306 + u32::from(green) * 601 + u32::from(blue) * 117;
    u8::try_from(weighted >> 10).unwrap_or(u8::MAX)
}

fn scaled_pixels(points: f32, scale: f32) -> i32 {
    ((points * scale).round().max(1.0)) as i32
}

impl TextSource for PdfText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        let file = File::open(path).map_err(|err| TextError::Io {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        self.extract_open(path, &file)
    }

    fn extract_open(&self, path: &Path, file: &File) -> Result<Extracted, TextError> {
        if !is_pdf(path, file)? {
            return Ok(Extracted::NotApplicable);
        }
        (&*file)
            .seek(SeekFrom::Start(0))
            .map_err(|err| TextError::Io {
                path: path.to_path_buf(),
                reason: err.to_string(),
            })?;
        let unreadable = |reason: String| TextError::Unreadable {
            path: path.to_path_buf(),
            reason,
        };
        let document = self
            .pdfium
            .load_pdf_from_reader(file, None)
            .map_err(|err| unreadable(format!("cannot open the PDF: {err:?}")))?;
        let mut pages = Vec::new();
        let mut collected: usize = 0;
        for page in document.pages().iter() {
            let text = page
                .text()
                .map_err(|err| unreadable(format!("cannot read a page: {err:?}")))?;
            let page_text = text.all();
            collected = collected.saturating_add(page_text.len());
            pages.push(page_text);
            if collected >= MAX_PDF_TEXT_BYTES {
                break;
            }
        }
        let joined = pages.join("\n");
        let text = cut_at_boundary(joined.trim(), MAX_PDF_TEXT_BYTES);
        if text.is_empty() {
            Self::render_pages(&document, path).map(Extracted::PageImages)
        } else {
            Ok(Extracted::Text(text.to_owned()))
        }
    }
}

/// The most text kept of one PDF, as for the other formats: what lies beyond
/// is not stored or searched, and memory does not grow with a book.
const MAX_PDF_TEXT_BYTES: usize = MAX_TEXT_BYTES;

/// `text` cut to at most `max` bytes, on a character boundary.
pub fn cut_at_boundary(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
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
        let file = File::open(path).map_err(|err| TextError::Io {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        self.extract_open(path, &file)
    }

    fn extract_open(&self, path: &Path, file: &File) -> Result<Extracted, TextError> {
        if is_pdf(path, file)? {
            Err(self.reason.clone())
        } else {
            Ok(Extracted::NotApplicable)
        }
    }
}

/// By the first bytes, not the extension.
fn is_pdf(path: &Path, file: &File) -> Result<bool, TextError> {
    let unreadable = |err: std::io::Error| TextError::Io {
        path: path.to_path_buf(),
        reason: err.to_string(),
    };
    let mut header = Vec::with_capacity(PDF_HEADER_WINDOW as usize);
    file.take(PDF_HEADER_WINDOW)
        .read_to_end(&mut header)
        .map_err(unreadable)?;
    Ok(header
        .windows(PDF_HEADER.len())
        .any(|window| window == PDF_HEADER))
}

fn holds_library(dir: &Path) -> bool {
    Pdfium::pdfium_platform_library_name_at_path(dir).is_file()
}

const LIBRARY_DIR_VARIABLE: &str = "NCRS_PDFIUM_DIR";

/// Where the app looks for the pdfium library, best place first: next to the
/// executable, on macOS also the `Frameworks` folder of the app bundle. Debug
/// builds and test builds (feature `test-support`) add `NCRS_PDFIUM_DIR` and the
/// folder the fetch script fills; release builds read no environment variable,
/// so nobody can make them load another library. When the executable was
/// started through a symlink, as winget does, the folder of the real file
/// comes first: the link's folder is shared and writable by the user, so a
/// foreign library there must not win.
pub fn pdfium_library_candidates() -> Vec<PathBuf> {
    let executable = std::env::current_exe().ok();
    let executable_dir = executable
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let resolved_dir = executable
        .and_then(|path| std::fs::canonicalize(path).ok())
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(without_verbatim_prefix);
    let development = cfg!(any(debug_assertions, feature = "test-support"))
        .then(|| std::env::var_os(LIBRARY_DIR_VARIABLE));
    candidates_in(executable_dir, resolved_dir, development)
}

/// `development` is `None` in release builds, otherwise the value of the
/// override variable.
fn candidates_in(
    executable_dir: Option<PathBuf>,
    resolved_dir: Option<PathBuf>,
    development: Option<Option<std::ffi::OsString>>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let different_executable =
        executable_dir.filter(|executable| Some(executable) != resolved_dir.as_ref());
    for dir in resolved_dir.into_iter().chain(different_executable) {
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
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::test_support::{image_only_pdf, pdf_bytes, pdf_text, text_pdf, write};

    #[test]
    fn a_long_pdf_text_is_cut_on_a_character_boundary() {
        let text = "ä".repeat(MAX_PDF_TEXT_BYTES);

        let cut = cut_at_boundary(&text, MAX_PDF_TEXT_BYTES);

        assert_eq!(cut.len(), MAX_PDF_TEXT_BYTES);
        assert_eq!(cut_at_boundary("short", MAX_PDF_TEXT_BYTES), "short");
        assert_eq!(cut_at_boundary("ä", 1), "");
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
        // Cut to the drawing and scaled, so no longer the 200 points at 200 dpi.
        assert!(image.width > 0 && image.height > 0);
        assert!(image.width.max(image.height) <= 2400);
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
    fn scan_pages_are_grey_by_the_luma_of_their_colour() {
        let dir = TempDir::new().unwrap();
        let cases: [(&str, u8); 2] = [("1 0 0", 76), ("0 0 1", 29)];
        for (colour, expected) in cases {
            let content = format!("{colour} rg 0 0 200 200 re f");
            let path = write(&dir, "fill.pdf", &pdf_bytes(&content, "", &[]));
            let Extracted::PageImages(images) = pdf_text().extract(&path).unwrap() else {
                panic!("expected page images");
            };
            assert!(
                images[0].pixels.iter().all(|p| p.abs_diff(expected) <= 1),
                "{colour}: {:?}..{:?}",
                images[0].pixels.iter().min(),
                images[0].pixels.iter().max()
            );
        }
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

    #[test]
    fn candidates_start_at_the_executable_and_development_ones_follow() {
        let executable = PathBuf::from("/app/bin");
        let release = candidates_in(Some(executable.clone()), None, None);
        assert_eq!(release.first(), Some(&executable));
        assert!(release
            .iter()
            .all(|dir| !dir.ends_with("target/pdfium/lib") && !dir.ends_with("target/pdfium/bin")));

        let overridden = PathBuf::from("/elsewhere");
        let debug = candidates_in(
            Some(executable.clone()),
            None,
            Some(Some(overridden.clone().into_os_string())),
        );
        let override_at = debug.iter().position(|dir| *dir == overridden).unwrap();
        let fetched_at = debug
            .iter()
            .position(|dir| dir.to_string_lossy().contains("target/pdfium"))
            .unwrap();
        assert_eq!(debug.first(), Some(&executable));
        assert!(override_at < fetched_at);
        let unset = candidates_in(Some(executable), None, Some(None));
        assert!(unset
            .iter()
            .any(|dir| dir.to_string_lossy().contains("target/pdfium")));
    }

    #[test]
    fn a_symlinked_launch_tries_the_real_folder_first_and_the_link_one_second() {
        let link_dir = PathBuf::from("/links");
        let real_dir = PathBuf::from("/packages/ncrs");
        let candidates = candidates_in(Some(link_dir.clone()), Some(real_dir.clone()), None);
        let link_at = candidates.iter().position(|dir| *dir == link_dir).unwrap();
        let real_at = candidates.iter().position(|dir| *dir == real_dir).unwrap();
        assert_eq!(real_at, 0);
        assert!(real_at < link_at);
        if cfg!(target_os = "macos") {
            assert!(candidates.contains(&real_dir.join("..").join("Frameworks")));
        }

        let same = candidates_in(Some(link_dir.clone()), Some(link_dir.clone()), None);
        assert_eq!(same, candidates_in(Some(link_dir), None, None));
    }

    #[test]
    fn a_verbatim_windows_prefix_is_dropped_so_the_same_folder_counts_once() {
        let plain = PathBuf::from(r"C:\app");
        let verbatim = PathBuf::from(r"\\?\C:\app");
        let same = candidates_in(
            Some(plain.clone()),
            Some(without_verbatim_prefix(verbatim)),
            None,
        );
        assert_eq!(same, candidates_in(Some(plain), None, None));
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
}
