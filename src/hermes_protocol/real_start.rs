//! The real start path of the app, from a config file and a data folder of its
//! own, up to the chat. An in-memory key store stands in for the keychain, and
//! the walk never answers yes for a folder outside its data folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced_test::futures::futures::StreamExt as _;
use ncrs_hermes_chat::{ChipReply, Message as ChatMessage};
use ncrs_index::{InMemoryKeyStore, KeyStore, KeyStoreError};

use super::scratch::pdfs_below;
use super::thread::{busy, chat, chip_labels, entry_text, is_ready, whole_thread};
use super::{picker, TICK_EVERY, WINDOW};
use crate::app::{App, Environment};
use crate::e2e_tests::{Outcome, Session};
use crate::filing::{FilingMessage, Phase};
use crate::messages::Message;

type Store = Arc<dyn KeyStore + Send + Sync>;

const FILING_ONLY: &str = "version = 1\n[modules.filing]\nenabled = true\n";
const FILING_AI: &str = "version = 1\n[modules.filing]\nenabled = true\n[modules.filing.ai]\nenabled = true\nprovider = \"local\"\n";
const SWITCHED_OFF: &str = "version = 1\n[modules.filing]\nenabled = false\n";
const READ_STEP: Duration = Duration::from_secs(900);
const DOCUMENTS: usize = 24;

/// A keychain that says no, as one does when its dialog is refused.
struct RefusingStore;

impl KeyStore for RefusingStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }
}

fn empty_store() -> Store {
    Arc::new(InMemoryKeyStore::empty())
}

fn boot(
    data: &Path,
    store: Store,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let boot_data = data.to_path_buf();
    // iced boots inside its runtime; `Session::boot` does not.
    let boot_runtime = tokio::runtime::Runtime::new().unwrap();
    let trash_folder = data.join("trash");
    std::fs::create_dir_all(&trash_folder).unwrap();
    let trash: Arc<dyn crate::fs::Trash> = Arc::new(crate::fs::JournaledTrash::new(
        data.join("journal.toml"),
        crate::fs::FolderBackend(trash_folder),
    ));
    Session::boot(crate::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let _inside = boot_runtime.enter();
            let environment = Environment {
                config_path: Some(boot_data.join("config.toml")),
                index_path: Some(boot_data.join("index.db")),
                state_path: None,
                journal_path: None,
                view_copies_dir: None,
                key_store: Arc::clone(&store),
                passwords: Arc::new(crate::fs::remote::credentials::InMemoryPasswords::default()),
                ssh_known_hosts: None,
                own_known_hosts: None,
            };
            let (app, task) =
                App::restored(environment, crate::session_state::SessionState::default());
            (
                app.with_trash(Arc::clone(&trash))
                    .with_folder_picker(picker),
                task,
            )
        },
    ))
}

fn pump_for<P>(session: &mut Session<P>, span: Duration, until: fn(&App) -> bool)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    let mut last_tick = Instant::now();
    let end = Instant::now() + span;
    while Instant::now() < end && !until(&session.app) {
        let actions = &mut session.actions;
        let got = session.runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(100), actions.next()).await
        });
        match got {
            Ok(Some(Outcome::Message(message))) => {
                let task = session.send_held(message);
                session.drive(task);
            }
            Ok(Some(Outcome::Finished)) => session.in_flight -= 1,
            Ok(Some(_) | None) | Err(_) => {}
        }
        if last_tick.elapsed() >= TICK_EVERY {
            last_tick = Instant::now();
            let task = session.send_held(Message::HermesChat(ChatMessage::WorkTick));
            session.drive(task);
        }
    }
}

fn started(app: &App) -> bool {
    app.filing_module_for_test()
        .is_some_and(|module| !matches!(module.phase(), Phase::Starting))
}

fn phase_name(app: &App) -> String {
    app.filing_module_for_test().map_or_else(
        || "no module".to_owned(),
        |module| match module.phase() {
            Phase::Starting => "Starting".to_owned(),
            Phase::AskEncryption { busy, error, .. } => {
                format!("AskEncryption busy={busy} error={error:?}")
            }
            Phase::ShowRecoveryKey { .. } => "ShowRecoveryKey".to_owned(),
            Phase::AskRecoveryKey { .. } => "AskRecoveryKey".to_owned(),
            Phase::PlainFileWarning => "PlainFileWarning".to_owned(),
            Phase::Ready => "Ready".to_owned(),
            Phase::Encrypting => "Encrypting".to_owned(),
            Phase::StartingOver => "StartingOver".to_owned(),
            Phase::Failed(reason) => format!("Failed({reason})"),
            Phase::Dismissed => "Dismissed".to_owned(),
        },
    )
}

