//! The card that asks before the AI model is downloaded, and the download
//! itself. Nothing here runs unless the user pressed the button of the card:
//! the only way to a [`DownloadJob`] is [`AiModelCard::start`]. The download is
//! blocking and belongs on a blocking thread; it continues where an earlier one
//! stopped. The card's state has no widgets and no network: `App` owns it, the
//! view is `ui::ai_model`.

pub mod view;

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use ncrs_ai::{ModelFileError, ModelPin, PartialDownload, LOCAL_MODEL};

use crate::open_vsx::{self, FetchError};

/// Hugging Face, and the CDN hosts below hf.co it forwards a download to.
pub const AI_MODEL_HOSTS: [&str; 2] = [AI_MODEL_HOST, ".hf.co"];

/// The host the card names; the rest of the list are the hosts it forwards to.
pub const AI_MODEL_HOST: &str = "huggingface.co";

/// Where the model is made and under which license, as the card says it.
pub const AI_MODEL_PROJECT: &str = "huggingface.co/Qwen/Qwen3-0.6B-GGUF";
pub const AI_MODEL_LICENSE: &str = "Apache-2.0";

/// What can go wrong between pressing the button and having the file in
/// place. The card words it with the detail in English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    NoDataFolder,
    /// The user stopped it. Not a failure: the bytes so far stay.
    Cancelled,
    Network(FetchError),
    File(ModelFileError),
}

impl From<ModelFileError> for DownloadError {
    fn from(error: ModelFileError) -> Self {
        match error {
            ModelFileError::Cancelled => Self::Cancelled,
            other => Self::File(other),
        }
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDataFolder => write!(f, "no data folder"),
            Self::Cancelled => write!(f, "stopped"),
            Self::File(error) => write!(f, "{error}"),
            Self::Network(error) => {
                write!(
                    f,
                    "{}",
                    crate::ocr_models::DownloadError::Network(error.clone())
                )
            }
        }
    }
}

/// The right to download into one folder, handed out by the card's button and
/// by nothing else. `stop` on the card ends the download it belongs to.
#[derive(Debug)]
pub struct DownloadJob {
    dir: PathBuf,
    cancel: Arc<AtomicBool>,
    /// Bytes of the file that are on disk, read by the card for its bar.
    received: Arc<AtomicU64>,
}

/// What the card tells after its download ended with the file in place: what
/// became of the AI switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The AI was off; the switch is being written.
    TurningOn,
    /// The AI was off and is on now, and runs with the new model.
    TurnedOn,
    /// The AI was off and is on now; the module picks it up at the next start.
    TurnedOnAtNextStart,
    /// The AI was on and waited for the file.
    InUse,
    /// The AI is still off: the switch could not be written.
    StillOff,
}

#[derive(Debug, Clone)]
pub struct AiModelCard {
    /// Where the file will be saved; `None` without a data folder.
    target: Option<PathBuf>,
    /// Bytes of an earlier download are there, the next one continues them.
    has_partial: bool,
    downloading: bool,
    paused: bool,
    error: Option<DownloadError>,
    cancel: Option<Arc<AtomicBool>>,
    received: Arc<AtomicU64>,
    /// The file was complete when the card opened.
    installed: bool,
    /// This card's download just ended with the file in place.
    finished: bool,
    outcome: Option<InstallOutcome>,
}

impl PartialEq for DownloadJob {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir && Arc::ptr_eq(&self.cancel, &other.cancel)
    }
}

impl Eq for DownloadJob {}

impl AiModelCard {
    /// `has_partial` is looked up when the card is made, so the view never
    /// touches the disk.
    pub fn new(target: Option<PathBuf>, has_partial: bool) -> Self {
        Self {
            target,
            has_partial,
            downloading: false,
            paused: false,
            error: None,
            cancel: None,
            received: Arc::new(AtomicU64::new(0)),
            installed: false,
            finished: false,
            outcome: None,
        }
    }

    /// A card for a model that is already in place: it says so and closes.
    pub fn installed(target: Option<PathBuf>) -> Self {
        Self {
            installed: true,
            ..Self::new(target, false)
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed
    }

    /// The download this card ran ended with the file in place.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Lets a test draw the bar at a chosen point of a running download.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_received_for_test(&self, bytes: u64) {
        self.received.store(bytes, Ordering::Relaxed);
    }

