//! The protocol run of the Hermes chat: the real program, driven through eight
//! flows in German and English against a copy of a real archive, with an image
//! and a duration for every step.
//!
//! Run it as a release build, the model and the PDF reader are too slow in debug:
//!
//! ```text
//! NCRS_PROTOCOL_ARCHIVE=<folder, only read> NCRS_AI_MODELS_DIR=<models>
//! NCRS_OCR_MODELS_DIR=<models> NCRS_PDFIUM_DIR=<pdfium>
//! cargo test --release -p ncrs hermes_chat_protocol -- --ignored --nocapture
//! ```
//!
//! `NCRS_PROTOCOL_LANGS` (default `de,en`) picks the languages.
//! `NCRS_PROTOCOL_REUSE_INDEX=1` starts from the index (and the copy of the
//! archive) an earlier run kept in `target/protocol/cache/<hash of the archive>/`;
//! `NCRS_PROTOCOL_AI_WAIT_SECS` (default 3600) is how long the run waits for the
//! AI to read the documents before the first flow that needs them.
//! Everything the
//! run writes goes to `target/protocol/<stamp>/`: `<lang>/NN-<step>.png` and
//! `<lang>/protocol.md`. The archive is only read; the run works on a copy in
//! its own scratch folder with a plain index, an in-memory key store and a
//! trash of its own, so neither the user's configuration, index nor keychain is
//! touched.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::too_many_lines
)]

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::keyboard::{self, Key};
use iced::Task;
use iced_test::futures::futures::StreamExt as _;
use ncrs_hermes_chat::{ChipReply, Entry, Message as ChatMessage, Step};
use ncrs_i18n::Language;

use crate::app::App;
use crate::config::{AiSetting, Config, Filing, FilingLocation};
use crate::e2e_tests::{Outcome, Session};
use crate::filing::FilingMessage;
use crate::messages::{Message, SettingsMessage};

mod free_sentences;
pub mod real_start;
mod scratch;
pub mod thread;
mod tidy_flows;
mod waiting;

use scratch::{
    copy_tree, count_files, count_indexable, link_tree, path_hash, pdfs_below, report_removal,
};
use thread::{
    areas_in_plain_words, busy, chat, chip_labels, has_chip, is_ready, last_hermes_has_chips,
    last_text, last_working_has_digit,
};

const WINDOW: iced::Size = iced::Size::new(1200.0, 760.0);
const TICK_EVERY: Duration = Duration::from_millis(250);
const READ_LIMIT: Duration = Duration::from_secs(600);

static PICKED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn picker(_start_in: &Path) -> Task<Option<PathBuf>> {
    Task::done(PICKED.lock().unwrap().take())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[test]
#[ignore = "runs the Hermes chat protocol against a copied archive; see the module doc"]
fn hermes_chat_protocol() {
    let Some(archive) = env_path("NCRS_PROTOCOL_ARCHIVE") else {
        println!("hermes_chat_protocol: NCRS_PROTOCOL_ARCHIVE is not set, nothing to run");
        return;
    };
    let languages: Vec<Language> = std::env::var("NCRS_PROTOCOL_LANGS")
        .unwrap_or_else(|_| "de,en".to_owned())
        .split(',')
        .filter_map(|code| match code.trim() {
            "de" => Some(Language::German),
            "en" => Some(Language::English),
            _ => None,
        })
        .collect();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("protocol")
        .join(stamp);
    std::fs::create_dir_all(&root).unwrap();
    println!("protocol folder: {}", root.display());

    let scratch = root.join("scratch");
    let reuse = std::env::var("NCRS_PROTOCOL_REUSE_INDEX").is_ok_and(|value| value == "1");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("protocol")
        .join("cache")
        .join(path_hash(&archive));
    let master = workspace.join("master");
    if !(reuse && master.is_dir()) {
        report_removal(&workspace, std::fs::remove_dir_all(&workspace));
        let started = Instant::now();
        copy_tree(&archive, &master).unwrap();
        println!(
            "copied the archive: {} files in {:.1}s",
            count_files(&master),
            started.elapsed().as_secs_f64()
        );
    }

    for language in languages {
        let code = if language == Language::German {
            "de"
        } else {
            "en"
        };
        let previous_run = workspace.join("run");
        report_removal(&previous_run, std::fs::remove_dir_all(&previous_run));
        let mut run = start_protocol(
            &archive,
            &workspace,
            &scratch.join(code),
            &root.join(code),
            language,
            reuse,
        );
        run.flows();
    }
    if std::env::var_os("NCRS_PROTOCOL_KEEP").is_none() {
        report_removal(&scratch, std::fs::remove_dir_all(&scratch));
    }
    println!("protocol folder: {}", root.display());
}

struct Row {
    step: String,
    action: String,
    seconds: f64,
    image: String,
    expectation: String,
    met: Result<(), String>,
}

struct Protocol<P: iced::Program<State = App, Message = Message> + 'static> {
    session: Session<P>,
    out: PathBuf,
    language: Language,
    rows: Vec<Row>,
    images: usize,
    last_tick: Instant,
    held_start: Rc<RefCell<Option<Task<Message>>>>,
    archive: PathBuf,
    original: PathBuf,
    downloads: PathBuf,
    nothing: PathBuf,
    toomany: PathBuf,
    expected_documents: usize,
    index_path: PathBuf,
    /// Where the index is kept for a later run.
    kept_index: PathBuf,
    /// The run started from the kept index.
    reused: bool,
}

