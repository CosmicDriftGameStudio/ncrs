//! The harness of the conversation tests: a real index, a real chat access,
//! a folder backed trash, and the work of `update` run on the spot instead of
//! on a thread.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code,
    reason = "test code: a failing assertion is the signal; each test file uses a part of the shared helpers"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Local, TimeZone as _};
use ncrs_ai::{ChatModel, Compute, LocalModelStatus, ScriptedChatModel};
use ncrs_filing::chat_work::{ChatAccess, ChatWork};
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_hermes_chat::{update, Action, Availability, ChipReply, Ctx, Entry, Message, Part, State};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::Index;
use ncrs_intake::test_archive::OWN_NAME;
use ncrs_intake::{Extracted, TextChain, TextError, TextSource};
use ncrs_pipeline::OwnDetails;
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
    /// All work the chat asked for.
    pub works: Vec<ChatWork>,
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
        let root = ncrs_intake::canonical_path(dir.path()).unwrap();
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
            changes_allowed: true,
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
            works: Vec::new(),
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

    /// A document of the archive: the file on disk and its entry in the index
    /// with `text`.
    pub fn index_document(&self, relative: &str, text: &str) -> ncrs_index::DocumentId {
        self.write(relative, text);
        let mut guard = self.index.lock().unwrap();
        let index = guard.as_mut().unwrap();
        let new_document = |hash| ncrs_index::NewDocument {
            hash,
            path: self.path(relative),
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            stamp: None,
        };
        // The hash of the content, so a run can move the file. A copy of a
        // document that is already indexed gets a hash of its path instead.
        let content_hash = ncrs_intake::hash_path(&self.path(relative)).unwrap();
        let path_hash = ncrs_index::ContentHash(Sha256::digest(relative.as_bytes()).into());
        let id = index
            .insert_document(&new_document(content_hash))
            .or_else(|_duplicate| index.insert_document(&new_document(path_hash)))
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
        self.root.join(relative.split('/').collect::<PathBuf>())
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
            self.works.push(work.clone());
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
                .map_err(|error| ncrs_ai::ModelProblem::from(&error)),
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

    /// What the chat handed to the rule suggestions: the folder and the pattern.
    pub fn suggestions(&self) -> Vec<(PathBuf, Option<String>)> {
        self.actions
            .iter()
            .filter_map(|action| match action {
                Action::SuggestRules { folder, pattern } => {
                    Some((folder.clone(), pattern.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The work of the chat only reads or undoes a run; what is left to check is
    /// that it saved no rule.
    pub fn assert_saves_no_rule(&self) {
        assert_eq!(self.rule_count(), 0, "the chat saves no rule");
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
        "{sender}\nMoritzhof 1, 09111 Chemnitz\n\nMarc Beispiel\nMusterweg 1\n00000 Musterstadt\n\n\
         Chemnitz, {date}\n\n{title}\n\nRechnungsnummer: {number}\n\n\
         Mit freundlichen Grüßen\n{sender}\nAmtsgericht Chemnitz HRB 11111\n"
    )
}
