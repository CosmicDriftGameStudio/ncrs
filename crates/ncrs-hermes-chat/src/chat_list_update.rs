//! What the list of chats asks of the conversation: saving it as text, its
//! status for the list, and the messages the list sends.

use std::path::PathBuf;

use chrono::{DateTime, Local};

use crate::action::Action;
use crate::chat_list::ChatList;
use crate::chat_store::{ChatId, ChatStatus, Line, Speaker};
use crate::ctx::Ctx;
use crate::state::{Entry, Part, State, Step};
use crate::update::{is_busy, restart_conversation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListMessage {
    Show(ChatId),
    /// Cmd+Up: the chat above the shown one.
    Previous,
    /// Cmd+Down: the chat below the shown one.
    Next,
    Search(String),
    /// Space: marks or unmarks the shown chat, when nothing is typed.
    KeyMark,
    OpenMenu(ChatId),
    Rename(ChatId),
    RenameInput(String),
    RenameSubmit,
    /// The footer button: the marked chats.
    DeleteMarked,
    /// Del: the marked chats, or the shown one; only when nothing is typed.
    KeyDelete,
    DeleteOne(ChatId),
    DeleteDone,
    ConfirmDelete,
    KeepChats,
}

fn part_text(part: &Part) -> String {
    match part {
        Part::Text(text) => text.clone(),
        Part::Levels(card) => card.title.clone(),
        Part::Areas(card) => card
            .rows
            .iter()
            .map(|row| format!("{}: {}", row.name, row.description))
            .collect::<Vec<_>>()
            .join("\n"),
        Part::Folder(mention) => format!("{} ({})", mention.name, mention.path),
        Part::Space(card) => card.title.clone(),
    }
}

/// The conversation as the text a closed chat shows again: buttons, cards
/// and the working line are left out.
pub fn transcript(thread: &[Entry]) -> Vec<Line> {
    thread
        .iter()
        .filter_map(|entry| match entry {
            Entry::User(text) => Some(Line {
                speaker: Speaker::User,
                text: text.clone(),
            }),
            Entry::UserFolder(mention) => Some(Line {
                speaker: Speaker::User,
                text: format!("{} ({})", mention.name, mention.path),
            }),
            Entry::Hermes { parts, .. } => {
                let text = parts
                    .iter()
                    .map(part_text)
                    .filter(|piece| !piece.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.is_empty()).then_some(Line {
                    speaker: Speaker::Hermes,
                    text,
                })
            }
            Entry::Working(_) => None,
        })
        .collect()
}

/// Where the conversation stands: levels that wait for a yes, a finished job,
/// or a question.
pub fn status_of(state: &State) -> ChatStatus {
    if state.session.rule.is_some() {
        ChatStatus::PlanOpen
    } else if state.step == Step::Idle && state.thread.iter().any(|e| matches!(e, Entry::User(_))) {
        ChatStatus::Done
    } else {
        ChatStatus::Question
    }
}

impl State {
    /// Saves the chat where a restart finds it again: `dir` is the folder of
    /// the chat files; the chats found there are listed.
    pub fn use_chat_store(&mut self, dir: PathBuf, now: DateTime<Local>) {
        self.chats.attach(dir, now.timestamp());
    }

    /// After how many days an unused chat is deleted; `None` never.
    pub fn set_chat_keep_days(&mut self, days: Option<u32>) {
        self.chats.set_keep_days(days);
    }

    pub fn chat_keep_days(&self) -> Option<u32> {
        self.chats.keep_days()
    }

    pub(crate) fn sync_chats(&mut self, now: DateTime<Local>) {
        let lines = transcript(&self.thread);
        let status = status_of(self);
        self.chats.sync_live(lines, status, now.timestamp());
    }

    pub fn chats(&self) -> &ChatList {
        &self.chats
    }
}

fn delete(state: &mut State, ctx: &Ctx, ids: Vec<ChatId>) {
    let busy = is_busy(state);
    let live = state.chats.live().cloned();
    let kept = ids
        .into_iter()
        .filter(|id| !(busy && Some(id) == live.as_ref()))
        .collect();
    state.chats.request_delete(kept);
    after_delete(state, ctx, live.is_some());
}

/// The running conversation was among the deleted: a new one takes its place.
fn after_delete(state: &mut State, ctx: &Ctx, had_live: bool) {
    if had_live && state.chats.live().is_none() {
        restart_conversation(state, ctx);
    }
}

pub fn handle(state: &mut State, message: ListMessage, ctx: &Ctx) -> Option<Action> {
    let typing = !state.input.is_empty() || state.chats.renaming().is_some();
    match message {
        ListMessage::Show(id) => {
            state.chats.dismiss_open_thing();
            state.chats.show(&id);
        }
        ListMessage::Previous => state.chats.step(-1),
        ListMessage::Next => state.chats.step(1),
        ListMessage::Search(text) => state.chats.set_search(text),
        ListMessage::KeyMark => {
            if !typing {
                state.chats.toggle_mark_shown();
            }
        }
        ListMessage::OpenMenu(id) => state.chats.toggle_menu(id),
        ListMessage::Rename(id) => state.chats.start_rename(&id),
        ListMessage::RenameInput(draft) => state.chats.rename_input(draft),
        ListMessage::RenameSubmit => state.chats.finish_rename(),
        ListMessage::DeleteMarked => {
            let ids = state.chats.delete_targets();
            delete(state, ctx, ids);
        }
        ListMessage::KeyDelete => {
            if !typing && state.chats.asking().is_none() {
                let ids = state.chats.delete_targets();
                delete(state, ctx, ids);
            }
        }
        ListMessage::DeleteOne(id) => delete(state, ctx, vec![id]),
        ListMessage::DeleteDone => {
            let ids = state.chats.done_chats();
            delete(state, ctx, ids);
        }
        ListMessage::ConfirmDelete => {
            let had_live = state.chats.live().is_some();
            state.chats.confirm_delete();
            after_delete(state, ctx, had_live);
        }
        ListMessage::KeepChats => state.chats.cancel_delete(),
    }
    None
}