    pub fn outcome(&self) -> Option<InstallOutcome> {
        self.outcome
    }

    pub fn set_outcome(&mut self, outcome: InstallOutcome) {
        if self.finished {
            self.outcome = Some(outcome);
        }
    }

    /// Bytes on disk and bytes in all, while a download runs.
    pub fn progress(&self) -> Option<(u64, u64)> {
        self.downloading
            .then(|| (self.received.load(Ordering::Relaxed), LOCAL_MODEL.size))
    }

    pub fn target(&self) -> Option<&Path> {
        self.target.as_deref()
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading
    }

    /// The user asked to stop and the download has not yet noticed.
    pub fn is_stopping(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// The last download was stopped by the user and can be continued.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The button says "Continue download" instead of "Download".
    pub fn continues(&self) -> bool {
        self.has_partial
    }

    pub fn error(&self) -> Option<&DownloadError> {
        self.error.as_ref()
    }

    /// The button: the job to run, or the reason there is none. A download that
    /// runs is not started twice.
    pub fn start(&mut self) -> Option<DownloadJob> {
        if self.downloading {
            return None;
        }
        let Some(dir) = self.target.clone() else {
            self.error = Some(DownloadError::NoDataFolder);
            return None;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let received = Arc::new(AtomicU64::new(0));
        self.received = Arc::clone(&received);
        self.error = None;
        self.paused = false;
        self.downloading = true;
        self.cancel = Some(Arc::clone(&cancel));
        Some(DownloadJob {
            dir,
            cancel,
            received,
        })
    }

    /// The Stop button. The download ends at its next chunk.
    pub fn stop(&mut self) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// The download is over. `true` when the file is in place; the card then
    /// goes away. An answer nobody waits for changes nothing.
    pub fn finish(&mut self, result: Result<(), DownloadError>) -> bool {
        if !self.downloading {
            return false;
        }
        self.downloading = false;
        self.cancel = None;
        match result {
            Ok(()) => {
                self.finished = true;
                true
            }
            Err(DownloadError::Cancelled) => {
                self.paused = true;
                self.has_partial = true;
                false
            }
            Err(error) => {
                self.has_partial = match &error {
                    DownloadError::File(
                        ModelFileError::Incomplete | ModelFileError::Interrupted(_),
                    ) => true,
                    DownloadError::File(
                        ModelFileError::TooLarge
                        | ModelFileError::WrongSize
                        | ModelFileError::WrongHash,
                    ) => false,
                    DownloadError::NoDataFolder
                    | DownloadError::Cancelled
                    | DownloadError::Network(_)
                    | DownloadError::File(
                        ModelFileError::Cancelled
                        | ModelFileError::WriteFailed { .. }
                        | ModelFileError::NotARegularFile,
                    ) => self.has_partial,
                };
                self.error = Some(error);
                false
            }
        }
    }
}

/// The model file is in `dir` with the pinned size. Not hashed: a download only
/// renames a file that passed the check, and hashing 639 MB does not belong in
/// the code that opens a card.
pub fn model_is_installed(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(LOCAL_MODEL.name))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == LOCAL_MODEL.size)
}

/// Downloads the model into the folder of `job`. Only a running card hands out
/// a job, so this never runs unasked.
pub fn download_ai_model(job: &DownloadJob) -> Result<(), DownloadError> {
    download_with(
        &open_vsx::agent(),
        LOCAL_MODEL.url,
        &|url| open_vsx::check_host_in(url, &AI_MODEL_HOSTS),
        job,
        &LOCAL_MODEL,
    )
}

/// More than this many attempts that each added bytes still end the download;
/// the part stays, so "Continue download" goes on.
const MAX_RESUMES: usize = 50;

/// One attempt per connection: the agent's global timeout caps each, so a slow
/// link needs several. An attempt that ended in the middle of the body and
/// added bytes is followed by another one that asks for the rest; one that
/// added nothing ends the download with its error.
fn download_with(
    agent: &ureq::Agent,
    start_url: &str,
    check_host: &dyn Fn(&str) -> Result<(), FetchError>,
    job: &DownloadJob,
    pin: &ModelPin,
) -> Result<(), DownloadError> {
    let mut last = Err(DownloadError::Cancelled);
    for _ in 0..=MAX_RESUMES {
        let mut grew = false;
        last = attempt(agent, start_url, check_host, job, pin, &mut grew);
        let broken_off = matches!(
            last,
            Err(DownloadError::File(
                ModelFileError::Interrupted(_) | ModelFileError::Incomplete
            ))
        );
        if !(broken_off && grew) {
            break;
        }
    }
    last
}