fn report(app: &App, step: &str) {
    println!("--- {step}: phase {}, busy {}", phase_name(app), busy(app));
    if let Some(entry) = chat(app).thread().last() {
        println!("    last: {}", entry_text(entry).replace('\n', " / "));
    }
    println!(
        "    entries {}, chips: {}",
        chat(app).thread().len(),
        chip_labels(app)
    );
}

/// Answers the key card the way a user does, when it is up.
fn type_key_back<P>(session: &mut Session<P>)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    let group = session.app.filing_module_for_test().and_then(|module| {
        if let Phase::ShowRecoveryKey { key, group, .. } = module.phase() {
            key.display_text().split('-').nth(*group).map(str::to_owned)
        } else {
            None
        }
    });
    if let Some(typed) = group {
        session.send(Message::Filing(FilingMessage::GroupInput(typed.into())));
        session.send(Message::Filing(FilingMessage::GroupSubmit));
        pump_for(session, Duration::from_secs(10), is_ready);
        report(&session.app, "key typed back");
    }
}

fn talk_ready(data: &Path, store: Store, label: &str) {
    assert!(
        talk(data, store, label),
        "{label}: Hermes never became ready"
    );
}

fn talk_not_ready(data: &Path, store: Store, label: &str) {
    assert!(
        !talk(data, store, label),
        "{label}: Hermes became ready without its key"
    );
}

/// F2, then a sentence; a card that can be left is left and the chat tried again.
fn talk(data: &Path, store: Store, label: &str) -> bool {
    println!("\n[{label}]");
    let shots = data.join("shots").join(label.replace(' ', "-"));
    std::fs::create_dir_all(&shots).unwrap();
    let out = shots.to_string_lossy().into_owned();
    let mut session = boot(data, store);
    pump_for(&mut session, Duration::from_secs(5), started);
    report(&session.app, "after boot");
    session.send(Message::HermesChat(ChatMessage::Open));
    pump_for(&mut session, Duration::from_secs(20), |app| {
        started(app)
            && !app.filing_module_for_test().is_some_and(|module| {
                matches!(module.phase(), Phase::AskEncryption { busy: true, .. })
            })
    });
    report(&session.app, "chat opened");
    shot(&session.app, &out, "1-open");
    type_key_back(&mut session);
    session.send(Message::HermesChat(ChatMessage::InputChanged(
        "Sortier mir meine Downloads".to_owned(),
    )));
    session.send(Message::HermesChat(ChatMessage::Submit));
    pump_for(&mut session, Duration::from_secs(3), |_| false);
    pump_for(&mut session, Duration::from_secs(240), |app| !busy(app));
    report(&session.app, "sentence answered");
    shot(&session.app, &out, "2-sentence");
    let leaves_card = session
        .app
        .filing_module_for_test()
        .is_some_and(crate::filing::FilingModule::card_can_be_dismissed);
    if leaves_card {
        session.send(Message::Filing(FilingMessage::Dismiss));
        pump_for(&mut session, Duration::from_secs(2), |_| false);
        report(&session.app, "card left");
        session.send(Message::HermesChat(ChatMessage::InputChanged(
            "Sortier mir meine Downloads".to_owned(),
        )));
        session.send(Message::HermesChat(ChatMessage::Submit));
        pump_for(&mut session, Duration::from_secs(3), |_| false);
        report(&session.app, "sentence after the card");
        shot(&session.app, &out, "3-card-left");
        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: "Start Hermes".to_owned(),
            reply: ChipReply::TurnOn,
        }));
        pump_for(&mut session, Duration::from_secs(5), |_| false);
        report(&session.app, "started again");
        shot(&session.app, &out, "4-started-again");
    } else if chat(&session.app)
        .chips()
        .iter()
        .any(|chip| chip.reply == ChipReply::TurnOn)
    {
        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: "Start Hermes".to_owned(),
            reply: ChipReply::TurnOn,
        }));
        pump_for(&mut session, Duration::from_secs(20), started);
        report(&session.app, "started from the chat");
        type_key_back(&mut session);
        pump_for(&mut session, Duration::from_secs(5), |_| false);
        report(&session.app, "answered after the start");
        shot(&session.app, &out, "3-started");
    }
    is_ready(&session.app)
}

fn shot(app: &App, out: &str, name: &str) {
    crate::screenshot_tests::write_screenshot_to(app, out, WINDOW, &format!("{name}.png"));
}

