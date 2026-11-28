use std::path::{Path, PathBuf};

use std::sync::Arc;

use chrono::{DateTime, Local};
use ncrs_ai::ChatModel;
use ncrs_filing::chat_work::ChatAccess;
use ncrs_i18n::Language;

/// Whether the filing is up, so the chat has something to work with.
#[derive(Debug, Clone)]
pub enum Availability {
    Ready(Box<ChatAccess>),
    Starting,
    /// The module tried to start and stopped; the module's reason.
    Failed(String),
    /// The built-in model cannot run; the sentence comes from the problem,
    /// not from a technical reason.
    ModelFailed(ncrs_ai::ModelProblem),
    Off,
}

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub lang: Language,
    pub availability: Availability,
    pub home: PathBuf,
    /// Folders the user files into.
    pub targets: Vec<PathBuf>,
    /// The folders pinned in the drive menu.
    pub favourites: Vec<PathBuf>,
    /// Folders where new files arrive.
    pub sources: Vec<PathBuf>,
    pub now: DateTime<Local>,
    /// The filing setup's check of a folder: its canonical path, or the
    /// reason it is not a folder. Blocking.
    pub resolve_folder: fn(&Path) -> Result<PathBuf, String>,
    /// Whether the AI setting is on; off, no sentence goes to the model.
    pub ai_on: bool,
    /// Whether the build may change files: a locked trial lets Hermes read but
    /// not file, trash or undo.
    pub changes_allowed: bool,
    /// The App Store build: it removes no worktrees, so the card offers none.
    pub store_build: bool,
    /// The name of the drive the card is about.
    pub drive_name: String,
    /// `None` in a build without the local language model.
    pub model: Option<Arc<dyn ChatModel>>,
}
