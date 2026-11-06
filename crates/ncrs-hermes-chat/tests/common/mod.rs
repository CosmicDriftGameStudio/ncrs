//! The harness of the conversation tests: a real index, a real chat access,
//! a folder backed trash, and the work of `update` run on the spot instead of
//! on a thread.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Local, TimeZone as _};
use ncrs_ai::{ChatModel, Compute, LocalModelStatus, ScriptedChatModel};
use ncrs_filing::chat_work::{ChatAccess, TrialPlan};
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_hermes_chat::{update, Action, Availability, ChipReply, Ctx, Entry, Message, Part, State};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{BatchKind, Index};
use ncrs_intake::test_archive::OWN_NAME;
use ncrs_intake::{Extracted, TextChain, TextError, TextSource};
use ncrs_pipeline::OwnDetails;
use ncrs_ui_kit::format;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

mod resolve;
pub use resolve::resolve_folder;

pub const LANG: Language = Language::German;

/// The test archive keeps its text in files named like PDFs.
#[derive(Debug)]
struct Utf8Text;

impl TextSource for Utf8Text {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        let bytes = fs::read(path).map_err(|err| TextError::Unreadable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => Extracted::Text(text),
            Err(_) => Extracted::NotApplicable,
        })
    }
}

pub struct Talk {
    pub dir: TempDir,
    /// The canonical path of `dir`.
    pub root: PathBuf,
    pub access: ChatAccess,
    pub ctx: Ctx,
    pub state: State,
    pub actions: Vec<Action>,
    /// Entries of the user: chips pressed and answers typed.
    pub entries: usize,
    pub index: Arc<Mutex<Option<Index>>>,
    /// The scripted model behind `ctx.model`.
    pub model: Arc<ScriptedChatModel>,
    /// A real model that answers instead of the scripted one.
    pub real_model: Option<Arc<dyn ChatModel>>,
    /// The line shown while the model worked on the last typed sentence.
    pub working: Option<String>,
}

impl Talk {
    /// `targets` and `sources` are folders below the temp dir.
    pub fn in_dir(dir: TempDir, targets: &[&str], sources: &[&str]) -> Self {
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let slot = Arc::new(Mutex::new(Some(index)));
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let trash = JournaledTrash::new(dir.path().join("trash.toml"), FolderBackend(bin));
        let access = ChatAccess::new(
            Arc::clone(&slot),
            Arc::new(TextChain::new().with_source(Utf8Text)),
            Arc::new(trash),
        )
        .with_own(OwnDetails {
            names: vec![OWN_NAME.to_owned()],
            ..OwnDetails::default()
        });
        let model = Arc::new(ScriptedChatModel::new(
            LocalModelStatus::Loaded {
                name: "test".to_owned(),
                memory_bytes: 0,
                compute: Compute::Cpu,
            },
            [],
        ));
        // Resolved folders are canonical, so the configured ones must be too.
        let root = fs::canonicalize(dir.path()).unwrap();
        let below = |names: &[&str]| names.iter().map(|name| root.join(name)).collect();
        let ctx = Ctx {
            lang: LANG,
            availability: Availability::Ready(Box::new(access.clone())),
            home: root.join("home"),
            targets: below(targets),
            favourites: Vec::new(),
            sources: below(sources),
            // Long after any run of the test, so a run is never "today".
            now: Local.with_ymd_and_hms(2031, 1, 1, 12, 0, 0).unwrap(),
            ai_on: true,
            ocr_models_present: true,
            model: Some(Arc::clone(&model) as Arc<dyn ChatModel>),
            resolve_folder,
        };
        Self {
            dir,
            root,
            access,
            ctx,
            state: State::default(),
            actions: Vec::new(),
            entries: 0,
            index: slot,
            model,
            real_model: None,
            working: None,
        }
    }

    /// The sentences go to `model` instead of the scripted one.
    pub fn with_real_model(mut self, model: Arc<dyn ChatModel>) -> Self {
        self.ctx.model = Some(Arc::clone(&model));
        self.real_model = Some(model);
        self
    }

    fn model_answering(&self) -> &dyn ChatModel {
        self.real_model
            .as_deref()
            .unwrap_or(self.model.as_ref() as &dyn ChatModel)
    }

    /// The chat can count, show and file the documents that wait; they are
    /// filed below `archive`.
    pub fn with_review(mut self) -> Self {
        let review = ncrs_filing::review_work::ReviewAccess::new(
            Arc::clone(&self.index),
            self.path("archive"),
        );
        self.access = self.access.with_review(Some(review));
        self.ctx.availability = Availability::Ready(Box::new(self.access.clone()));
        self
    }