type Check = Result<(), String>;
/// A screenshot name and the moment, while busy, at which to take it.
type ShotWhen<'name> = Option<(&'name str, fn(&App) -> bool)>;

fn expect(condition: bool, otherwise: impl Into<String>) -> Check {
    if condition {
        Ok(())
    } else {
        Err(otherwise.into())
    }
}

fn all(checks: impl IntoIterator<Item = Check>) -> Check {
    let failed: Vec<String> = checks.into_iter().filter_map(Result::err).collect();
    expect(failed.is_empty(), failed.join("; "))
}

#[allow(clippy::needless_pass_by_value)]
fn start_protocol(
    original: &Path,
    workspace: &Path,
    scratch: &Path,
    out: &Path,
    language: Language,
    reuse: bool,
) -> Protocol<impl iced::Program<State = App, Message = Message> + 'static> {
    std::fs::create_dir_all(out).unwrap();
    // The same paths in every language and run, so the index kept in the
    // workspace knows the documents again.
    let archive = workspace.join("run").join("archive");
    link_tree(&workspace.join("master"), &archive).unwrap();
    let downloads = workspace.join("run").join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let mut pdfs = Vec::new();
    pdfs_below(&archive, &mut pdfs, usize::MAX);
    let every = pdfs.len().div_euclid(40).max(1);
    // New documents of the kinds the archive holds: moved out of the copy
    // before it is indexed, so they have neighbours but are no duplicates.
    for pdf in pdfs.iter().step_by(every).take(40) {
        std::fs::rename(pdf, downloads.join(pdf.file_name().unwrap())).unwrap();
    }
    let toomany = scratch.join("toomany");
    std::fs::create_dir_all(&toomany).unwrap();
    for number in 0..20_001 {
        std::fs::write(toomany.join(format!("file-{number}.txt")), "").unwrap();
    }
    let data = scratch.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let index_path = data.join("index.db");
    let kept_index = workspace.join("index.db");
    let reused = reuse && kept_index.is_file();
    if reused {
        std::fs::copy(&kept_index, &index_path).unwrap();
    } else {
        drop(ncrs_index::Index::create_plain(&index_path).unwrap());
    }
    if let Some(models) = env_path("NCRS_AI_MODELS_DIR") {
        std::os::unix::fs::symlink(models, data.join("ai-models")).unwrap();
    }
    if let Some(models) = env_path("NCRS_OCR_MODELS_DIR") {
        std::os::unix::fs::symlink(models, data.join("ocr-models")).unwrap();
    }
    let trash_folder = scratch.join("trash");
    std::fs::create_dir_all(&trash_folder).unwrap();
    let trash: Arc<dyn crate::fs::Trash> = Arc::new(crate::fs::JournaledTrash::new(
        scratch.join("journal.toml"),
        crate::fs::FolderBackend(trash_folder),
    ));
    let expected_documents = count_indexable(&archive) + count_indexable(&downloads);

    let filing = Filing {
        enabled: true,
        locations: vec![
            FilingLocation {
                path: archive.clone(),
                mode: ncrs_intake::LocationMode::Archive,
                images: false,
                is_target: true,
                transfer: Default::default(),
            },
            FilingLocation {
                path: downloads.clone(),
                mode: ncrs_intake::LocationMode::Inbox,
                images: false,
                is_target: false,
                transfer: Default::default(),
            },
        ],
        ai: AiSetting::Local { threads: None },
        ..Filing::default()
    };
    let config = Config {
        lang: Some(language),
        filing: filing.clone(),
        ..Config::default()
    };
    let held_start: Rc<RefCell<Option<Task<Message>>>> = Rc::new(RefCell::new(None));
    let (left, right) = (archive.clone(), downloads.clone());
    let mut session = Session::boot(crate::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, panel_loads) = App::starting_in(left.clone(), right.clone());
            let configured = app
                .with_config(Ok(config.clone()))
                .with_trash(Arc::clone(&trash))
                .with_folder_picker(picker);
            (configured, panel_loads)
        },
    ));
    // The start work runs at once on the blocking pool; its report is the
    // task held back here, so the chat can be opened while the module starts.
    let Session { app, runtime, .. } = &mut session;
    let start = runtime.enter(|| {
        app.start_filing_for_test(
            &filing,
            Some(index_path.clone()),
            Arc::new(ncrs_index::InMemoryKeyStore::empty()),
        )
    });
    *held_start.borrow_mut() = Some(start);
    Protocol {
        session,
        out: out.to_path_buf(),
        language,
        rows: Vec::new(),
        images: 0,
        last_tick: Instant::now(),
        held_start,
        archive,
        original: original.to_path_buf(),
        downloads,
        nothing: scratch.join("nothing"),
        toomany,
        expected_documents,
        index_path,
        kept_index,
        reused,
    }
}

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    fn app(&self) -> &App {
        &self.session.app
    }

    fn german(&self) -> bool {
        self.language == Language::German
    }

    fn say(&self, german: &'static str, english: &'static str) -> &'static str {
        if self.german() {
            german
        } else {
            english
        }
    }

    fn shot(&mut self, name: &str) -> String {
        self.images += 1;
        let file = format!("{:02}-{name}.png", self.images);
        crate::screenshot_tests::write_screenshot_to(
            &self.session.app,
            &self.out.to_string_lossy(),
            WINDOW,
            &file,
        );
        file
    }

    fn record(
        &mut self,
        step: &str,
        action: &str,
        seconds: f64,
        image: &str,
        expectation: &str,
        met: Check,
    ) {
        println!(
            "[{}] {step}: {action} -> {seconds:.1}s {image} {}",
            self.language.code(),
            match &met {
                Ok(()) => "yes".to_owned(),
                Err(reason) => format!("NO: {reason}"),
            }
        );
        self.rows.push(Row {
            step: step.to_owned(),
            action: action.to_owned(),
            seconds,
            image: image.to_owned(),
            expectation: expectation.to_owned(),
            met,
        });
        self.write_protocol();
    }

    fn write_protocol(&self) {
        let mut text = format!(
            "# Hermes chat protocol ({})\n\n| Step | Action | Seconds | Image | Expectation | Met |\n|---|---|---|---|---|---|\n",
            self.language.code()
        );
        for row in &self.rows {
            let met = match &row.met {
                Ok(()) => "yes".to_owned(),
                Err(reason) => format!("no: {reason}"),
            };
            let cell = |value: &str| value.replace('|', "/").replace('\n', " ");
            writeln!(
                text,
                "| {} | {} | {:.1} | {} | {} | {} |",
                cell(&row.step),
                cell(&row.action),
                row.seconds,
                cell(&row.image),
                cell(&row.expectation),
                cell(&met)
            )
            .unwrap();
        }
        std::fs::write(self.out.join("protocol.md"), text).unwrap();
    }

    /// Delivers one message and starts what it asks for, without waiting.
    fn deliver(&mut self, message: Message) {
        let task = self.session.send_held(message);
        self.session.drive(task);
    }

    fn tick(&mut self) {
        self.last_tick = Instant::now();
        self.deliver(Message::HermesChat(ChatMessage::WorkTick));
    }

    /// Takes the next outcome of a task or subscription, waiting at most `wait`.
    fn pump(&mut self, wait: Duration) {
        let actions = &mut self.session.actions;
        let got = self
            .session
            .runtime
            .block_on(async { tokio::time::timeout(wait, actions.next()).await });
        match got {
            Ok(Some(Outcome::Message(message))) => self.deliver(message),
            Ok(Some(Outcome::Finished)) => self.session.in_flight -= 1,
            Ok(Some(_) | None) | Err(_) => {}
        }
        if self.last_tick.elapsed() >= TICK_EVERY {
            self.tick();
        }
    }

    /// Runs until the chat has nothing left to wait for. The image is taken
    /// once while it is still busy and `when` holds. Returns the seconds and the
    /// image, or an error when `limit` ran out.
    fn wait_idle(&mut self, limit: Duration, shot: ShotWhen<'_>) -> (f64, String, Check) {
        let started = Instant::now();
        let mut image = String::new();
        loop {
            self.pump(Duration::from_millis(100));
            if let Some((name, when)) = shot {
                if image.is_empty() && busy(self.app()) && when(self.app()) {
                    image = self.shot(name);
                }
            }
            if self.session.in_flight == 0 && !busy(self.app()) {
                self.session.settle();
                return (started.elapsed().as_secs_f64(), image, Ok(()));
            }
            if started.elapsed() > limit {
                return (
                    started.elapsed().as_secs_f64(),
                    image,
                    Err(format!("still busy after {} s", limit.as_secs())),
                );
            }
        }
    }

    fn type_sentence(&mut self, sentence: &str) {
        self.session
            .send(Message::HermesChat(ChatMessage::InputChanged(
                sentence.to_owned(),
            )));
        self.deliver(Message::HermesChat(ChatMessage::Submit));
    }

    fn press_chip(&mut self, wanted: impl Fn(&ChipReply) -> bool) -> Check {
        let Some(chip) = chat(self.app())
            .chips()
            .iter()
            .find(|chip| wanted(&chip.reply))
            .cloned()
        else {
            return Err(format!(
                "no such button; there are: {}",
                chip_labels(self.app())
            ));
        };
        self.deliver(Message::HermesChat(ChatMessage::ChipPressed {
            label: chip.label,
            reply: chip.reply,
        }));
        Ok(())
    }

    /// Types `sentence`, answers a confirmation of an unknown folder with
    /// "read anyway", and waits for the end of the work.
    fn say_and_wait(&mut self, sentence: &str, shot: ShotWhen<'_>) -> (f64, String, Check) {
        self.type_sentence(sentence);
        let (mut seconds, mut image, mut result) = self.wait_idle(READ_LIMIT, shot);
        if has_chip(self.app(), |reply| *reply == ChipReply::ReadAnyway) {
            let _ = self.press_chip(|reply| *reply == ChipReply::ReadAnyway);
            let (more, later, again) = self.wait_idle(READ_LIMIT, shot);
            seconds += more;
            if image.is_empty() {
                image = later;
            }
            result = again;
        }
        (seconds, image, result)
    }

    /// Answers the open question with the button that leaves things as they
    /// are, until the chat asks nothing any more.
    fn back_to_start(&mut self) {
        for _ in 0..6 {
            if matches!(chat(self.app()).step, Step::Greeting | Step::Idle) {
                return;
            }
            let pressed = self
                .press_chip(|reply| *reply == ChipReply::Cancel)
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::KeepYes))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::FineAsIs))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::Later))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::LeaveThem))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::SinglesLeave))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::NamesLeave))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::PhotosNo));
            if pressed.is_err() {
                return;
            }
            let _ = self.wait_idle(Duration::from_secs(60), None);
        }
    }

    /// The chat is ready to take a typed folder: the question "which folder
    /// to learn" is asked when it asks nothing else.
    fn ask_for_folder(&mut self) {
        self.back_to_start();
        if chat(self.app()).step == Step::Idle
            && self
                .press_chip(|reply| *reply == ChipReply::IntentStructure)
                .is_ok()
        {
            let _ = self.wait_idle(Duration::from_secs(60), None);
        }
    }

    fn flows(&mut self) {
        self.flow_1_greeting_while_starting();
        self.wait_for_indexing();
        self.flow_2_reading();
        self.flow_3_leave_and_return();
        self.flow_4_errors();
        self.flow_5_tidy();
        self.flow_6_structure();
        self.flow_7_unknown();
        self.flow_8_free_sentences();
    }

    fn flow_1_greeting_while_starting(&mut self) {
        let started = Instant::now();
        self.session.press(Key::Named(keyboard::key::Named::F2));
        let image = self.shot("greeting-while-starting");
        let greeted = all([
            expect(chat(self.app()).open, "the chat did not open"),
            expect(
                last_hermes_has_chips(self.app()),
                "no greeting with buttons",
            ),
            expect(
                has_chip(self.app(), |reply| *reply == ChipReply::PickFolder),
                "no button for another folder",
            ),
        ]);
        self.record(
            "1 greeting",
            "F2 before Hermes has started",
            started.elapsed().as_secs_f64(),
            &image,
            "greeting with buttons while Hermes starts",
            greeted,
        );

        let waiting_started = Instant::now();
        *PICKED.lock().unwrap() = Some(self.archive.clone());
        let pressed = self.press_chip(|reply| *reply == ChipReply::PickFolder);
        self.session.settle();
        let waiting_image = self.shot("waits-for-hermes");
        let waiting = all([
            pressed,
            expect(
                matches!(chat(self.app()).thread().last(), Some(Entry::Working(_))),
                "no waiting line after the chosen folder",
            ),
            expect(
                chat(self.app()).session.holds_something(),
                "the folder was not kept for later",
            ),
        ]);
        self.record(
            "1 wait",
            "pressed 'another folder', the picker answers the archive",
            waiting_started.elapsed().as_secs_f64(),
            &waiting_image,
            "a visible 'one moment' line, the folder waits",
            waiting,
        );

        let start_began = Instant::now();
        let start = self.held_start.borrow_mut().take();
        if let Some(task) = start {
            self.session.drive(task);
        }
        let mut start_image = String::new();
        let mut confirmed = false;
        let limit = Instant::now() + Duration::from_secs(300);
        while !(is_ready(self.app()) && !busy(self.app())) && Instant::now() < limit {
            self.pump(Duration::from_millis(100));
            if !confirmed
                && self.app().filing_module_for_test().is_some_and(|module| {
                    matches!(module.phase(), crate::filing::Phase::PlainFileWarning)
                })
            {
                confirmed = true;
                self.deliver(Message::Filing(FilingMessage::ConfirmPlainFile));
            }
            if start_image.is_empty() && busy(self.app()) && last_working_has_digit(self.app()) {
                start_image = self.shot("reading-after-start");
            }
        }
        self.session.settle();
        let after = self.shot("read-after-start");
        let result = all([
            expect(is_ready(self.app()), "Hermes did not become ready"),
            expect(
                last_hermes_has_chips(self.app()),
                format!(
                    "the reading ended without a card; last: {}",
                    last_text(self.app())
                ),
            ),
        ]);
        self.record(
            "1 start",
            "the held start runs; the waiting folder is read by itself",
            start_began.elapsed().as_secs_f64(),
            &format!("{start_image} {after}"),
            "Hermes ready, the reading starts without a new click",
            result,
        );
    }

    fn flow_2_reading(&mut self) {
        let archive = self.archive.to_string_lossy().into_owned();
        self.ask_for_folder();
        chat_mut(&mut self.session.app).session.readings.clear();
        let (seconds, image, limit) =
            self.say_and_wait(&archive, Some(("reading-warm", last_working_has_digit)));
        let result = all([
            limit,
            expect(
                last_hermes_has_chips(self.app()),
                format!(
                    "no result card with buttons; last: {}",
                    last_text(self.app())
                ),
            ),
            expect(
                areas_in_plain_words(self.app()),
                "the areas card still writes the order with slashes or bars",
            ),
        ]);
        self.record(
            "2 warm",
            "typed the path of the indexed copy",
            seconds,
            &image,
            "a counter while reading, then areas and buttons",
            result,
        );
        let after = self.shot("result-warm");
        self.rows
            .last_mut()
            .unwrap()
            .image
            .push_str(&format!(" {after}"));

        self.ask_for_folder();
        let original = self.original.to_string_lossy().into_owned();
        let (cold_seconds, cold_image, cold_limit) =
            self.say_and_wait(&original, Some(("reading-cold", last_working_has_digit)));
        let cold_result = all([
            cold_limit,
            expect(
                last_hermes_has_chips(self.app()),
                format!(
                    "no result card with buttons; last: {}",
                    last_text(self.app())
                ),
            ),
            expect(
                areas_in_plain_words(self.app()),
                "the areas card still writes the order with slashes or bars",
            ),
        ]);
        self.record(
            "2 cold",
            "typed the path of the original (not indexed)",
            cold_seconds,
            &cold_image,
            "a counter while reading, then areas and buttons",
            cold_result,
        );
        let after_cold = self.shot("result-cold");
        self.rows
            .last_mut()
            .unwrap()
            .image
            .push_str(&format!(" {after_cold}"));
    }

    fn flow_3_leave_and_return(&mut self) {
        self.ask_for_folder();
        chat_mut(&mut self.session.app).session.readings.clear();
        let original = self.original.to_string_lossy().into_owned();
        let started = Instant::now();
        self.type_sentence(&original);
        let (_, _, _) = self.wait_idle(Duration::from_secs(30), None);
        if has_chip(self.app(), |reply| *reply == ChipReply::ReadAnyway) {
            let _ = self.press_chip(|reply| *reply == ChipReply::ReadAnyway);
        }
        let working = busy(self.app());
        self.deliver(Message::HermesChat(ChatMessage::Close));
        self.deliver(Message::Settings(SettingsMessage::Toggle));
        self.pump(Duration::from_millis(300));
        let image_settings = self.shot("settings-while-reading");
        self.deliver(Message::Settings(SettingsMessage::Toggle));
        self.pump(Duration::from_millis(100));
        self.deliver(Message::HermesChat(ChatMessage::Open));
        let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
        let image_back = self.shot("chat-reopened");
        let back = all([
            limit,
            expect(
                working,
                "the reading had already ended before the chat was closed",
            ),
            expect(chat(self.app()).open, "the chat did not open again"),
            expect(!busy(self.app()), "the chat is still busy"),
            expect(
                last_hermes_has_chips(self.app()),
                "the result has no buttons",
            ),
        ]);
        let seconds = started.elapsed().as_secs_f64();
        self.record(
            "3 leave",
            "closed the chat while reading, looked at the settings, opened the chat",
            seconds,
            &format!("{image_settings} {image_back}"),
            "the result is there after returning",
            back,
        );

        let thanks = self.say("Danke", "Thanks");
        let thanks_started = Instant::now();
        self.back_to_start();
        self.type_sentence(thanks);
        let (_, _, thanks_limit) = self.wait_idle(Duration::from_secs(120), None);
        let image = self.shot("thanks");
        let answered = all([
            thanks_limit,
            expect(last_hermes_has_chips(self.app()), "no answer with buttons"),
        ]);
        self.record(
            "3 typing",
            "typed a thank you after returning",
            thanks_started.elapsed().as_secs_f64(),
            &image,
            "an answer with buttons",
            answered,
        );
    }

    fn flow_4_errors(&mut self) {
        for (label, path, text) in [
            (
                "4 gone",
                self.nothing.clone(),
                "a folder that does not exist",
            ),
            (
                "4 too many",
                self.toomany.clone(),
                "a folder with 20001 files",
            ),
        ] {
            self.ask_for_folder();
            let (seconds, _, limit) = self.say_and_wait(&path.to_string_lossy(), None);
            let image = self.shot(&label.replace(' ', "-"));
            let met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app()),
                    format!("no bubble with buttons; last: {}", last_text(self.app())),
                ),
            ]);
            self.record(
                label,
                &format!("typed {text}"),
                seconds,
                &image,
                "an explanation with buttons",
                met,
            );
        }
        self.record(
            "4 model error",
            "not forceable in this run",
            0.0,
            "",
            "an explanation with buttons",
            Err("not proven: a model error cannot be forced".to_owned()),
        );
    }

    fn files_below(&self, dir: &Path) -> usize {
        count_files(dir)
    }
}

fn chat_mut(app: &mut App) -> &mut ncrs_hermes_chat::State {
    app.hermes_chat_mut_for_test()
}
