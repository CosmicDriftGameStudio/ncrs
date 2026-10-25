//! Text from scanned pages with the ocrs engine. The models it needs are not
//! shipped: the user is asked first, and until they are in the models folder
//! (and verified) a scan simply yields no text.
//!
//! Text found here is data, like any other text of a document.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use sha2::{Digest as _, Sha256};

use crate::text::{ImageTextSource, PageImage, TextError};

/// The only host the models are fetched from.
pub const OCR_MODELS_HOST: &str = "ocrs-models.s3-accelerate.amazonaws.com";
/// Where the models come from, shown to the user before anything is sent.
pub const OCR_MODELS_PROJECT: &str = "github.com/robertknight/ocrs-models";
/// The folder inside the data folder that holds the models.
pub const OCR_MODELS_FOLDER: &str = "ocr-models";

/// One model file with the size and checksum it must have. Anything else is
/// refused, whether it comes from the network or from the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl OcrModelFile {
    pub fn url(&self) -> String {
        format!("https://{OCR_MODELS_HOST}/{}", self.name)
    }
}

/// The pins. scripts/fetch-ocr-models.sh carries the same values for CI; change
/// both together.
pub const OCR_MODEL_FILES: [OcrModelFile; 2] = [
    OcrModelFile {
        name: "text-detection.onnx",
        size: 2_499_479,
        sha256: "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe",
    },
    OcrModelFile {
        name: "text-recognition.onnx",
        size: 9_713_177,
        sha256: "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5",
    },
];

