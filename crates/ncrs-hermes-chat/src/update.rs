//! The message loop and the routing of what the user typed or pressed.

use std::path::{Component, Path, PathBuf};

use iced::Task;
use ncrs_filing::chat_work::{same_word, ChatAccess, ChatResult, ChatWork};
use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::{Availability, Ctx};
use crate::message::Message;
use crate::state::{Chip, ChipReply, Entry, Part, State, Step};
use crate::texts::{fill, folder_name, text};
use crate::{learn, tidy, undo};

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let action = route(state, message, ctx);
    (launch(state, ctx), action)
}

fn route(state: &mut State, message: Message, ctx: &Ctx) -> Option<Action> {
    match message {
        Message::Open => {
            state.open = true;
            if state.thread.is_empty() {
                greet(state, ctx);
            }
            None
        }
        Message::Close => {
            state.open = false;
            Some(Action::Close)
        }
        Message::InputChanged(input) => {
            state.input = input;
            None
        }
        Message::Submit => {
            let typed = std::mem::take(&mut state.input).trim().to_owned();
            if typed.is_empty() || state.pending.is_some() {
                return None;
            }
            user(state, typed.clone());
            if !is_ready(state, ctx) {
                return None;
            }
            handle_text(state, ctx, &typed)
        }
        Message::ChipPressed { label, reply } => {
            if state.pending.is_some() {
                return None;
            }
            let bubble = bubble_text(state, &label, &reply);
            user(state, bubble);
            if !is_ready(state, ctx) && reply != ChipReply::Setup {
                return None;
            }
            handle_reply(state, ctx, reply)
        }
        Message::Done(result) => {
            finish_work(state);
            handle_done(state, ctx, *result)
        }
        Message::Failed(reason) => {
            finish_work(state);
            say(
                state,
                text_with(ctx, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            None
        }
        Message::FolderPicked(picked) => {
            let path = picked?;
            if state.pending.is_some() {
                return None;
            }
            user(state, path.to_string_lossy().into_owned());
            folder_answer(state, ctx, path, false)
        }
    }
}

fn text_with(ctx: &Ctx, msg: Msg, values: &[(&str, &str)]) -> String {
    fill(ctx.lang, msg, values)
}

fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh) {
        return Task::none();
    }
    let (Some(work), Availability::Ready(ready)) = (state.pending.clone(), &ctx.availability)
    else {
        return Task::none();
    };
    let access = ready.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || access.run(work)).await },
        |joined| match joined {
            Ok(result) => Message::Done(Box::new(result)),
            Err(error) => Message::Failed(error.to_string()),
        },
    )
}

fn finish_work(state: &mut State) {
    state.pending = None;
    state
        .thread
        .retain(|entry| !matches!(entry, Entry::Working(_)));
}

fn handle_done(state: &mut State, ctx: &Ctx, result: ChatResult) -> Option<Action> {
    match result {
        ChatResult::Read(read) => learn::read_done(state, ctx, read),
        ChatResult::Preview(preview) => learn::preview_done(state, ctx, preview),
        ChatResult::Trial(trial) => tidy::trial_done(state, ctx, trial),
        ChatResult::Filed(filed) => tidy::filed_done(state, ctx, filed),
        ChatResult::Trashed(trashed) => tidy::trashed_done(state, ctx, trashed),
        ChatResult::LastRun(last) => undo::last_run_done(state, ctx, last),
        ChatResult::Undone(undone) => undo::undone_done(state, ctx, undone),
    }
    None
}

// Building the thread.

pub fn access(ctx: &Ctx) -> Option<&ChatAccess> {
    match &ctx.availability {
        Availability::Ready(access) => Some(access),
        _ => None,
    }
}

fn is_ready(state: &mut State, ctx: &Ctx) -> bool {
    match &ctx.availability {
        Availability::Ready(_) => true,
        Availability::Starting => {
            say(state, text(ctx.lang, Msg::HermesChatStarting), Vec::new());
            false
        }
        Availability::NeedsSetup => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatNeedsSetup),
                vec![chip(ctx, Msg::HermesChatSetup, ChipReply::Setup, true)],
            );
            false
        }
    }
}

pub fn chip(ctx: &Ctx, msg: Msg, reply: ChipReply, primary: bool) -> Chip {
    chip_labelled(text(ctx.lang, msg), reply, primary)
}

pub fn chip_labelled(label: String, reply: ChipReply, primary: bool) -> Chip {
    Chip {
        label,
        reply,
        primary,
    }
}

pub fn cancel_chip(ctx: &Ctx) -> Chip {
    chip(ctx, Msg::HermesChatCancel, ChipReply::Cancel, false)
}

/// A bubble of the user clears the chips Hermes offered before it.
pub fn user(state: &mut State, typed: String) {
    for entry in &mut state.thread {
        if let Entry::Hermes { chips, .. } = entry {
            chips.clear();
        }
    }
    state.thread.push(Entry::User(typed));
}

pub fn hermes(state: &mut State, parts: Vec<Part>, chips: Vec<Chip>) {
    state.thread.push(Entry::Hermes { parts, chips });
}

