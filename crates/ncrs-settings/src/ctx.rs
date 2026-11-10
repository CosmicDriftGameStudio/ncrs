use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::keymap::Keymap;
use ncrs_i18n::Language;

use ncrs_ai::Secret;

use crate::ai_model::{DownloadError as AiDownloadError, DownloadJob};
use crate::ai_server::{AiCheckJob, AiCheckResult};
use crate::ocr_models::DownloadError as OcrDownloadError;
use crate::open_vsx::{ExtensionId, FetchError, FetchedExtension};
use crate::theme_fetch::{Installed, Pick};

/// What `update` reads from the app.
#[derive(Debug)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub keymap: &'a Arc<Keymap>,
    pub config_path: Option<&'a Path>,
    pub home: &'a Path,
    pub sandbox_active: bool,
    /// A page of Hermes or a dialog is up that nothing opens over.
    pub blocking_overlay_open: bool,
    /// Same, apart from the pages of Hermes, over which the settings and the
    /// model cards open.
    pub dialog_open: bool,
    pub ocr_models_dir: Option<PathBuf>,
    pub ai_models_dir: Option<PathBuf>,
    pub file_list_tags: &'a FileListTags,
    pub ai_is_on: bool,
    /// The log of what the own server used, next to the index; none while
    /// there is no index.
    pub usage_log: Option<PathBuf>,
    pub services: Services,
}

/// The downloads, which the tests of the app replace by fakes.
#[derive(Debug, Clone, Copy)]
pub struct Services {
    pub fetch_extension: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    pub install_theme: fn(&Pick, &Path) -> Result<Box<Installed>, FetchError>,
    pub download_ocr_models: fn(&Path, &AtomicU64) -> Result<(), OcrDownloadError>,
    pub download_ai_model: fn(&DownloadJob) -> Result<(), AiDownloadError>,
    /// "Test connection" of the own server.
    pub check_ai_server: fn(&AiCheckJob) -> AiCheckResult,
    /// Keeps the API key of the own server: the endpoint, whether the App
    /// Store build runs, the key.
    pub store_ai_key: fn(&str, bool, &Secret) -> Result<(), String>,
}