const DOWNLOAD_BUFFER_SUFFIX: &str = ".part";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrModelError {
    Unreadable {
        file: &'static str,
        reason: String,
    },
    WrongSize {
        file: &'static str,
    },
    WrongHash {
        file: &'static str,
    },
    /// The files passed the check but the engine cannot use them.
    Engine {
        reason: String,
    },
    WriteFailed {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for OcrModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { file, reason } => write!(f, "{file}: {reason}"),
            Self::WrongSize { file } => write!(f, "{file} does not have the expected size"),
            Self::WrongHash { file } => write!(f, "{file} does not match its checksum"),
            Self::Engine { reason } => {
                write!(f, "the text recognition models do not load: {reason}")
            }
            Self::WriteFailed { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for OcrModelError {}

fn hex_of(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Size and checksum of bytes already in memory.
pub fn verify_model_bytes(file: &OcrModelFile, bytes: &[u8]) -> Result<(), OcrModelError> {
    if u64::try_from(bytes.len()) != Ok(file.size) {
        return Err(OcrModelError::WrongSize { file: file.name });
    }
    if hex_of(&Sha256::digest(bytes)) == file.sha256 {
        Ok(())
    } else {
        Err(OcrModelError::WrongHash { file: file.name })
    }
}

/// The verified bytes of a model, `None` when the file is not there. Reads at
/// most one byte more than the pin, so a wrong file cannot fill the memory.
fn read_model(dir: &Path, file: &OcrModelFile) -> Result<Option<Vec<u8>>, OcrModelError> {
    let unreadable = |err: io::Error| OcrModelError::Unreadable {
        file: file.name,
        reason: err.to_string(),
    };
    let opened = match File::open(dir.join(file.name)) {
        Ok(opened) => opened,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(unreadable(err)),
    };
    let mut bytes = Vec::new();
    opened
        .take(file.size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    verify_model_bytes(file, &bytes)?;
    Ok(Some(bytes))
}

/// Counts and hashes what passes through on its way to the file.
struct CheckingWriter<W> {
    inner: W,
    hasher: Sha256,
    written: u64,
}

impl<W: Write> Write for CheckingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let accepted = self.inner.write(buf)?;
        self.hasher.update(buf.get(..accepted).unwrap_or_default());
        self.written = self.written.saturating_add(accepted as u64);
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Writes what `source` delivers into `dir` as `file`. At most the pinned size
/// plus one byte is read; the data goes to a temporary file next to the target,
/// is checked for size and checksum, and only then renamed into place. On any
/// error the temporary file is gone and nothing replaced the target.
pub fn install_model(
    dir: &Path,
    file: &OcrModelFile,
    source: impl io::Read,
) -> Result<(), OcrModelError> {
    let temp_path = dir.join(format!("{}{DOWNLOAD_BUFFER_SUFFIX}", file.name));
    let write_failed = |location: &Path| {
        let path = location.to_path_buf();
        move |err: io::Error| OcrModelError::WriteFailed {
            path,
            reason: err.to_string(),
        }
    };
    let result = fs::create_dir_all(dir)
        .map_err(write_failed(dir))
        .and_then(|()| write_checked(&temp_path, file, source))
        .and_then(|()| {
            fs::rename(&temp_path, dir.join(file.name)).map_err(write_failed(&temp_path))
        });
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn write_checked(
    temp_path: &Path,
    file: &OcrModelFile,
    source: impl io::Read,
) -> Result<(), OcrModelError> {
    let write_failed = |err: io::Error| OcrModelError::WriteFailed {
        path: temp_path.to_path_buf(),
        reason: err.to_string(),
    };
    // A leftover of an earlier try, possibly a link: never written through.
    let _ = fs::remove_file(temp_path);
    let out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(write_failed)?;
    let mut writer = CheckingWriter {
        inner: out,
        hasher: Sha256::new(),
        written: 0,
    };
    io::copy(&mut source.take(file.size.saturating_add(1)), &mut writer).map_err(write_failed)?;
    writer.inner.sync_all().map_err(write_failed)?;
    if writer.written != file.size {
        return Err(OcrModelError::WrongSize { file: file.name });
    }
    if hex_of(&writer.hasher.finalize()) != file.sha256 {
        return Err(OcrModelError::WrongHash { file: file.name });
    }
    Ok(())
}

/// The ocrs engine with its two models loaded.
pub struct OcrText {
    engine: OcrEngine,
}

impl fmt::Debug for OcrText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OcrText").finish_non_exhaustive()
    }
}

impl OcrText {
    /// Loads both models from `dir` after checking them. `None` when one of
    /// them is not there (yet); a file that is there but wrong is an error and
    /// is never handed to the engine. The bytes that were checked are the bytes
    /// that are loaded.
    pub fn load(dir: &Path) -> Result<Option<Self>, OcrModelError> {
        let [detection_file, recognition_file] = &OCR_MODEL_FILES;
        let (Some(detection), Some(recognition)) = (
            read_model(dir, detection_file)?,
            read_model(dir, recognition_file)?,
        ) else {
            return Ok(None);
        };
        let load = |file: &OcrModelFile, bytes: Vec<u8>| {
            Model::load(bytes).map_err(|err| OcrModelError::Engine {
                reason: format!("{}: {err}", file.name),
            })
        };
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(load(detection_file, detection)?),
            recognition_model: Some(load(recognition_file, recognition)?),
            ..OcrEngineParams::default()
        })
        .map_err(|err| OcrModelError::Engine {
            reason: err.to_string(),
        })?;
        Ok(Some(Self { engine }))
    }

    fn read_page(&self, page: &PageImage) -> Result<String, String> {
        let source = ImageSource::from_bytes(&page.pixels, (page.width, page.height))
            .map_err(|err| err.to_string())?;
        let input = self
            .engine
            .prepare_input(source)
            .map_err(|err| err.to_string())?;
        self.engine.get_text(&input).map_err(|err| err.to_string())
    }
}

impl ImageTextSource for OcrText {
    fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError> {
        let mut pages = Vec::new();
        for page in images {
            let text = self
                .read_page(page)
                .map_err(|reason| TextError::Unreadable {
                    path: PathBuf::new(),
                    reason: format!("text recognition failed: {reason}"),
                })?;
            if !text.trim().is_empty() {
                pages.push(text);
            }
        }
        Ok((!pages.is_empty()).then(|| pages.join("\n\n")))
    }
}

/// An image source that looks for the models when the first scan arrives and
/// keeps the engine once it has found and verified them. Until then it yields
/// no text, without an error and without any network, so a download takes
/// effect without a restart.
pub struct LazyOcrText {
    models_dir: PathBuf,
    engine: Mutex<Option<Arc<OcrText>>>,
}

impl fmt::Debug for LazyOcrText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyOcrText")
            .field("models_dir", &self.models_dir)
            .finish_non_exhaustive()
    }
}