    /// The chat can try and save rules; its locations are the archive and the inbox.
    pub fn with_rules(mut self) -> Self {
        let archive = ncrs_intake::Location::new(
            &self.path("archive"),
            ncrs_intake::LocationMode::Archive,
            false,
        )
        .unwrap();
        std::fs::create_dir_all(self.path("inbox")).unwrap();
        let inbox = ncrs_intake::Location::new(
            &self.path("inbox"),
            ncrs_intake::LocationMode::Inbox,
            false,
        )
        .unwrap();
        let rules = ncrs_filing::rules_work::RulesAccess::new(
            Arc::clone(&self.index),
            vec![archive, inbox],
        );
        self.access = self.access.with_rules(Some(rules));
        self.ctx.availability = Availability::Ready(Box::new(self.access.clone()));
        self
    }

    /// A document of the archive the rules trial sees: the file on disk and
    /// its entry in the index with `text`.
    pub fn index_document(&self, relative: &str, text: &str) -> ncrs_index::DocumentId {
        self.index_with_source(relative, text, ncrs_index::DocumentSource::Archive)
    }

    /// A document that waits in the inbox, known to the index.
    pub fn index_inbox_document(&self, relative: &str, text: &str) -> ncrs_index::DocumentId {
        self.index_with_source(relative, text, ncrs_index::DocumentSource::Inbox)
    }

