//! The bridge that lets a running `git` ask the person at the window for a
//! login. git (and ssh) start the ncrs binary itself as the askpass helper; it
//! hands the prompt to the broker in this process over a loopback socket and
//! prints the answer for git. The answer travels only through that socket: it
//! is never put in an environment variable, a log or a debug print.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Set for the helper: `deny`, or the `host:port` of the broker.
pub const ENDPOINT_VARIABLE: &str = "NCRS_ASKPASS";
/// `<secret>:<job>`; the secret keeps other local processes off the socket. Every
/// job has its own: what one job's git starts cannot ask in the name of another.
pub const TOKEN_VARIABLE: &str = "NCRS_ASKPASS_TOKEN";

const DENY: &str = "deny";
const PROTOCOL: &str = "ncrs-askpass 1";
const HINT_CONFIRM: &str = "confirm";
const HINT_TEXT: &str = "text";
/// A question nobody answers in this time is refused, so a job cannot hang forever.
const ANSWER_LIMIT: Duration = Duration::from_secs(600);
const REPLY_LIMIT: Duration = Duration::from_secs(660);
/// The longest line a helper may send.
const LINE_LIMIT: u64 = 8 * 1024;

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// A job number no other owner of interactive jobs in this process has: the
/// questions of a job come back with it, so the app finds who asked.
pub fn next_job() -> u64 {
    NEXT_JOB.fetch_add(1, Ordering::Relaxed)
}

/// What git wants to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// A user name.
    Text,
    /// A password, passphrase or PIN: shown masked.
    Secret,
    /// A yes or no, like the question about an unknown host key.
    Confirm,
}

/// An answer that never prints: `Debug` shows `***`, there is no `Display`,
/// and the text is overwritten when the value is dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.fill(0);
    }
}

/// One question from a running job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskRequest {
    pub id: u64,
    pub job: u64,
    /// The text git or ssh printed, for example `Username for 'https://host': `.
    pub prompt: String,
    pub kind: PromptKind,
}

/// Sorts a prompt into what the dialog has to offer. `confirm_hint` is
/// `SSH_ASKPASS_PROMPT=confirm`, which newer ssh sets for yes/no questions.
/// Whatever does not clearly ask for a name is shown masked.
pub fn kind_of_prompt(prompt: &str, confirm_hint: bool) -> PromptKind {
    let lower = prompt.to_lowercase();
    if confirm_hint || lower.contains("(yes/no") || lower.contains("yes/no/[fingerprint]") {
        PromptKind::Confirm
    } else if ["username", "user name"]
        .iter()
        .any(|word| lower.trim_start().starts_with(word))
    {
        PromptKind::Text
    } else {
        PromptKind::Secret
    }
}

/// The secret of each job that may ask.
type Secrets = Arc<Mutex<HashMap<u64, String>>>;
type Pending = Arc<Mutex<HashMap<u64, (u64, Sender<Option<Secret>>)>>>;

/// Listens for helpers, turns their prompts into `AskRequest`s and hands the
/// answers back.
#[derive(Debug)]
pub struct Broker {
    address: SocketAddr,
    secrets: Secrets,
    pending: Pending,
}

impl Broker {
    /// Starts listening on a free loopback port. The requests arrive on the
    /// returned receiver; without anyone receiving, every prompt is refused.
    pub fn start() -> std::io::Result<(Self, Receiver<AskRequest>)> {
        let (sender, receiver) = mpsc::channel();
        let broker = Self::listen(sender, Arc::new(AtomicBool::new(true)))?;
        Ok((broker, receiver))
    }

    fn listen(requests: Sender<AskRequest>, listening: Arc<AtomicBool>) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let broker = Self {
            address: listener.local_addr()?,
            secrets: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        let shared = Shared {
            secrets: Arc::clone(&broker.secrets),
            pending: Arc::clone(&broker.pending),
            requests,
            listening,
            next_id: Arc::new(AtomicU64::new(1)),
        };
        std::thread::spawn(move || {
            for connection in listener.incoming().flatten() {
                let for_connection = shared.clone();
                std::thread::spawn(move || {
                    let _ = serve(connection, &for_connection);
                });
            }
        });
        Ok(broker)
    }

    pub fn endpoint(&self) -> String {
        self.address.to_string()
    }

    /// What a helper of `job` has to show. Empty, and so never accepted, when
    /// the system gives no random bytes.
    pub fn token(&self, job: u64) -> String {
        let mut secrets = lock(&self.secrets);
        let known = secrets.get(&job).cloned();
        let Some(secret) = known.or_else(|| random_secret().ok()) else {
            return String::new();
        };
        let token = format!("{secret}:{job}");
        secrets.insert(job, secret);
        token
    }