impl LazyOcrText {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            engine: Mutex::new(None),
        }
    }

    /// The lock is held while loading, so workers that meet the first scan
    /// together load the models once.
    fn engine(&self) -> Result<Option<Arc<OcrText>>, TextError> {
        let mut cached = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(engine) = cached.as_ref() {
            return Ok(Some(Arc::clone(engine)));
        }
        let loaded = OcrText::load(&self.models_dir).map_err(|err| TextError::Unreadable {
            path: self.models_dir.clone(),
            reason: err.to_string(),
        })?;
        *cached = loaded.map(Arc::new);
        Ok(cached.clone())
    }
}

impl ImageTextSource for LazyOcrText {
    fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError> {
        match self.engine()? {
            Some(engine) => engine.recognise(images),
            None => Ok(None),
        }
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
    use std::io::Cursor;

    use tempfile::TempDir;

    use super::*;
    use crate::test_support::{pdf_text, rendered_pages, text_pdf, write};

    const MODELS_DIR_VARIABLE: &str = "NCRS_OCR_MODELS_DIR";

    fn models_dir() -> PathBuf {
        std::env::var_os(MODELS_DIR_VARIABLE).map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ocr-models"),
            PathBuf::from,
        )
    }

    fn engine() -> OcrText {
        let dir = models_dir();
        OcrText::load(&dir)
            .unwrap_or_else(|err| panic!("{err}"))
            .unwrap_or_else(|| {
                panic!(
                    "no OCR models in {}. Run scripts/fetch-ocr-models.sh target/ocr-models, \
                     or set {MODELS_DIR_VARIABLE} to the folder with the models",
                    dir.display()
                )
            })
    }

    /// A pin for bytes that are not a model: same shape, own size and checksum.
    fn pin_of(bytes: &[u8]) -> OcrModelFile {
        let sha256: &'static str = Box::leak(hex_of(&Sha256::digest(bytes)).into_boxed_str());
        OcrModelFile {
            name: "text-detection.onnx",
            size: bytes.len() as u64,
            sha256,
        }
    }

