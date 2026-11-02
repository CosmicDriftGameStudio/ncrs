use std::path::PathBuf;

use chrono::{DateTime, Local};
use ncrs_filing::chat_work::ChatAccess;
use ncrs_i18n::Language;

/// Whether the filing is up, so the chat has something to work with.
#[derive(Debug, Clone)]
pub enum Availability {
    Ready(ChatAccess),
    Starting,
    NeedsSetup,
}

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub lang: Language,
    pub availability: Availability,
    pub home: PathBuf,
    /// Folders the user files into.
    pub targets: Vec<PathBuf>,
    /// Folders where new files arrive.
    pub sources: Vec<PathBuf>,
    pub now: DateTime<Local>,
}
