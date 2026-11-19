//! Text from documents. A chain of sources: the first one that returns text
//! wins. A source that only finds page images (a scan) hands them on to the
//! image sources, which recognise the text in them.
//!
//! Text found here is data. Nothing in it triggers anything; it is only stored.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::reading_marker::ReadingMarker;

/// The most text kept of one document, in every format: what lies beyond is
/// not stored or searched, and memory does not grow with a book.
pub const MAX_TEXT_BYTES: usize = 1 << 20;

/// One rendered page: `pixels` holds `width * height` bytes, one per pixel, row
/// by row from the top left, without padding. 0 is black, 255 is white.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The first page of a PDF for a preview: `rgba` holds `width * height * 4`
/// bytes, row by row from the top left, without padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub page_count: usize,
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
    LibraryMissing {
        path: PathBuf,
    },
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    /// The app crashed while reading this file at two starts in a row, so it
    /// is not read until a person asks for it.
    Quarantined {
        path: PathBuf,
    },
    /// Opening or reading the file failed: the file itself may be fine, so it
    /// must be read again later.
    Io {
        path: PathBuf,
        reason: String,
    },
    /// The recognition models could not be loaded: nothing is wrong with the
    /// file, so it must be read again once they work.
    EngineUnavailable {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryMissing { path } => write!(
                f,
                "the PDF library (pdfium) was not found or could not be loaded from {}",
                path.display()
            ),
            Self::Quarantined { path } => write!(
                f,
                "{}: the app crashed while reading this file",
                path.display()
            ),
            Self::Unreadable { path, reason }
            | Self::Io { path, reason }
            | Self::EngineUnavailable { path, reason } => {
                write!(f, "{}: {reason}", path.display())
            }
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

    /// False when `recognise` answers `Ok(None)` for lack of an engine (models
    /// not downloaded), not because the images hold no text. Asked after
    /// `recognise`.
    fn engine_ready(&self) -> bool {
        true
    }
}

/// What the text chain found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainReading {
    Text(String),
    NoText {
        /// An image source read the page images and found no text.
        recognition_ran: bool,
    },
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
        match self.preferred.recognise(images) {
            Ok(text) => Ok(text),
            // A fallback that finds nothing must not turn the failure into
            // "the page is blank".
            Err(failed) => match self.fallback.recognise(images)? {
                Some(text) => Ok(Some(text)),
                None => Err(failed),
            },
        }
    }

    fn engine_ready(&self) -> bool {
        self.preferred.engine_ready() || self.fallback.engine_ready()
    }
}

#[derive(Debug, Default)]
pub struct TextChain {
    sources: Vec<Box<dyn TextSource>>,
    image_sources: Vec<Box<dyn ImageTextSource>>,
    marker: Option<ReadingMarker>,
}

impl TextChain {
    /// Guards the reading of files with a marker file; see [`ReadingMarker`].
    #[must_use]
    pub fn with_crash_marker(mut self, file: PathBuf) -> Self {
        self.marker = Some(ReadingMarker::open(file));
        self
    }

    /// The app is closing: files being read now are not named for the next
    /// start.
    pub fn closing(&self) {
        if let Some(marker) = &self.marker {
            marker.closing();
        }
    }

    /// Reads `path` again even if the app crashed on it before.
    pub fn forget_crash(&self, path: &Path) {
        if let Some(marker) = &self.marker {
            marker.forget_crash(path);
        }
    }

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
        Ok(match self.read(path)? {
            ChainReading::Text(text) => Some(text),
            ChainReading::NoText { .. } => None,
        })
    }

    /// Like [`Self::extract`], and says whether a file without text was
    /// really read by text recognition, or never reached an engine that could
    /// read it (no image source, models not downloaded, picture not decodable).
    pub fn read(&self, path: &Path) -> Result<ChainReading, TextError> {
        let Some(marker) = &self.marker else {
            return self.read_sources(path);
        };
        if marker.is_quarantined(path) {
            return Err(TextError::Quarantined {
                path: path.to_path_buf(),
            });
        }
        marker.begin(path);
        let reading = self.read_sources(path);
        marker.end(path);
        reading
    }

    fn read_sources(&self, path: &Path) -> Result<ChainReading, TextError> {
        let mut images = Vec::new();
        for source in &self.sources {
            match source.extract(path)? {
                Extracted::Text(text) if !text.trim().is_empty() => {
                    return Ok(ChainReading::Text(text))
                }
                Extracted::Text(_) | Extracted::NotApplicable => {}
                Extracted::PageImages(found) => images.extend(found),
            }
        }
        if images.is_empty() {
            return Ok(ChainReading::NoText {
                recognition_ran: false,
            });
        }
        for source in &self.image_sources {
            if let Some(text) = source.recognise(&images).map_err(|error| match error {
                // The engines read pixels and do not know the file.
                TextError::Unreadable {
                    path: empty,
                    reason,
                } if empty.as_os_str().is_empty() => TextError::Unreadable {
                    path: path.to_path_buf(),
                    reason,
                },
                other => other,
            })? {
                if !text.trim().is_empty() {
                    return Ok(ChainReading::Text(text));
                }
            }
        }
        Ok(ChainReading::NoText {
            recognition_ran: self
                .image_sources
                .iter()
                .any(|source| source.engine_ready()),
        })
    }
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
    use crate::test_support::{image_only_pdf, pdf_text, write};

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
    fn a_failure_of_the_preferred_source_is_not_hidden_by_a_fallback_that_finds_nothing() {
        let source = ImageTextWithFallback::new(
            FixedImageSource(Err(failure())),
            FixedImageSource(Ok(None)),
        );
        assert_eq!(source.recognise(&one_page()), Err(failure()));
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
    fn a_file_without_text_says_whether_text_recognition_read_it() {
        let images = || Fixed(Ok(Extracted::PageImages(vec![some_image()])));
        let ran = TextChain::new()
            .with_source(images())
            .with_image_source(FixedImageSource(Ok(None)));
        let without_source = TextChain::new().with_source(images());
        let undecodable = TextChain::new()
            .with_source(Fixed(Ok(Extracted::NotApplicable)))
            .with_image_source(FixedImageSource(Ok(None)));

        assert_eq!(
            ran.read(Path::new("x")),
            Ok(ChainReading::NoText {
                recognition_ran: true
            })
        );
        for chain in [without_source, undecodable] {
            assert_eq!(
                chain.read(Path::new("x")),
                Ok(ChainReading::NoText {
                    recognition_ran: false
                })
            );
        }
    }

    #[test]
    fn an_empty_chain_yields_no_text() {
        assert_eq!(TextChain::new().extract(Path::new("x")), Ok(None));
    }
}
