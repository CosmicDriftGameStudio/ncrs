use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::keymap::Keymap;
use ncrs_i18n::Language;

use crate::ai_model::{DownloadError as AiDownloadError, DownloadJob};
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
    /// A page of the Ablage or a dialog is up that nothing opens over.
    pub blocking_overlay_open: bool,
    /// Same, apart from the overview, over which the settings and the model
    /// cards open.
    pub overlay_apart_from_overview_open: bool,
    pub overview_open: bool,
    pub ocr_models_dir: Option<PathBuf>,
    pub ai_models_dir: Option<PathBuf>,
    pub file_list_tags: &'a FileListTags,
    pub ai_is_on: bool,
    pub services: Services,
}

/// The downloads, which the tests of the app replace by fakes.
#[derive(Debug, Clone, Copy)]
pub struct Services {
    pub fetch_extension: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    pub install_theme: fn(&Pick, &Path) -> Result<Box<Installed>, FetchError>,
    pub download_ocr_models: fn(&Path, &AtomicU64) -> Result<(), OcrDownloadError>,
    pub download_ai_model: fn(&DownloadJob) -> Result<(), AiDownloadError>,
}