fn fresh(root: &Path, name: &str, config: Option<&str>, model: Option<&PathBuf>) -> PathBuf {
    println!("\n===== {name} =====");
    let data = root.join(name);
    drop(std::fs::remove_dir_all(&data));
    std::fs::create_dir_all(&data).unwrap();
    if let Some(text) = config {
        std::fs::write(data.join("config.toml"), text).unwrap();
    }
    if let Some(models) = model {
        std::os::unix::fs::symlink(models, data.join("ai-models")).unwrap();
    }
    data
}

fn locations(inbox: &Path, archive: &Path) -> String {
    format!(
        "{FILING_AI}[[modules.filing.locations]]\npath = {:?}\nmode = \"inbox\"\n\
         [[modules.filing.locations]]\npath = {:?}\nmode = \"archive\"\ntarget = true\n",
        inbox, archive
    )
}

/// An answer the walk may give: nothing that opens a picker or leaves the data
/// folder, which is the only place it may tidy up.
fn safe_to_press(reply: &ChipReply, data: &Path) -> bool {
    match reply {
        ChipReply::Folder(folder) | ChipReply::NamedFolder(folder) => folder.starts_with(data),
        ChipReply::PickFolder
        | ChipReply::NewTarget
        | ChipReply::TrialOtherTarget
        | ChipReply::GroupNewFolder
        | ChipReply::IntentSearch
        | ChipReply::Cancel
        | ChipReply::Later => false,
        _ => true,
    }
}

#[test]
#[ignore = "walks the real start with a config and a data folder of its own"]
fn hermes_real_start() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("protocol")
        .join("real-start");
    let models = super::env_path("NCRS_AI_MODELS_DIR");

    let no_config = fresh(&root, "no-config", None, None);
    talk_ready(&no_config, empty_store(), "first start");
    let switched_off = fresh(&root, "switched-off", Some(SWITCHED_OFF), None);
    talk_ready(&switched_off, empty_store(), "first start");
    let without_model = fresh(&root, "ai-without-model", Some(FILING_AI), None);
    let without_model_keys = empty_store();
    talk_ready(
        &without_model,
        Arc::clone(&without_model_keys),
        "first start",
    );
    talk_ready(&without_model, without_model_keys, "second start");
    let with_model = fresh(&root, "ai-with-model", Some(FILING_AI), models.as_ref());
    let with_model_keys = empty_store();
    talk_ready(&with_model, Arc::clone(&with_model_keys), "first start");
    talk_ready(&with_model, with_model_keys, "second start");

    // A lost or refused key leaves the index closed: Hermes is not ready.
    let key_missing = fresh(&root, "key-missing", Some(FILING_ONLY), None);
    talk_ready(&key_missing, empty_store(), "index made");
    talk_not_ready(&key_missing, empty_store(), "key gone");
    let refused_new = fresh(&root, "keychain-refused-new-index", Some(FILING_ONLY), None);
    talk_not_ready(&refused_new, Arc::new(RefusingStore), "refused");
    let refused_old = fresh(&root, "keychain-refused-old-index", Some(FILING_ONLY), None);
    talk_ready(&refused_old, empty_store(), "index made");
    talk_not_ready(&refused_old, Arc::new(RefusingStore), "refused");

    let data = fresh(&root, "archive-gone", None, models.as_ref());
    let inbox = data.join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let gone = locations(&inbox, Path::new("/Volumes/ncrs-not-mounted/archive"));
    std::fs::write(data.join("config.toml"), gone).unwrap();
    talk_ready(&data, empty_store(), "first start");
    println!("\nshots below {}", root.display());
}

/// The way a user takes from one answer on: the highlighted or else the first
/// answer, until Hermes asks nothing more.
fn press_through<P>(session: &mut Session<P>, data: &Path, prefix: &str, first: (&str, ChipReply))
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    let out = data.join("shots").to_string_lossy().into_owned();
    std::fs::create_dir_all(&out).unwrap();
    let mut label = first.0.to_owned();
    let mut reply = Some(first.1);
    for step in 0..16 {
        let Some(answer) = reply.take() else {
            break;
        };
        println!("    pressing: {label}");
        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: label.clone(),
            reply: answer,
        }));
        pump_for(session, Duration::from_secs(1), |_| false);
        pump_for(session, READ_STEP, |app| !busy(app));
        report(&session.app, &format!("{prefix} {step}"));
        shot(&session.app, &out, &format!("{prefix}-{step:02}"));
        let chips = chat(&session.app).chips();
        if chips.iter().any(|chip| chip.reply == ChipReply::PickFolder) {
            break;
        }
        let usable = || chips.iter().filter(|chip| safe_to_press(&chip.reply, data));
        if let Some(chip) = usable()
            .find(|chip| chip.primary)
            .or_else(|| usable().next())
        {
            label.clone_from(&chip.label);
            reply = Some(chip.reply.clone());
        }
    }
}