    /// Answers a question; `None` cancels it (git then fails as unauthenticated).
    pub fn answer(&self, id: u64, answer: Option<Secret>) {
        let waiting = lock(&self.pending).remove(&id);
        if let Some((_, sender)) = waiting {
            let _ = sender.send(answer);
        }
    }

    /// Cancels every open question of one job, for a closed tab, and takes no
    /// new one with the token it had.
    pub fn cancel_job(&self, job: u64) {
        lock(&self.secrets).remove(&job);
        let ids: Vec<u64> = lock(&self.pending)
            .iter()
            .filter(|(_, (owner, _))| *owner == job)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.answer(id, None);
        }
    }

    pub fn cancel_all(&self) {
        lock(&self.secrets).clear();
        let ids: Vec<u64> = lock(&self.pending).keys().copied().collect();
        for id in ids {
            self.answer(id, None);
        }
    }
}

#[derive(Clone)]
struct Shared {
    secrets: Secrets,
    pending: Pending,
    requests: Sender<AskRequest>,
    listening: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn random_secret() -> std::io::Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Compares without stopping at the first difference, so the time a refused
/// token takes says nothing about how much of it was right.
fn same_secret(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (left, right)| diff | (left ^ right))
            == 0
}

/// A whole line; one that is cut off or longer than the limit is an error.
fn read_field(reader: &mut BufReader<TcpStream>) -> std::io::Result<String> {
    let mut line = String::new();
    reader.by_ref().take(LINE_LIMIT).read_line(&mut line)?;
    match line.strip_suffix('\n') {
        Some(field) => Ok(field.trim_end_matches('\r').to_string()),
        None => Err(std::io::ErrorKind::InvalidData.into()),
    }
}

fn job_of_token(token: &str, secrets: &Secrets) -> Option<u64> {
    let (secret, number) = token.split_once(':')?;
    let job = number.parse::<u64>().ok()?;
    lock(secrets)
        .get(&job)
        .is_some_and(|expected| same_secret(secret, expected))
        .then_some(job)
}

fn serve(mut connection: TcpStream, shared: &Shared) -> std::io::Result<()> {
    connection.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(connection.try_clone()?);
    let protocol = read_field(&mut reader)?;
    let token = read_field(&mut reader)?;
    let allowed = job_of_token(&token, &shared.secrets).filter(|_| protocol == PROTOCOL);
    let Some(job) = allowed else {
        return connection.write_all(b"no\n");
    };
    let hint = read_field(&mut reader)?;
    let prompt = read_field(&mut reader)?;
    if !shared.listening.load(Ordering::Relaxed) {
        return connection.write_all(b"no\n");
    }
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (reply, wait) = mpsc::channel();
    lock(&shared.pending).insert(id, (job, reply));
    let request = AskRequest {
        id,
        job,
        kind: kind_of_prompt(&prompt, hint == HINT_CONFIRM),
        prompt,
    };
    if shared.requests.send(request).is_err() {
        lock(&shared.pending).remove(&id);
        return connection.write_all(b"no\n");
    }
    match wait.recv_timeout(ANSWER_LIMIT) {
        Ok(Some(answer)) => {
            connection.write_all(b"ok\n")?;
            connection.write_all(answer.expose().as_bytes())?;
            connection.write_all(b"\n")
        }
        _ => {
            lock(&shared.pending).remove(&id);
            connection.write_all(b"no\n")
        }
    }
}

struct Global {
    broker: Broker,
    requests: Mutex<Option<Receiver<AskRequest>>>,
    listening: Arc<AtomicBool>,
}

fn global() -> Option<&'static Global> {
    static GLOBAL: OnceLock<Option<Global>> = OnceLock::new();
    GLOBAL
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            let listening = Arc::new(AtomicBool::new(false));
            let broker = Broker::listen(sender, Arc::clone(&listening)).ok()?;
            Some(Global {
                broker,
                requests: Mutex::new(Some(receiver)),
                listening,
            })
        })
        .as_ref()
}

/// The questions of every interactive job of this process. Handed out once;
/// until someone has taken it, prompts are refused.
pub fn take_requests() -> Option<Receiver<AskRequest>> {
    let shared = global()?;
    let receiver = lock(&shared.requests).take();
    if receiver.is_some() {
        shared.listening.store(true, Ordering::Relaxed);
    }
    receiver
}

