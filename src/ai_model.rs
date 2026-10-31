//! The card that asks before the AI model is downloaded, and the download
//! itself. Nothing here runs unless the user pressed the button of the card:
//! the only way to a [`DownloadJob`] is [`AiModelCard::start`]. The download is
//! blocking and belongs on a blocking thread; it continues where an earlier one
//! stopped. The card's state has no widgets and no network: `App` owns it, the
//! view is `ui::ai_model`.

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
    #[cfg(test)]
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
mod tests {
    use std::fs;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::Mutex;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use sha2::{Digest as _, Sha256};

    use super::*;

    fn card() -> AiModelCard {
        AiModelCard::new(Some(PathBuf::from("data/ai-models")), false)
    }

    #[test]
    fn the_card_starts_only_on_the_button_and_only_once() {
        let mut card = card();
        assert!(!card.is_downloading());
        let job = card.start().unwrap();
        assert_eq!(job.dir, PathBuf::from("data/ai-models"));
        assert!(card.is_downloading());
        assert_eq!(card.start(), None, "no second download");
    }

    #[test]
    fn without_a_data_folder_nothing_starts_and_the_reason_is_shown() {
        let mut card = AiModelCard::new(None, false);
        assert_eq!(card.start(), None);
        assert!(!card.is_downloading());
        assert_eq!(card.error(), Some(&DownloadError::NoDataFolder));
    }

    #[test]
    fn stopping_is_shown_until_the_download_has_ended() {
        let mut card = card();
        let job = card.start().unwrap();
        assert!(!card.is_stopping());
        card.stop();
        assert!(card.is_stopping());
        assert!(job.cancel.load(Ordering::Relaxed));
        assert!(!card.finish(Err(DownloadError::Cancelled)));
        assert!(!card.is_stopping());
        assert!(!card.is_downloading());
    }

    #[test]
    fn a_stopped_download_is_paused_and_not_an_error() {
        let mut card = card();
        assert!(!card.continues());
        card.start().unwrap();
        card.stop();
        assert!(!card.finish(Err(DownloadError::Cancelled)));
        assert!(card.is_paused());
        assert!(card.continues());
        assert_eq!(card.error(), None);

        assert!(card.start().is_some(), "the button continues");
        assert!(!card.is_paused());
    }

    #[test]
    fn a_card_made_with_bytes_on_disk_offers_to_continue() {
        let card = AiModelCard::new(Some(PathBuf::from("data/ai-models")), true);
        assert!(card.continues());
    }

    #[test]
    fn a_failed_download_keeps_the_card_with_the_reason_and_can_be_tried_again() {
        let mut card = card();
        card.start().unwrap();
        let error = DownloadError::File(ModelFileError::WrongHash);
        assert!(!card.finish(Err(error.clone())));
        assert_eq!(card.error(), Some(&error));
        assert!(!card.is_downloading());
        assert!(!card.continues(), "a bad file is gone");
        assert!(card.start().is_some());
        assert_eq!(card.error(), None, "a new try ends the complaint");
    }

    #[test]
    fn a_broken_off_download_is_continued_by_the_next_one() {
        let mut card = card();
        card.start().unwrap();
        assert!(!card.finish(Err(DownloadError::File(ModelFileError::Incomplete))));
        assert!(card.continues());
    }

    #[test]
    fn a_finished_download_closes_the_card() {
        let mut card = card();
        card.start().unwrap();
        assert!(card.finish(Ok(())));
    }

    #[test]
    fn an_answer_nobody_waits_for_changes_nothing() {
        let mut card = card();
        assert!(!card.finish(Ok(())));
        assert!(!card.finish(Err(DownloadError::NoDataFolder)));
        assert_eq!(card.error(), None);
    }