    #[test]
    fn the_words_of_a_rendered_page_come_back() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "invoice.pdf", &text_pdf("Invoice 4711 Company A"));
        let pages = rendered_pages(&pdf_text(), &path);
        let text = engine().recognise(&pages).unwrap().expect("text");
        let lower = text.to_lowercase();
        for word in ["invoice", "4711", "company"] {
            assert!(lower.contains(word), "{word} missing in {text:?}");
        }
    }

    #[test]
    fn a_blank_page_yields_no_text() {
        let blank = PageImage {
            width: 200,
            height: 200,
            pixels: vec![255; 200 * 200],
        };
        assert_eq!(engine().recognise(&[blank]), Ok(None));
    }

    #[test]
    fn a_page_with_inconsistent_pixels_is_an_error() {
        let broken = PageImage {
            width: 10,
            height: 10,
            pixels: vec![0; 7],
        };
        assert!(matches!(
            engine().recognise(&[broken]),
            Err(TextError::Unreadable { .. })
        ));
    }

    #[test]
    fn without_the_models_a_scan_yields_no_text_and_no_error() {
        let dir = TempDir::new().unwrap();
        let source = LazyOcrText::new(dir.path().join("ocr-models"));
        let page = PageImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        assert_eq!(source.recognise(&[page]), Ok(None));
        assert!(
            !dir.path().join("ocr-models").exists(),
            "nothing is created"
        );
    }

    #[test]
    fn a_model_that_appears_later_is_used_without_a_restart() {
        let target = TempDir::new().unwrap();
        let source = LazyOcrText::new(target.path().to_path_buf());
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "invoice.pdf", &text_pdf("Invoice 4711 Company A"));
        let pages = rendered_pages(&pdf_text(), &path);
        assert_eq!(source.recognise(&pages), Ok(None));
        for file in OCR_MODEL_FILES {
            fs::copy(models_dir().join(file.name), target.path().join(file.name)).unwrap();
        }
        let text = source.recognise(&pages).unwrap().expect("text");
        assert!(text.to_lowercase().contains("invoice"), "{text:?}");
    }

    #[test]
    fn a_model_with_a_wrong_checksum_is_named_and_never_loaded() {
        let dir = TempDir::new().unwrap();
        let [detection, recognition] = OCR_MODEL_FILES;
        for file in [detection, recognition] {
            let mut bytes: Vec<u8> = vec![0; usize::try_from(file.size).unwrap()];
            bytes[0] = 1;
            fs::write(dir.path().join(file.name), bytes).unwrap();
        }
        assert_eq!(
            OcrText::load(dir.path()).err(),
            Some(OcrModelError::WrongHash {
                file: detection.name
            })
        );
        let source = LazyOcrText::new(dir.path().to_path_buf());
        let page = PageImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        let Err(TextError::Unreadable { reason, .. }) = source.recognise(&[page]) else {
            panic!("expected an error");
        };
        assert!(reason.contains(detection.name), "{reason}");
    }

    #[test]
    fn a_model_of_the_wrong_size_is_refused_before_it_is_hashed() {
        let dir = TempDir::new().unwrap();
        let [detection, _recognition] = OCR_MODEL_FILES;
        fs::write(dir.path().join(detection.name), b"short").unwrap();
        assert_eq!(
            OcrText::load(dir.path()).err(),
            Some(OcrModelError::WrongSize {
                file: detection.name
            })
        );
    }

    #[test]
    fn a_download_is_renamed_into_place_only_after_the_checks() {
        let dir = TempDir::new().unwrap();
        let bytes = b"model bytes";
        let file = pin_of(bytes);
        install_model(dir.path(), &file, Cursor::new(bytes.to_vec())).unwrap();
        assert_eq!(fs::read(dir.path().join(file.name)).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no leftovers");
    }

    #[test]
    fn a_download_with_a_wrong_checksum_leaves_nothing_behind() {
        let dir = TempDir::new().unwrap();
        let file = pin_of(b"model bytes");
        let err =
            install_model(dir.path(), &file, Cursor::new(b"model BYTES".to_vec())).unwrap_err();
        assert_eq!(err, OcrModelError::WrongHash { file: file.name });
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_wrong_download_does_not_replace_a_good_file() {
        let dir = TempDir::new().unwrap();
        let file = pin_of(b"model bytes");
        install_model(dir.path(), &file, Cursor::new(b"model bytes".to_vec())).unwrap();
        install_model(dir.path(), &file, Cursor::new(b"model BYTES".to_vec())).unwrap_err();
        assert_eq!(
            fs::read(dir.path().join(file.name)).unwrap(),
            b"model bytes"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_download_longer_than_the_pin_is_cut_and_refused() {
        let dir = TempDir::new().unwrap();
        let file = pin_of(b"model bytes");
        let endless = io::repeat(b'x');
        let err = install_model(dir.path(), &file, endless).unwrap_err();
        assert_eq!(err, OcrModelError::WrongSize { file: file.name });
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_download_that_breaks_off_leaves_nothing_behind() {
        struct Breaks;
        impl io::Read for Breaks {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::ConnectionReset))
            }
        }
        let dir = TempDir::new().unwrap();
        let file = pin_of(b"model bytes");
        assert!(matches!(
            install_model(dir.path(), &file, Breaks),
            Err(OcrModelError::WriteFailed { .. })
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_pins_are_well_formed() {
        for file in OCR_MODEL_FILES {
            assert_eq!(file.sha256.len(), 64, "{}", file.name);
            assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(file
                .url()
                .starts_with(&format!("https://{OCR_MODELS_HOST}/")));
        }
    }
}