/// Until the AI has been through the documents of these folders.
fn wait_for_ai<P>(session: &mut Session<P>, data: &Path, store: &Store, folders: &[&Path])
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    let index = ncrs_index::Index::open(&data.join("index.db"), store.as_ref()).unwrap();
    let started_waiting = Instant::now();
    while started_waiting.elapsed() < READ_STEP {
        pump_for(session, Duration::from_secs(20), |_| false);
        let coverage: Vec<(usize, usize)> = folders
            .iter()
            .map(|folder| index.ai_coverage_beneath(folder).unwrap_or_default())
            .collect();
        let shown: Vec<String> = coverage
            .iter()
            .map(|(with_text, understood)| format!("{understood} of {with_text}"))
            .collect();
        println!(
            "    {:.0} s, understood: {}",
            started_waiting.elapsed().as_secs_f32(),
            shown.join(", ")
        );
        if coverage
            .iter()
            .all(|(with_text, understood)| *with_text > 0 && understood >= with_text)
        {
            return;
        }
    }
}

fn pdf_count(folder: &Path) -> usize {
    let mut found = Vec::new();
    pdfs_below(folder, &mut found, usize::MAX);
    found.len()
}

/// A data folder with an archive and an inbox, every third document going to
/// the inbox. The archive keeps the order of the source or lies flat.
fn documents_for(
    root: &Path,
    name: &str,
    source: &Path,
    models: Option<&PathBuf>,
    archive_in_order: bool,
) -> PathBuf {
    let data = fresh(root, name, None, models);
    std::fs::create_dir_all(data.join("inbox")).unwrap();
    let mut pdfs = Vec::new();
    pdfs_below(source, &mut pdfs, DOCUMENTS);
    for (number, pdf) in pdfs.iter().enumerate() {
        let copy = if number % 3 == 0 {
            data.join("inbox").join(pdf.file_name().unwrap())
        } else if archive_in_order {
            data.join("archive").join(pdf.strip_prefix(source).unwrap())
        } else {
            data.join("archive").join(pdf.file_name().unwrap())
        };
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(pdf, copy).unwrap();
    }
    data
}

fn open_chat<P>(session: &mut Session<P>)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    pump_for(session, Duration::from_secs(10), started);
    session.send(Message::HermesChat(ChatMessage::Open));
    pump_for(session, Duration::from_secs(20), started);
    type_key_back(session);
    report(&session.app, "greeting");
}

fn result_of<P>(session: &Session<P>, data: &Path)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    println!("    whole thread:\n{}", whole_thread(&session.app));
    println!(
        "    pdfs: inbox {}, archive {}",
        pdf_count(&data.join("inbox")),
        pdf_count(&data.join("archive"))
    );
}

#[test]
#[ignore = "tidies copied documents with the real model, inside a data folder of its own"]
fn hermes_real_tidy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("protocol")
        .join("real-start");
    let models = super::env_path("NCRS_AI_MODELS_DIR");
    let source = super::env_path("NCRS_PROTOCOL_ARCHIVE").unwrap();
    read_folders_at_once_and_after_reading(&root, &source, models.as_ref());
    an_archive_without_order(&root, &source, models.as_ref());
    a_folder_the_user_names(&root, &source, models.as_ref());
}

/// Both are read folders; the user does not wait for the AI, then does.
fn read_folders_at_once_and_after_reading(root: &Path, source: &Path, models: Option<&PathBuf>) {
    let data = documents_for(root, "tidy-read-folders", source, models, true);
    let (inbox, archive) = (data.join("inbox"), data.join("archive"));
    std::fs::write(data.join("config.toml"), locations(&inbox, &archive)).unwrap();
    let store = empty_store();
    let mut session = boot(&data, Arc::clone(&store));
    open_chat(&mut session);
    press_through(
        &mut session,
        &data,
        "at-once",
        ("inbox", ChipReply::Folder(inbox.clone())),
    );
    wait_for_ai(&mut session, &data, &store, &[&inbox, &archive]);
    press_through(
        &mut session,
        &data,
        "after-reading",
        ("inbox", ChipReply::Folder(inbox.clone())),
    );
    result_of(&session, &data);
}