/// A single connection. `grew` tells whether it added bytes to the part.
fn attempt(
    agent: &ureq::Agent,
    start_url: &str,
    check_host: &dyn Fn(&str) -> Result<(), FetchError>,
    job: &DownloadJob,
    pin: &ModelPin,
    grew: &mut bool,
) -> Result<(), DownloadError> {
    let mut partial = PartialDownload::open(&job.dir, pin)?;
    let mut url = start_url.to_owned();
    for hop in 0..=open_vsx::MAX_REDIRECTS {
        if job.cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        check_host(&url).map_err(DownloadError::Network)?;
        let offset = partial.offset();
        if offset == pin.size {
            return Ok(partial.finish()?);
        }
        let mut request = agent.get(&url);
        if offset > 0 {
            request = request.header("Range", &format!("bytes={offset}-"));
        }
        let mut response = request
            .call()
            .map_err(|error| DownloadError::Network(open_vsx::network_error(error)))?;
        let status = response.status().as_u16();
        if open_vsx::REDIRECT_STATUSES.contains(&status) {
            if hop == open_vsx::MAX_REDIRECTS {
                return Err(DownloadError::Network(FetchError::TooManyRedirects));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            url = absolute_location(&url, location);
            continue;
        }
        let remaining = match status {
            206 => {
                let range = response
                    .headers()
                    .get("content-range")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if !range_continues(range, offset, pin.size) {
                    partial.restart()?;
                    return Err(DownloadError::Network(FetchError::BadAnswer(
                        "the server answered with another part of the file".to_owned(),
                    )));
                }
                pin.size.saturating_sub(offset)
            }
            200 => {
                partial.restart()?;
                pin.size
            }
            416 if offset == pin.size => return Ok(partial.finish()?),
            other => return Err(DownloadError::Network(FetchError::Status(other))),
        };
        if response
            .body()
            .content_length()
            .is_some_and(|declared| declared > remaining)
        {
            return Err(DownloadError::Network(FetchError::TooLarge));
        }
        let reader = response
            .body_mut()
            .with_config()
            .limit(remaining.saturating_add(1))
            .reader();
        let before = partial.offset();
        job.received.store(before, Ordering::Relaxed);
        let counted = CountingReader::new(reader, &job.received);
        let appended = partial.append(counted, &job.cancel);
        *grew = partial.offset() > before;
        appended?;
        return Ok(partial.finish()?);
    }
    Err(DownloadError::Network(FetchError::TooManyRedirects))
}

/// Passes the bytes on and keeps the running total for the card's bar.
#[derive(Debug)]
pub struct CountingReader<'a, R> {
    inner: R,
    base: u64,
    total: &'a AtomicU64,
}

impl<'a, R> CountingReader<'a, R> {
    /// Counts on from what `total` holds now.
    pub(crate) fn new(inner: R, total: &'a AtomicU64) -> Self {
        Self {
            inner,
            base: total.load(Ordering::Relaxed),
            total,
        }
    }
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.base += read as u64;
        self.total.store(self.base, Ordering::Relaxed);
        Ok(read)
    }
}

/// `Content-Range: bytes <offset>-<last>/<total>` for exactly the bytes asked
/// for of a file of `size`.
fn range_continues(header: &str, offset: u64, size: u64) -> bool {
    let from_offset = format!("bytes {offset}-");
    header.starts_with(&from_offset)
        && header
            .rsplit_once('/')
            .and_then(|(_, total)| total.parse::<u64>().ok())
            == Some(size)
}

/// A `Location` that only names a path stays on the host that sent it.
fn absolute_location(current: &str, location: &str) -> String {
    if !location.starts_with('/') || location.starts_with("//") {
        return location.to_owned();
    }
    let origin_end = current
        .split_once("://")
        .map(|(scheme, rest)| scheme.len() + 3 + rest.find('/').unwrap_or(rest.len()))
        .unwrap_or_default();
    format!(
        "{}{location}",
        current.get(..origin_end).unwrap_or_default()
    )
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
