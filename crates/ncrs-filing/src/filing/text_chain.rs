//! The text sources the intake reads documents with and where their models live.

use super::*;

/// The PDF source; when the pdfium library is missing, each PDF is held back
/// instead, and the other files go on. A scan is read by OCR once the user has
/// downloaded the models into `ocr_models_dir`; until then it yields no text.
/// On macOS the text recognition of the system comes first and the models are
/// the fallback when it fails.
///
/// The second value is the folder the library is missing from, when it is.
pub(in crate::filing) fn shipped_text_chain(
    ocr_models_dir: PathBuf,
) -> (TextChain, Option<PathBuf>) {
    let (chain, library_missing_from) = match PdfText::load_shipped() {
        Ok(pdf) => (TextChain::new().with_source(pdf), None),
        Err(err) => {
            let folder = match &err {
                TextError::LibraryMissing { path } => Some(path.clone()),
                _ => None,
            };
            (
                TextChain::new().with_source(PdfUnavailable::new(err)),
                folder,
            )
        }
    };
    (
        chain
            .with_source(PlainText)
            .with_source(OfficeText)
            .with_image_source(shipped_image_source(ocr_models_dir)),
        library_missing_from,
    )
}

#[cfg(target_os = "macos")]
pub(in crate::filing) fn shipped_image_source(
    ocr_models_dir: PathBuf,
) -> impl ImageTextSource + 'static {
    ImageTextWithFallback::new(
        ncrs_ocr_vision::VisionText,
        LazyOcrText::new(ocr_models_dir),
    )
}

#[cfg(not(target_os = "macos"))]
pub(in crate::filing) fn shipped_image_source(
    ocr_models_dir: PathBuf,
) -> impl ImageTextSource + 'static {
    LazyOcrText::new(ocr_models_dir)
}

/// Where the OCR models live: next to the index, so a portable data folder
/// carries them along.
pub fn ocr_models_dir(index_path: &Path) -> Option<PathBuf> {
    index_path
        .parent()
        .map(|data_dir| data_dir.join(OCR_MODELS_FOLDER))
}

/// Where the AI model lives, next to the index like the OCR models.
pub fn ai_models_dir(index_path: &Path) -> Option<PathBuf> {
    index_path
        .parent()
        .map(|data_dir| data_dir.join(ncrs_ai::LOCAL_MODEL_FOLDER))
}