/// Answers a question of the process-wide broker.
pub fn answer(id: u64, answer: Option<Secret>) {
    if let Some(shared) = global() {
        shared.broker.answer(id, answer);
    }
}

/// Cancels the open questions of one job (its tab was closed).
pub fn cancel_job(job: u64) {
    if let Some(shared) = global() {
        shared.broker.cancel_job(job);
    }
}

/// Cancels every open question, at the end of the app.
pub fn cancel_all() {
    if let Some(shared) = global() {
        shared.broker.cancel_all();
    }
}

/// What `git` needs in its environment to ask through the broker.
pub struct Setup {
    /// The value for both `GIT_ASKPASS` and `SSH_ASKPASS`.
    pub askpass: String,
    pub endpoint: String,
    pub token: String,
}

pub fn setup_for(job: u64) -> Option<Setup> {
    let shared = global()?;
    // After an upgrade the running binary can be gone from disk: no helper then.
    let helper = std::env::current_exe().ok().filter(|path| path.is_file())?;
    Some(Setup {
        askpass: askpass_value(&helper, cfg!(windows)),
        endpoint: shared.broker.endpoint(),
        token: shared.broker.token(job),
    })
}

/// git and ssh both start the helper directly, without a shell: quotes around
/// a path with a space would become part of the file name.
fn askpass_value(helper: &Path, windows: bool) -> String {
    let path = helper.to_string_lossy();
    if windows {
        path.replace('\\', "/")
    } else {
        path.into_owned()
    }
}