pub fn say(state: &mut State, sentence: String, chips: Vec<Chip>) {
    hermes(state, vec![Part::Text(sentence)], chips);
}

/// Sets work for the app to run. `working` is the line shown meanwhile;
/// `None` for work too quick to show one.
pub fn work(state: &mut State, work: ChatWork, working: Option<String>) {
    if let Some(line) = working {
        state.thread.push(Entry::Working(line));
    }
    state.pending = Some(work);
    state.fresh = true;
}

fn bubble_text(state: &State, label: &str, reply: &ChipReply) -> String {
    if let ChipReply::UnsureChoice { entry, .. } = reply {
        if let Some(name) = state
            .session
            .trial
            .as_ref()
            .and_then(|trial| trial.plan.entries.get(*entry))
            .map(|planned| planned.name.as_str())
        {
            return format!("{name}: {label}");
        }
    }
    label.to_owned()
}

// Folders.

fn well_known(ctx: &Ctx) -> [PathBuf; 4] {
    [
        ctx.home.clone(),
        ctx.home.join("Documents"),
        ctx.home.join("Downloads"),
        ctx.home.join("Desktop"),
    ]
}

fn is_known(state: &State, ctx: &Ctx, path: &Path) -> bool {
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&state.session.learned)
        .chain(&well_known(ctx))
        .any(|known| known == path)
}

/// Targets and sources first, then Documents and Downloads when they exist.
fn candidate_folders(ctx: &Ctx, with_targets: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let own = if with_targets {
        ctx.targets.iter().chain(&ctx.sources)
    } else {
        [].iter().chain(&ctx.sources)
    };
    let usual = [ctx.home.join("Documents"), ctx.home.join("Downloads")]
        .into_iter()
        .filter(|path| path.is_dir());
    for path in own.cloned().chain(usual) {
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

pub fn folder_chips(ctx: &Ctx, folders: Vec<PathBuf>) -> Vec<Chip> {
    folders
        .into_iter()
        .map(|path| chip_labelled(folder_name(ctx, &path), ChipReply::Folder(path), false))
        .collect()
}

/// The folders to offer when a folder is asked for, and the picker.
pub fn ask_chips(ctx: &Ctx) -> Vec<Chip> {
    let mut chips = folder_chips(ctx, candidate_folders(ctx, true));
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

pub fn source_chips(state: &State, ctx: &Ctx) -> Vec<Chip> {
    let _ = state;
    let mut chips = folder_chips(ctx, candidate_folders(ctx, false));
    chips.push(chip(ctx, Msg::HermesChatLater, ChipReply::Later, false));
    chips
}

fn greet(state: &mut State, ctx: &Ctx) {
    if !is_ready(state, ctx) {
        return;
    }
    state.step = Step::ReadFolder;
    state.session.read_for = Some(crate::state::ReadFor::Learn);
    say(
        state,
        text(ctx.lang, Msg::HermesChatGreeting),
        ask_chips(ctx),
    );
}

fn expects_folder(step: Step) -> bool {
    matches!(
        step,
        Step::ReadFolder | Step::Source | Step::TidyFolder | Step::Target
    )
}

fn expand_home(ctx: &Ctx, path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => ctx.home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// A configured, learned or well-known folder the word names.
fn known_folder(state: &State, ctx: &Ctx, word: &str) -> Option<PathBuf> {
    let usual = well_known(ctx);
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&state.session.learned)
        .chain(&usual[1..])
        .find(|path| {
            same_word(word, &folder_name(ctx, path))
                || path
                    .file_name()
                    .is_some_and(|last| same_word(word, &last.to_string_lossy()))
        })
        .cloned()
}

/// The folder a name stands for: a typed path, a known folder, a path below
/// a known folder, or a folder in the home folder. `None` for names that
/// climb out with `..`.
pub fn resolve_folder_name(state: &State, ctx: &Ctx, name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    if name.starts_with('/') || name.starts_with('~') {
        return Some(expand_home(ctx, path));
    }
    if let Some(found) = known_folder(state, ctx, name) {
        return Some(found);
    }
    if let Some((first, rest)) = name.split_once('/') {
        if let Some(found) = known_folder(state, ctx, first) {
            return Some(found.join(rest));
        }
    }
    Some(ctx.home.join(name))
}

pub fn folder_answer(state: &mut State, ctx: &Ctx, path: PathBuf, typed: bool) -> Option<Action> {
    if typed && !is_known(state, ctx, &path) {
        let shown = path.to_string_lossy().into_owned();
        state.session.confirm = Some((path, state.step));
        state.step = Step::ConfirmUnknown;
        say(
            state,
            text_with(ctx, Msg::HermesChatUnknownPathConfirm, &[("path", &shown)]),
            vec![
                chip(ctx, Msg::HermesChatReadAnyway, ChipReply::ReadAnyway, true),
                cancel_chip(ctx),
            ],
        );
        return None;
    }
    match state.step {
        Step::ReadFolder => {
            learn::read(state, ctx, path);
            None
        }
        Step::Source => {
            let role = (!ctx.sources.contains(&path)).then(|| Action::SetFolderRole {
                folder: path.clone(),
                role: FolderRole::Inbox,
            });
            tidy::begin(state, ctx, path);
            role
        }
        Step::TidyFolder => {
            tidy::begin(state, ctx, path);
            None
        }
        Step::Target => {
            tidy::choose_target(state, ctx, path);
            None
        }
        _ => None,
    }
}

// What the user said or pressed.

fn handle_text(state: &mut State, ctx: &Ctx, typed: &str) -> Option<Action> {
    let looks_like_path = typed.starts_with('/') || typed.starts_with('~');
    if expects_folder(state.step) && looks_like_path {
        return folder_answer(state, ctx, expand_home(ctx, Path::new(typed)), true);
    }
    fallback(state, ctx);
    None
}

fn ask_tidy_folder(state: &mut State, ctx: &Ctx, folder: Option<PathBuf>) -> Option<Action> {
    state.step = Step::TidyFolder;
    let Some(path) = folder else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatAskTidyFolder),
            ask_chips(ctx),
        );
        return None;
    };
    folder_answer(state, ctx, path, true)
}

