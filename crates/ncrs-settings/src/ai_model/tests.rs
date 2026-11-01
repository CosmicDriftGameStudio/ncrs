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