    #[test]
    fn the_hosts_of_the_model_pass_and_others_do_not() {
        for url in [
            LOCAL_MODEL.url,
            "https://cas-bridge.xethub.hf.co/a",
            "https://cdn-lfs.hf.co/a",
        ] {
            assert_eq!(
                open_vsx::check_host_in(url, &AI_MODEL_HOSTS),
                Ok(()),
                "{url}"
            );
        }
        for url in [
            "http://huggingface.co/a",
            "https://hf.co/a",
            "https://evilhf.co/a",
            "https://huggingface.co.evil.example/a",
            "https://user@huggingface.co/a",
            "https://open-vsx.org/a",
        ] {
            assert!(
                matches!(
                    open_vsx::check_host_in(url, &AI_MODEL_HOSTS),
                    Err(FetchError::DownloadRefused { .. })
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn a_location_with_only_a_path_stays_on_the_host_that_sent_it() {
        assert_eq!(
            absolute_location("https://huggingface.co/a/b?x=1", "/api/c"),
            "https://huggingface.co/api/c"
        );
        assert_eq!(
            absolute_location("https://huggingface.co", "/api/c"),
            "https://huggingface.co/api/c"
        );
        assert_eq!(
            absolute_location("https://huggingface.co/a", "https://cdn.hf.co/x"),
            "https://cdn.hf.co/x"
        );
        assert_eq!(
            absolute_location("https://huggingface.co/a", "//evil.example/x"),
            "//evil.example/x"
        );
    }

    #[test]
    fn only_the_bytes_asked_for_of_the_right_file_count_as_a_continuation() {
        assert!(range_continues("bytes 100-199/200", 100, 200));
        assert!(!range_continues("bytes 0-199/200", 100, 200));
        assert!(!range_continues("bytes 100-199/300", 100, 200));
        assert!(!range_continues("bytes 100-199/*", 100, 200));
        assert!(!range_continues("", 100, 200));
    }

    // A server on this machine, one request at a time, so that the download
    // is tried against real HTTP without a network.

    struct Seen {
        range: Option<String>,
    }

    enum Reply {
        /// Honours `Range` when `honour_range`; sends only `cut_after` bytes of
        /// the body and then hangs up when that is set.
        Serve {
            honour_range: bool,
            cut_after: Option<usize>,
        },
        Redirect(String),
    }

    struct Server {
        port: u16,
        seen: Arc<Mutex<Vec<Seen>>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Server {
        fn start(data: Vec<u8>, script: impl Fn(usize) -> Reply + Send + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let (seen_by_thread, stop_flag) = (Arc::clone(&seen), Arc::clone(&stop));
                std::thread::spawn(move || {
                    for incoming in listener.incoming() {
                        if stop_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        let Ok(stream) = incoming else { continue };
                        let index = seen_by_thread.lock().unwrap().len();
                        let request = read_request(&stream);
                        let reply = script(index);
                        seen_by_thread.lock().unwrap().push(request);
                        answer(stream, &data, &reply, seen_by_thread.lock().unwrap().last());
                    }
                })
            };
            Self {
                port,
                seen,
                stop,
                thread: Some(thread),
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/model.gguf", self.port)
        }

        fn requests(&self) -> usize {
            self.seen.lock().unwrap().len()
        }

        fn range_of(&self, index: usize) -> Option<String> {
            self.seen.lock().unwrap()[index].range.clone()
        }

        fn check_host(&self, url: &str) -> Result<(), FetchError> {
            if url.starts_with(&format!("http://127.0.0.1:{}/", self.port)) {
                Ok(())
            } else {
                Err(FetchError::DownloadRefused {
                    host: url.to_owned(),
                })
            }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect(("127.0.0.1", self.port));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn read_request(stream: &TcpStream) -> Seen {
        let mut range = None;
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("range") {
                    range = Some(value.trim().to_owned());
                }
            }
        }
        Seen { range }
    }

    fn answer(mut stream: TcpStream, data: &[u8], reply: &Reply, seen: Option<&Seen>) {
        let head = match reply {
            Reply::Redirect(to) => {
                format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            }
            Reply::Serve { honour_range, .. } => {
                let start = seen
                    .and_then(|request| request.range.as_deref())
                    .and_then(|range| range.strip_prefix("bytes="))
                    .and_then(|range| range.strip_suffix('-'))
                    .and_then(|digits| digits.parse::<usize>().ok())
                    .filter(|start| *honour_range && *start > 0)
                    .unwrap_or_default();
                if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len() - 1,
                        data.len(),
                        data.len() - start
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len()
                    )
                }
            }
        };
        let _ = stream.write_all(head.as_bytes());
        if let Reply::Serve {
            honour_range,
            cut_after,
        } = reply
        {
            let start = if head.starts_with("HTTP/1.1 206") && *honour_range {
                head.split("bytes ")
                    .nth(1)
                    .and_then(|rest| rest.split('-').next())
                    .and_then(|digits| digits.parse::<usize>().ok())
                    .unwrap_or_default()
            } else {
                0
            };
            let rest = &data[start..];
            let sent = &rest[..cut_after.map_or(rest.len(), |cut| cut.min(rest.len()))];
            let _ = stream.write_all(sent);
        }
        let _ = stream.flush();
        let _ = stream.shutdown(Shutdown::Both);
    }

    fn test_agent() -> ureq::Agent {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build();
        ureq::Agent::new_with_config(config)
    }

    fn model_bytes() -> Vec<u8> {
        (0..200_000)
            .map(|index: u32| u8::try_from(index % 251).unwrap_or_default())
            .collect()
    }

    fn pin_of(bytes: &[u8]) -> &'static ModelPin {
        let sha256: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Box::leak(Box::new(ModelPin {
            name: "model.gguf",
            size: bytes.len() as u64,
            sha256: Box::leak(sha256.into_boxed_str()),
            url: "https://example.invalid/model.gguf",
        }))
    }

    fn job_in(dir: &Path, cancelled: bool) -> DownloadJob {
        DownloadJob {
            dir: dir.to_path_buf(),
            cancel: Arc::new(AtomicBool::new(cancelled)),
            received: Arc::new(AtomicU64::new(0)),
        }
    }

    fn run(server: &Server, job: &DownloadJob, pin: &ModelPin) -> Result<(), DownloadError> {
        download_with(
            &test_agent(),
            &server.url(),
            &|url| server.check_host(url),
            job,
            pin,
        )
    }

    fn run_once(server: &Server, job: &DownloadJob, pin: &ModelPin) -> Result<(), DownloadError> {
        attempt(
            &test_agent(),
            &server.url(),
            &|url| server.check_host(url),
            job,
            pin,
            &mut false,
        )
    }

    fn part_len(dir: &Path) -> u64 {
        fs::metadata(dir.join("model.gguf.part")).unwrap().len()
    }

    #[test]
    fn a_wrong_checksum_is_rejected_and_leaves_nothing() {
        let data = model_bytes();
        let mut other = data.clone();
        other[1000] ^= 0xFF;
        let server = Server::start(data, |_| Reply::Serve {
            honour_range: true,
            cut_after: None,
        });
        let dir = tempfile::tempdir().unwrap();

        let result = run(&server, &job_in(dir.path(), false), pin_of(&other));

        assert_eq!(result, Err(DownloadError::File(ModelFileError::WrongHash)));
        assert!(!dir.path().join("model.gguf").exists());
        assert!(!dir.path().join("model.gguf.part").exists());
    }

    #[test]
    fn a_broken_off_download_resumes_with_range() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |index| Reply::Serve {
            honour_range: true,
            cut_after: (index == 0).then_some(70_000),
        });
        let dir = tempfile::tempdir().unwrap();

        assert!(run_once(&server, &job_in(dir.path(), false), pin).is_err());
        let kept = part_len(dir.path());
        assert!(kept > 0 && kept < pin.size, "{kept} bytes kept");
        assert!(!dir.path().join("model.gguf").exists());

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));

        assert_eq!(server.requests(), 2);
        assert_eq!(server.range_of(0), None);
        assert_eq!(server.range_of(1), Some(format!("bytes={kept}-")));
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
        assert!(!dir.path().join("model.gguf.part").exists());
    }

    #[test]
    fn the_counter_ends_at_the_size_of_the_file() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data, |_| Reply::Serve {
            honour_range: true,
            cut_after: None,
        });
        let dir = tempfile::tempdir().unwrap();
        let job = job_in(dir.path(), false);

        assert_eq!(run(&server, &job, pin), Ok(()));

        assert_eq!(job.received.load(Ordering::Relaxed), pin.size);
    }

    #[test]
    fn a_resumed_download_counts_from_the_bytes_on_disk() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data, |index| Reply::Serve {
            honour_range: true,
            cut_after: (index == 0).then_some(70_000),
        });
        let dir = tempfile::tempdir().unwrap();
        assert!(run_once(&server, &job_in(dir.path(), false), pin).is_err());
        let kept = part_len(dir.path());
        let job = job_in(dir.path(), false);

        assert_eq!(run(&server, &job, pin), Ok(()));

        assert_eq!(job.received.load(Ordering::Relaxed), pin.size);
        assert!(kept > 0 && kept < pin.size);
    }

    #[test]
    fn the_card_reports_progress_only_while_downloading() {
        let mut card = card();
        assert_eq!(card.progress(), None);
        card.start().unwrap();
        assert_eq!(card.progress(), Some((0, LOCAL_MODEL.size)));
        assert!(card.finish(Ok(())));
        assert_eq!(card.progress(), None);
        assert!(card.is_finished());
    }

    #[test]
    fn a_download_cut_twice_goes_on_by_itself_with_the_right_ranges() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |index| Reply::Serve {
            honour_range: true,
            cut_after: (index < 2).then_some(50_000),
        });
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));

        assert_eq!(server.requests(), 3);
        assert_eq!(server.range_of(0), None);
        assert_eq!(server.range_of(1), Some("bytes=50000-".to_owned()));
        assert_eq!(server.range_of(2), Some("bytes=100000-".to_owned()));
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
    }

    #[test]
    fn a_server_that_delivers_nothing_new_ends_the_download_with_an_error() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data, |_| Reply::Serve {
            honour_range: true,
            cut_after: Some(0),
        });
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("model.gguf.part"), &model_bytes()[..1000]).unwrap();

        let result = run(&server, &job_in(dir.path(), false), pin);

        assert!(result.is_err(), "{result:?}");
        assert_eq!(server.requests(), 1, "no second try without progress");
        assert_eq!(part_len(dir.path()), 1000);
    }

    #[test]
    fn a_cancelled_download_keeps_its_part_and_continues() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |index| Reply::Serve {
            honour_range: true,
            cut_after: (index == 0).then_some(70_000),
        });
        let dir = tempfile::tempdir().unwrap();
        assert!(run_once(&server, &job_in(dir.path(), false), pin).is_err());
        let kept = part_len(dir.path());

        let stopped = run(&server, &job_in(dir.path(), true), pin);

        assert_eq!(stopped, Err(DownloadError::Cancelled));
        assert_eq!(part_len(dir.path()), kept, "the part stays as it was");
        assert!(!dir.path().join("model.gguf").exists());

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
        assert_eq!(server.range_of(1), Some(format!("bytes={kept}-")));
    }

    #[test]
    fn a_stop_from_the_card_ends_the_download_before_it_asks() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data, |_| Reply::Serve {
            honour_range: true,
            cut_after: None,
        });
        let dir = tempfile::tempdir().unwrap();
        let mut card = AiModelCard::new(Some(dir.path().to_path_buf()), false);
        let job = card.start().unwrap();
        card.stop();

        let result = run(&server, &job, pin);

        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(!dir.path().join("model.gguf").exists());
        assert!(!card.finish(result));
        assert!(card.is_paused());
    }

    #[test]
    fn a_server_that_ignores_range_starts_over() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |index| Reply::Serve {
            honour_range: false,
            cut_after: (index == 0).then_some(70_000),
        });
        let dir = tempfile::tempdir().unwrap();
        assert!(run_once(&server, &job_in(dir.path(), false), pin).is_err());
        let kept = part_len(dir.path());
        assert!(kept > 0);

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));

        assert_eq!(server.range_of(1), Some(format!("bytes={kept}-")));
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
    }

    #[test]
    fn a_redirect_to_a_host_that_is_not_allowed_is_refused() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data, |_| {
            Reply::Redirect("http://example.invalid/model.gguf".to_owned())
        });
        let dir = tempfile::tempdir().unwrap();

        let result = run(&server, &job_in(dir.path(), false), pin);

        assert!(
            matches!(
                result,
                Err(DownloadError::Network(FetchError::DownloadRefused { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(server.requests(), 1, "the other host was never asked");
        assert!(!dir.path().join("model.gguf").exists());
    }

    #[test]
    fn a_redirect_inside_the_allowed_host_is_followed_with_the_range() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |index| match index {
            0 => Reply::Serve {
                honour_range: true,
                cut_after: Some(70_000),
            },
            1 => Reply::Redirect("/elsewhere/model.gguf".to_owned()),
            _ => Reply::Serve {
                honour_range: true,
                cut_after: None,
            },
        });
        let dir = tempfile::tempdir().unwrap();
        assert!(run_once(&server, &job_in(dir.path(), false), pin).is_err());
        let kept = part_len(dir.path());

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));

        assert_eq!(server.requests(), 3);
        assert_eq!(server.range_of(1), Some(format!("bytes={kept}-")));
        assert_eq!(server.range_of(2), Some(format!("bytes={kept}-")));
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
    }

    #[test]
    fn a_part_that_is_already_complete_is_checked_and_put_in_place_without_a_request() {
        let data = model_bytes();
        let pin = pin_of(&data);
        let server = Server::start(data.clone(), |_| Reply::Serve {
            honour_range: true,
            cut_after: None,
        });
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("model.gguf.part"), &data).unwrap();

        assert_eq!(run(&server, &job_in(dir.path(), false), pin), Ok(()));

        assert_eq!(server.requests(), 0);
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), data);
    }

    #[cfg(feature = "local-ai")]
    fn lazy_model_stays_not_downloaded(dir: &Path) {
        use ncrs_ai::{ExtractionRequest, LazyLocalModel, LocalModelStatus, Provider as _};
        use std::num::NonZeroU32;

        let lazy = LazyLocalModel::new(
            dir.join(LOCAL_MODEL.name),
            LOCAL_MODEL,
            NonZeroU32::MIN,
            NonZeroU32::new(4096).unwrap(),
        );
        let request = ExtractionRequest {
            text: "Rechnung".to_owned(),
            open_fields: vec![ncrs_index::Field::Number],
            known: ncrs_index::DocumentFields::default(),
            source: PathBuf::from("inbox-one"),
            kinds: Arc::default(),
        };

        let error = lazy.extract(&request).unwrap_err();

        assert_eq!(lazy.status(), LocalModelStatus::NotDownloaded);
        assert!(error.to_string().contains("not downloaded"), "{error}");
    }

    #[test]
    fn nothing_is_downloaded_without_consent() {
        let data = model_bytes();
        let server = Server::start(data, |_| Reply::Serve {
            honour_range: true,
            cut_after: None,
        });
        let dir = tempfile::tempdir().unwrap();

        // A card that nobody pressed the button of has no job to run.
        let card = AiModelCard::new(Some(dir.path().to_path_buf()), false);
        assert!(!card.is_downloading());

        #[cfg(feature = "local-ai")]
        lazy_model_stays_not_downloaded(dir.path());
        assert_eq!(server.requests(), 0);
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