fn ask_read_folder(state: &mut State, ctx: &Ctx, folder: Option<PathBuf>) -> Option<Action> {
    state.step = Step::ReadFolder;
    state.session.read_for = Some(crate::state::ReadFor::Learn);
    let Some(path) = folder else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatAskReadFolder),
            ask_chips(ctx),
        );
        return None;
    };
    folder_answer(state, ctx, path, true)
}

fn search_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatSearchNotYet),
        vec![chip(
            ctx,
            Msg::HermesChatOpenSearch,
            ChipReply::OpenSearch,
            true,
        )],
    );
}

fn explain_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatExplainNotYet),
        Vec::new(),
    );
}

fn batch_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatBatchNotYet),
        Vec::new(),
    );
}

fn fallback(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatFallback),
        vec![
            chip(
                ctx,
                Msg::HermesChatIntentSearch,
                ChipReply::IntentSearch,
                false,
            ),
            chip(ctx, Msg::HermesChatIntentTidy, ChipReply::IntentTidy, false),
            chip(
                ctx,
                Msg::HermesChatIntentStructure,
                ChipReply::IntentStructure,
                false,
            ),
            chip(ctx, Msg::HermesChatIntentUndo, ChipReply::IntentUndo, false),
        ],
    );
}

fn handle_reply(state: &mut State, ctx: &Ctx, reply: ChipReply) -> Option<Action> {
    match reply {
        ChipReply::Cancel => cancel(state, ctx),
        ChipReply::Folder(path) => return folder_answer(state, ctx, path, false),
        ChipReply::PickFolder => return Some(Action::PickFolder),
        ChipReply::Setup => return Some(Action::OpenFilingSetup),
        ChipReply::ReadAnyway => {
            if let Some((path, step)) = state.session.confirm.take() {
                state.step = step;
                return folder_answer(state, ctx, path, false);
            }
        }
        ChipReply::KeepYes | ChipReply::FineAsIs => return learn::keep(state, ctx),
        ChipReply::WantChange => learn::want_change(state, ctx),
        ChipReply::ShowAll => learn::show_all(state, ctx),
        ChipReply::ScopeAll => learn::scope(state, ctx, None),
        ChipReply::ScopeOnly(value) => learn::scope(state, ctx, Some(value)),
        ChipReply::ToTrash => tidy::trash_offer(state, ctx, true),
        ChipReply::LeaveThem => {
            tidy::trash_offer(state, ctx, false);
            if state.step == Step::Duplicates {
                learn::after_duplicates(state, ctx);
            }
        }
        ChipReply::Later => {
            say(state, text(ctx.lang, Msg::HermesChatLaterReply), Vec::new());
            state.step = Step::Idle;
        }
        ChipReply::Once => tidy::once(state, ctx, false),
        ChipReply::Always => tidy::once(state, ctx, true),
        ChipReply::ModeCopy => tidy::mode(state, ctx, ncrs_index::Transfer::Copy),
        ChipReply::ModeMove => tidy::mode(state, ctx, ncrs_index::Transfer::Move),
        ChipReply::FileAll => tidy::file_all(state, ctx),
        ChipReply::ShowMore => tidy::show_more(state, ctx),
        ChipReply::UnsureChoice { entry, choice } => tidy::unsure(state, ctx, entry, choice),
        ChipReply::PutBack => undo::put_back(state, ctx),
        ChipReply::Undo | ChipReply::IntentUndo => undo::ask(state, ctx),
        ChipReply::OpenSearch | ChipReply::IntentSearch => return Some(Action::OpenSearch),
        ChipReply::IntentTidy => {
            return ask_tidy_folder(state, ctx, None);
        }
        ChipReply::IntentStructure => {
            return ask_read_folder(state, ctx, None);
        }
    }
    None
}

pub fn cancel(state: &mut State, ctx: &Ctx) {
    state.session.tidy = None;
    state.session.trial = None;
    state.session.pending_change = None;
    state.session.offer = None;
    state.session.resume_tidy = false;
    state.session.confirm = None;
    state.step = Step::Idle;
    say(state, text(ctx.lang, Msg::HermesChatCancelled), Vec::new());
}