/// An archive without any order of its own; the user cancels the folder dialog
/// and tries again once the AI has read everything.
fn an_archive_without_order(root: &Path, source: &Path, models: Option<&PathBuf>) {
    let data = documents_for(root, "tidy-flat-archive", source, models, false);
    let (inbox, archive) = (data.join("inbox"), data.join("archive"));
    std::fs::write(data.join("config.toml"), locations(&inbox, &archive)).unwrap();
    let store = empty_store();
    let mut session = boot(&data, Arc::clone(&store));
    open_chat(&mut session);
    press_through(
        &mut session,
        &data,
        "flat-at-once",
        ("inbox", ChipReply::Folder(inbox.clone())),
    );
    let offers_other_folder = chat(&session.app)
        .chips()
        .iter()
        .any(|chip| chip.reply == ChipReply::TrialOtherTarget);
    println!("    other folder on offer: {offers_other_folder}");
    if offers_other_folder {
        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: "Other folder …".to_owned(),
            reply: ChipReply::TrialOtherTarget,
        }));
        pump_for(&mut session, Duration::from_secs(1), |_| false);
        report(&session.app, "folder dialog cancelled");
    } else {
        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: "Cancel".to_owned(),
            reply: ChipReply::Cancel,
        }));
    }
    wait_for_ai(&mut session, &data, &store, &[&inbox, &archive]);
    press_through(
        &mut session,
        &data,
        "flat-tidy-again",
        ("Tidy up a folder", ChipReply::IntentTidy),
    );
    press_through(
        &mut session,
        &data,
        "flat-after-reading",
        ("inbox", ChipReply::Folder(inbox.clone())),
    );
    result_of(&session, &data);
}

/// The folder to tidy is one the user names, no read folder: the usual case.
fn a_folder_the_user_names(root: &Path, source: &Path, models: Option<&PathBuf>) {
    let data = documents_for(root, "tidy-named-folder", source, models, true);
    let (inbox, archive) = (data.join("inbox"), data.join("archive"));
    let config = format!(
        "{FILING_AI}[[modules.filing.locations]]\npath = {archive:?}\nmode = \"archive\"\ntarget = true\n"
    );
    std::fs::write(data.join("config.toml"), config).unwrap();
    let store = empty_store();
    let mut session = boot(&data, Arc::clone(&store));
    open_chat(&mut session);
    wait_for_ai(&mut session, &data, &store, &[&archive]);
    press_through(
        &mut session,
        &data,
        "named",
        ("inbox", ChipReply::Folder(inbox.clone())),
    );
    result_of(&session, &data);
}

/// Named so CI's GPU-only run skips them with the other sessions: a session
/// needs a renderer, and a runner has no GPU.
mod e2e_tests {
    use std::time::Duration;

    use ncrs_hermes_chat::{ChipReply, Message as ChatMessage};

    use super::{boot, chat, empty_store, is_ready, pump_for, FILING_ONLY};
    #[cfg(feature = "local-ai")]
    use super::{phase_name, FILING_AI};
    use crate::messages::Message;

    #[test]
    fn a_cancelled_folder_dialog_leaves_a_way_on() {
        let scratch = tempfile::tempdir().unwrap();
        let data = scratch.path();
        std::fs::write(data.join("config.toml"), FILING_ONLY).unwrap();
        drop(ncrs_index::Index::create_plain(&data.join("index.db")).unwrap());
        let mut session = boot(data, empty_store());
        pump_for(&mut session, Duration::from_secs(20), is_ready);
        session.send(Message::HermesChat(ChatMessage::Open));

        session.send(Message::HermesChat(ChatMessage::ChipPressed {
            label: "Other folder …".to_owned(),
            reply: ChipReply::PickFolder,
        }));
        pump_for(&mut session, Duration::from_secs(1), |_| false);

        assert!(!chat(&session.app).chips().is_empty());
    }

    /// A missing model used to open its window at the start, and F2 did nothing
    /// behind it. The chat offers the download itself. Only the `local-ai`
    /// build has a built-in model that can be missing; without it there is
    /// nothing to download and no chip to offer.
    #[cfg(feature = "local-ai")]
    #[test]
    fn a_missing_model_leaves_f2_to_the_chat() {
        let scratch = tempfile::tempdir().unwrap();
        let data = scratch.path();
        std::fs::write(data.join("config.toml"), FILING_AI).unwrap();
        drop(ncrs_index::Index::create_plain(&data.join("index.db")).unwrap());
        let mut session = boot(data, empty_store());
        pump_for(&mut session, Duration::from_secs(20), is_ready);
        assert!(is_ready(&session.app), "{}", phase_name(&session.app));

        session.send(Message::HermesChat(ChatMessage::Open));

        assert!(chat(&session.app).open);
        assert!(chat(&session.app)
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::DownloadModel));
        assert!(session.app.ai_model_for_test().is_none());
    }
}