/// The helper side. Call it first thing in `main`: `Some(exit code)` means
/// this process was started by git as the askpass helper and has done its job.
pub fn run_if_asked(args: &[String]) -> Option<i32> {
    let endpoint = std::env::var(ENDPOINT_VARIABLE).ok()?;
    if endpoint == DENY {
        return Some(1);
    }
    let mode = std::env::var("SSH_ASKPASS_PROMPT").unwrap_or_default();
    if mode == "none" {
        return Some(0);
    }
    let prompt = args.get(1).map_or("", String::as_str);
    let token = std::env::var(TOKEN_VARIABLE).unwrap_or_default();
    let hint = if mode == HINT_CONFIRM {
        HINT_CONFIRM
    } else {
        HINT_TEXT
    };
    match exchange(&endpoint, &token, prompt, hint) {
        Ok(Some(answer)) => {
            let reply =
                if mode == HINT_CONFIRM || kind_of_prompt(prompt, false) == PromptKind::Confirm {
                    "yes".to_string()
                } else {
                    answer
                };
            let mut out = std::io::stdout();
            let written = out
                .write_all(reply.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            Some(i32::from(written.is_err()))
        }
        _ => Some(1),
    }
}

/// One question and its answer; `Ok(None)` when the person said no.
pub fn exchange(
    endpoint: &str,
    token: &str,
    prompt: &str,
    hint: &str,
) -> std::io::Result<Option<String>> {
    let mut connection = TcpStream::connect(endpoint)?;
    connection.set_read_timeout(Some(REPLY_LIMIT))?;
    let one_line = prompt.replace(['\r', '\n'], " ");
    // One write: the broker may refuse after the token and close the connection.
    let question = format!("{PROTOCOL}\n{token}\n{hint}\n{one_line}\n");
    connection.write_all(question.as_bytes())?;
    let mut reader = BufReader::new(connection);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    if status.trim_end() != "ok" {
        return Ok(None);
    }
    let mut answer = String::new();
    reader.read_line(&mut answer)?;
    Ok(Some(answer.trim_end_matches(['\r', '\n']).to_string()))
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setup")
            .field("endpoint", &self.endpoint)
            .field("token", &"***")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_whole_secret_opens_the_socket() {
        let secret = random_secret().unwrap();
        assert_eq!(secret.len(), 64);
        assert_ne!(secret, random_secret().unwrap());
        assert!(same_secret(&secret, &secret));
        assert!(!same_secret(&secret[..63], &secret));
        assert!(!same_secret(
            &format!("{}0", &secret[..63]),
            &format!("{}1", &secret[..63])
        ));
    }

    #[test]
    fn a_secret_never_prints() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn prompts_are_sorted_by_what_they_ask_for() {
        let kind = |prompt| kind_of_prompt(prompt, false);
        assert_eq!(
            kind("Username for 'https://github.com': "),
            PromptKind::Text
        );
        assert_eq!(
            kind("Password for 'https://me@github.com': "),
            PromptKind::Secret
        );
        assert_eq!(
            kind("Enter passphrase for key '/home/me/.ssh/id_ed25519': "),
            PromptKind::Secret
        );
        assert_eq!(kind("me@host's password: "), PromptKind::Secret);
        assert_eq!(
            kind("Are you sure you want to continue connecting (yes/no/[fingerprint])? "),
            PromptKind::Confirm
        );
        assert_eq!(kind_of_prompt("Touch your key", true), PromptKind::Confirm);
    }

    #[test]
    fn the_helper_path_carries_no_quotes() {
        let path = Path::new(r"C:\Program Files\ncrs\ncrs.exe");
        assert_eq!(askpass_value(path, true), "C:/Program Files/ncrs/ncrs.exe");
        assert_eq!(
            askpass_value(Path::new("/opt/my ncrs/ncrs"), false),
            "/opt/my ncrs/ncrs"
        );
    }

    #[test]
    fn an_unknown_prompt_is_masked() {
        let kind = |prompt| kind_of_prompt(prompt, false);
        assert_eq!(
            kind("Enter the code from your device: "),
            PromptKind::Secret
        );
        assert_eq!(kind(""), PromptKind::Secret);
        assert_eq!(
            kind("Password for 'https://username@host': "),
            PromptKind::Secret
        );
    }

    #[test]
    fn the_token_of_one_job_does_not_ask_for_another() {
        let (broker, requests) = Broker::start().unwrap();
        let first = broker.token(1);
        assert_eq!(broker.token(1), first, "every git call of a job shares it");
        let second = broker.token(2);
        let (secret, _) = first.split_once(':').unwrap();
        assert!(!second.starts_with(secret));
        let answer = exchange(
            &broker.endpoint(),
            &format!("{secret}:2"),
            "Username: ",
            HINT_TEXT,
        )
        .unwrap();
        assert_eq!(answer, None);

        broker.cancel_job(1);
        let late = exchange(&broker.endpoint(), &first, "Username: ", HINT_TEXT).unwrap();
        assert_eq!(late, None);
        assert!(requests.try_recv().is_err(), "nobody was asked");
    }

    #[test]
    fn an_endless_line_is_refused() {
        let (broker, requests) = Broker::start().unwrap();
        let prompt = "x".repeat(20_000);
        let answer = exchange(&broker.endpoint(), &broker.token(1), &prompt, HINT_TEXT);
        assert!(
            !matches!(answer, Ok(Some(_))),
            "a dropped connection or a no"
        );
        assert!(requests.try_recv().is_err(), "nobody was asked");
    }

    fn ask_in_thread(
        broker: &Broker,
        job: u64,
        prompt: &'static str,
    ) -> std::thread::JoinHandle<Option<String>> {
        let endpoint = broker.endpoint();
        let token = broker.token(job);
        std::thread::spawn(move || exchange(&endpoint, &token, prompt, HINT_TEXT).unwrap())
    }

    #[test]
    fn a_question_travels_to_the_receiver_and_the_answer_back() {
        let (broker, requests) = Broker::start().unwrap();
        let asking = ask_in_thread(&broker, 7, "Username for 'https://host': ");
        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((request.job, request.kind), (7, PromptKind::Text));
        assert_eq!(request.prompt, "Username for 'https://host': ");
        broker.answer(request.id, Some(Secret::new("marc")));
        assert_eq!(asking.join().unwrap().as_deref(), Some("marc"));
    }

    #[test]
    fn no_answer_and_a_cancelled_job_both_say_no() {
        let (broker, requests) = Broker::start().unwrap();
        let first = ask_in_thread(&broker, 1, "Password for 'https://h': ");
        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        broker.answer(request.id, None);
        assert_eq!(first.join().unwrap(), None);

        let second = ask_in_thread(&broker, 2, "Password for 'https://h': ");
        requests.recv_timeout(Duration::from_secs(5)).unwrap();
        broker.cancel_job(2);
        assert_eq!(second.join().unwrap(), None);
    }

    #[test]
    fn a_wrong_token_is_refused_without_a_request() {
        let (broker, requests) = Broker::start().unwrap();
        let answer = exchange(&broker.endpoint(), "wrong:1", "Username: ", HINT_TEXT).unwrap();
        assert_eq!(answer, None);
        assert!(requests.try_recv().is_err(), "nobody was asked");
    }

    #[test]
    fn nobody_receiving_means_refusing() {
        let (broker, requests) = Broker::start().unwrap();
        drop(requests);
        let answer = exchange(
            &broker.endpoint(),
            &broker.token(1),
            "Username: ",
            HINT_TEXT,
        )
        .unwrap();
        assert_eq!(answer, None);
    }
}
