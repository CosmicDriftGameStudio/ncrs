//! The card that asks before the models for reading scanned pages are
//! downloaded, and the download itself. Nothing here runs unless the user
//! pressed the button of the card; the download is blocking and belongs on a
//! blocking thread. The card's state has no widgets and no network: `App` owns
//! it, the view is `ui::ocr_models`.

pub mod view;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ncrs_intake::{install_model, OcrModelError, OcrModelFile, OCR_MODELS_HOST, OCR_MODEL_FILES};

use crate::ai_model::CountingReader;
use crate::open_vsx::{self, FetchError};

/// The only host a model may come from, redirects included.
const MODEL_HOSTS: [&str; 1] = [OCR_MODELS_HOST];

pub use ncrs_config::keymap::system_reads_scans;

/// What can go wrong between pressing the button and having the files in
/// place. The card words it with the detail in English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    NoDataFolder,
    Network(FetchError),
    Install(OcrModelError),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDataFolder => write!(f, "no data folder"),
            Self::Install(error) => write!(f, "{error}"),
            Self::Network(error) => match error {
                FetchError::Status(code) => write!(f, "the server answered {code}"),
                FetchError::NoConnection(detail) => write!(f, "no connection: {detail}"),
                FetchError::Timeout => write!(f, "the server did not answer in time"),
                FetchError::Tls(detail) => write!(f, "secure connection failed: {detail}"),
                FetchError::Network(detail) => write!(f, "{detail}"),
                FetchError::DownloadRefused { host } => write!(f, "{host} is not allowed"),
                FetchError::TooManyRedirects => write!(f, "forwarded too often"),
                FetchError::TooLarge => write!(f, "larger than expected"),
                FetchError::InvalidId
                | FetchError::NotFound(_)
                | FetchError::BadAnswer(_)
                | FetchError::NoDownloadLink
                | FetchError::BadPackage(_)
                | FetchError::NoThemes
                | FetchError::MissingFile(_)
                | FetchError::UnsafePath(_)
                | FetchError::FileTooLarge(_)
                | FetchError::BadJson { .. }
                | FetchError::NoConfigPath
                | FetchError::Config(_)
                | FetchError::WriteFailed { .. } => write!(f, "unexpected answer"),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrModelsCard {
    /// Where the files will be saved; `None` without a data folder.
    target: Option<PathBuf>,
    downloading: bool,
    error: Option<DownloadError>,
    received: Arc<AtomicU64>,
    /// The download of this card just ended with the files in place.
    finished: bool,
    /// The files were all there when the card opened.
    installed: bool,
}

impl OcrModelsCard {
    pub fn new(target: Option<PathBuf>) -> Self {
        Self {
            target,
            downloading: false,
            error: None,
            received: Arc::new(AtomicU64::new(0)),
            finished: false,
            installed: false,
        }
    }

    /// A card for models that are already in place.
    pub fn installed(target: Option<PathBuf>) -> Self {
        Self {
            installed: true,
            ..Self::new(target)
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Bytes read and bytes in all, while a download runs.
    pub fn progress(&self) -> Option<(u64, u64)> {
        self.downloading.then(|| {
            (
                self.received.load(Ordering::Relaxed),
                OCR_MODEL_FILES.iter().map(|file| file.size).sum(),
            )
        })
    }

    /// The counter the download of this card fills.
    pub fn received(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.received)
    }

    pub fn target(&self) -> Option<&Path> {
        self.target.as_deref()
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading
    }

    pub fn error(&self) -> Option<&DownloadError> {
        self.error.as_ref()
    }

    /// The button: the folder to download into, or the reason there is none.
    /// A download that runs is not started twice.
    pub fn start(&mut self) -> Option<PathBuf> {
        if self.downloading {
            return None;
        }
        let Some(target) = self.target.clone() else {
            self.error = Some(DownloadError::NoDataFolder);
            return None;
        };
        self.error = None;
        self.received.store(0, Ordering::Relaxed);
        self.downloading = true;
        Some(target)
    }

    /// The download is done. `true` when the files are in place; the card then
    /// goes away. An answer nobody waits for changes nothing.
    pub fn finish(&mut self, result: Result<(), DownloadError>) -> bool {
        if !self.downloading {
            return false;
        }
        self.downloading = false;
        match result {
            Ok(()) => {
                self.finished = true;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
}

/// Downloads both model files into `dir`. Each is held to its pinned size and
/// checksum and put in place only when it passes; the first failure stops.
pub fn download_models(dir: &Path, received: &AtomicU64) -> Result<(), DownloadError> {
    let agent = open_vsx::agent();
    OCR_MODEL_FILES
        .iter()
        .try_for_each(|file| download_model(&agent, dir, file, received))
}

fn download_model(
    agent: &ureq::Agent,
    dir: &Path,
    file: &OcrModelFile,
    received: &AtomicU64,
) -> Result<(), DownloadError> {
    let mut url = file.url();
    for hop in 0..=open_vsx::MAX_REDIRECTS {
        open_vsx::check_host_in(&url, &MODEL_HOSTS).map_err(DownloadError::Network)?;
        let mut response = agent
            .get(&url)
            .call()
            .map_err(|error| DownloadError::Network(open_vsx::network_error(error)))?;
        let status = response.status().as_u16();
        if open_vsx::REDIRECT_STATUSES.contains(&status) {
            if hop == open_vsx::MAX_REDIRECTS {
                return Err(DownloadError::Network(FetchError::TooManyRedirects));
            }
            url = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            continue;
        }
        if status != 200 {
            return Err(DownloadError::Network(FetchError::Status(status)));
        }
        if response
            .body()
            .content_length()
            .is_some_and(|declared| declared > file.size)
        {
            return Err(DownloadError::Network(FetchError::TooLarge));
        }
        let reader = response
            .body_mut()
            .with_config()
            .limit(file.size.saturating_add(1))
            .reader();
        return install_model(dir, file, CountingReader::new(reader, received))
            .map_err(DownloadError::Install);
    }
    Err(DownloadError::Network(FetchError::TooManyRedirects))
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
mod tests;