    fn index_with_source(
        &self,
        relative: &str,
        text: &str,
        source: ncrs_index::DocumentSource,
    ) -> ncrs_index::DocumentId {
        self.write(relative, text);
        let hash: [u8; 32] = Sha256::digest(relative.as_bytes()).into();
        let mut guard = self.index.lock().unwrap();
        let index = guard.as_mut().unwrap();
        let id = index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash(hash),
                path: self.path(relative),
                source,
                received_at: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: None,
            })
            .unwrap();
        index.set_document_text(id, text).unwrap();
        id
    }

    /// The fields the index holds of a document, as if the AI had read them.
    pub fn set_fields(&self, id: ncrs_index::DocumentId, fields: &ncrs_index::DocumentFields) {
        let mut guard = self.index.lock().unwrap();
        let index = guard.as_mut().unwrap();
        index
            .set_document_fields(id, fields, |_| ncrs_index::FieldOrigin::Manual)
            .unwrap();
    }

    /// How many rules the index holds.
    pub fn rule_count(&self) -> usize {
        self.rules().len()
    }

    pub fn rules(&self) -> Vec<ncrs_index::Rule> {
        self.index
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .rules()
            .unwrap()
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn write(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// One message, then the work it asks for, run here instead of on a thread.
    pub fn send(&mut self, first: Message) {
        let next = self.step(first);
        self.settle(next);
    }

    fn step(&mut self, message: Message) -> Option<Message> {
        let (_task, action) = update(&mut self.state, message, &self.ctx);
        self.actions.extend(action);
        if let Some(work) = self.state.pending_work().cloned() {
            return Some(Message::Done(Box::new(self.access.run(work))));
        }
        if let Some(naming) = self.state.pending_naming() {
            return Some(naming.resolve(self.ctx.resolve_folder));
        }
        if let Some((typed, step)) = self.state.pending_resolve() {
            let result = (self.ctx.resolve_folder)(typed);
            return Some(Message::TypedFolderResolved {
                typed: typed.to_path_buf(),
                step,
                result,
            });
        }
        if let Some(ask) = self.state.pending_rule_ask().cloned() {
            return Some(Message::RuleAnswered(ask.run(self.model_answering())));
        }
        let turn = self.state.pending_turn().cloned()?;
        Some(Message::Understood(
            self.model_answering()
                .understand(&turn)
                .map_err(|error| error.to_string()),
        ))
    }

    fn settle(&mut self, mut next: Option<Message>) {
        while let Some(message) = next.take() {
            next = self.step(message);
        }
    }

    pub fn press(&mut self, reply: &ChipReply) {
        let chip = self
            .state
            .chips()
            .iter()
            .find(|chip| &chip.reply == reply)
            .unwrap_or_else(|| panic!("no chip {reply:?} in {:?}", self.state.chips()))
            .clone();
        self.entries += 1;
        self.send(Message::ChipPressed {
            label: chip.label,
            reply: chip.reply,
        });
    }

    /// Leaves every question about the staying files as it is, up to the
    /// offer to file. These presses are not counted in `entries`.
    pub fn leave_stay_questions(&mut self) {
        let leaving = [
            ChipReply::GroupLeave,
            ChipReply::SinglesLeave,
            ChipReply::NamesLeave,
        ];
        let counted = self.entries;
        while let Some(reply) = leaving
            .iter()
            .find(|reply| self.state.chips().iter().any(|chip| &chip.reply == *reply))
        {
            self.press(&reply.clone());
        }
        self.entries = counted;
    }

    pub fn type_text(&mut self, typed: &str) {
        self.entries += 1;
        self.send(Message::InputChanged(typed.to_owned()));
        self.send(Message::Submit);
    }

    /// A sentence the model understands as `reply`; the model line shows
    /// while it works.
    pub fn type_text_understood(&mut self, typed: &str, reply: &str) {
        self.model.push_reply(Ok(reply));
        self.entries += 1;
        self.send(Message::InputChanged(typed.to_owned()));
        let next = self.step(Message::Submit);
        let Some(Entry::Working(line)) = self.state.thread().last() else {
            panic!("{:?}", self.state.thread().last());
        };
        self.working = Some(line.clone());
        self.settle(next);
    }

    /// Answers "Where should they go?" with the archive, when it is asked.
    pub fn choose_archive_if_asked(&mut self) {
        let archive = ChipReply::Folder(self.path("archive"));
        if self.state.chips().iter().any(|chip| chip.reply == archive) {
            self.press(&archive);
        }
    }

    pub fn open(&mut self) {
        self.send(Message::Open);
    }

    pub fn last_parts(&self) -> &[Part] {
        match self.state.thread().last() {
            Some(Entry::Hermes { parts, .. }) => parts,
            other => panic!("{other:?}"),
        }
    }

    /// The text parts of the newest Hermes entry.
    pub fn last_texts(&self) -> Vec<String> {
        self.last_parts()
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// "{date} {time}" of the last undoable run, as the undo question says it.
    pub fn last_run_when(&self) -> String {
        let guard = self.index.lock().unwrap();
        let index = guard.as_ref().unwrap();
        let batch = index
            .last_undoable_batch(BatchKind::Hermes)
            .unwrap()
            .unwrap();
        let info = index.batch_info(batch).unwrap().unwrap();
        let seconds = u64::try_from(info.finished_at.unwrap_or(info.started_at)).unwrap();
        let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds);
        let formatted = format::time(Some(at));
        let (date, time) = formatted.split_once(' ').unwrap();
        say(Msg::HermesChatWhenDate, &[("date", date), ("time", time)])
    }

    pub fn plan(&self) -> &TrialPlan {
        self.state.trial_plan().unwrap()
    }

    pub fn labels(&self) -> Vec<String> {
        self.state
            .chips()
            .iter()
            .map(|chip| chip.label.clone())
            .collect()
    }

    /// Every file below the temp dir, bin and index aside, with its hash.
    pub fn files(&self) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        for entry in fs::read_dir(self.dir.path()).unwrap() {
            let folder = entry.unwrap().path();
            let name = folder.file_name().unwrap().to_string_lossy().into_owned();
            if folder.is_dir() && name != "bin" {
                for (path, hash) in listing(&folder) {
                    found.insert(format!("{name}/{path}"), hash);
                }
            }
        }
        found
    }
}

pub fn say(msg: Msg, values: &[(&str, &str)]) -> String {
    let named: Vec<(String, &str)> = values
        .iter()
        .map(|(name, value)| (format!("{{{name}}}"), *value))
        .collect();
    let borrowed: Vec<(&str, &str)> = named.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    fill_template(LANG.text(msg), &borrowed)
}

pub fn count(value: usize) -> String {
    value.to_string()
}

pub fn listing(root: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, folder: &Path, found: &mut BTreeMap<String, String>) {
        let mut entries: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(base, &path, found);
            } else {
                let digest = Sha256::digest(fs::read(&path).unwrap());
                let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
                let relative = path.strip_prefix(base).unwrap();
                let slashed: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.insert(slashed.join("/"), hex);
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

/// A business letter like the ones of the test archive.
pub fn letter(sender: &str, date: &str, title: &str, number: u32) -> String {
    format!(
        "{sender}\nMoritzhof 1, 09111 Chemnitz\n\nMarc Beispiel\nAndrestr. 12\n09112 Chemnitz\n\n\
         Chemnitz, {date}\n\n{title}\n\nRechnungsnummer: {number}\n\n\
         Mit freundlichen Grüßen\n{sender}\nAmtsgericht Chemnitz HRB 11111\n"
    )
}
