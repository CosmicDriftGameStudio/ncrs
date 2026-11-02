//! The message loop and the routing of what the user typed or pressed.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_filing::chat_work::{ChatAccess, ChatResult, ChatWork};
use ncrs_i18n::Msg;
use ncrs_intent::{intent_of, normalize_word, reply_of, same_word, FolderRef, Intent, Reply};
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
            let offered = state.chips().to_vec();
            user(state, typed.clone());
            if !is_ready(state, ctx) {
                return None;
            }
            handle_text(state, ctx, &offered, &typed)
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

pub fn resolve_folder(ctx: &Ctx, folder: &FolderRef) -> PathBuf {
    match folder {
        FolderRef::Downloads => ctx.home.join("Downloads"),
        FolderRef::Documents => ctx.home.join("Documents"),
        FolderRef::Desktop => ctx.home.join("Desktop"),
        FolderRef::Path(path) => expand_home(ctx, path),
        FolderRef::Named(name) => ctx
            .targets
            .iter()
            .chain(&ctx.sources)
            .find(|path| {
                path.file_name()
                    .is_some_and(|last| same_word(name, &last.to_string_lossy()))
            })
            .cloned()
            .unwrap_or_else(|| ctx.home.join(name)),
    }
}

fn expand_home(ctx: &Ctx, path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => ctx.home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// A folder given as a word: a well-known or configured one.
fn folder_from_word(state: &State, ctx: &Ctx, typed: &str) -> Option<PathBuf> {
    candidate_folders(ctx, true)
        .into_iter()
        .chain(state.session.learned.iter().cloned())
        .chain(well_known(ctx))
        .find(|path| {
            same_word(typed, &folder_name(ctx, path))
                || path
                    .file_name()
                    .is_some_and(|last| same_word(typed, &last.to_string_lossy()))
        })
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

fn meaning(reply: &ChipReply) -> Option<Reply> {
    match reply {
        ChipReply::ReadAnyway
        | ChipReply::KeepYes
        | ChipReply::ScopeAll
        | ChipReply::FineAsIs
        | ChipReply::ToTrash
        | ChipReply::FileAll
        | ChipReply::PutBack => Some(Reply::Yes),
        ChipReply::LeaveThem | ChipReply::WantChange | ChipReply::Later => Some(Reply::No),
        ChipReply::Cancel => Some(Reply::Cancel),
        _ => None,
    }
}

/// The chip a typed answer stands for.
fn chip_for(chips: &[Chip], typed: &str) -> Option<ChipReply> {
    let find = |wanted: Reply| {
        chips
            .iter()
            .find(|chip| meaning(&chip.reply) == Some(wanted))
    };
    if let Some(reply) = reply_of(typed) {
        let chip = match reply {
            Reply::Yes => find(Reply::Yes),
            Reply::No => find(Reply::No).or_else(|| find(Reply::Cancel)),
            Reply::Cancel => find(Reply::Cancel).or_else(|| find(Reply::No)),
        };
        if let Some(found) = chip {
            return Some(found.reply.clone());
        }
    }
    let wanted = normalize_word(typed);
    chips
        .iter()
        .find(|chip| {
            chip.label.to_lowercase() == typed.to_lowercase()
                || normalize_word(&chip.label) == wanted
        })
        .map(|chip| chip.reply.clone())
}

fn handle_text(state: &mut State, ctx: &Ctx, offered: &[Chip], typed: &str) -> Option<Action> {
    if let Some(reply) = chip_for(offered, typed) {
        return handle_reply(state, ctx, reply);
    }
    if expects_folder(state.step) {
        let looks_like_path = typed.starts_with('/') || typed.starts_with('~');
        if looks_like_path {
            return folder_answer(state, ctx, expand_home(ctx, Path::new(typed)), true);
        }
    }
    match intent_of(typed) {
        Intent::Unknown if expects_folder(state.step) => {
            if let Some(path) = folder_from_word(state, ctx, typed) {
                return folder_answer(state, ctx, path, false);
            }
            fallback(state, ctx);
            None
        }
        intent => handle_intent(state, ctx, intent, typed),
    }
}

fn handle_intent(state: &mut State, ctx: &Ctx, intent: Intent, typed: &str) -> Option<Action> {
    match intent {
        Intent::TidyFolder { folder } => {
            state.step = Step::TidyFolder;
            match folder {
                None => {
                    say(
                        state,
                        text(ctx.lang, Msg::HermesChatAskTidyFolder),
                        ask_chips(ctx),
                    );
                    None
                }
                Some(named) => {
                    let path = resolve_folder(ctx, &named);
                    folder_answer(state, ctx, path, true)
                }
            }
        }
        Intent::LearnStructure { folder } => {
            state.step = Step::ReadFolder;
            state.session.read_for = Some(crate::state::ReadFor::Learn);
            match folder {
                None => {
                    say(
                        state,
                        text(ctx.lang, Msg::HermesChatAskReadFolder),
                        ask_chips(ctx),
                    );
                    None
                }
                Some(named) => {
                    let path = resolve_folder(ctx, &named);
                    folder_answer(state, ctx, path, true)
                }
            }
        }
        Intent::Search { .. } => {
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
            None
        }
        Intent::Explain { .. } => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatExplainNotYet),
                Vec::new(),
            );
            None
        }
        Intent::BatchMove { .. } => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatBatchNotYet),
                Vec::new(),
            );
            None
        }
        Intent::Undo => {
            undo::ask(state, ctx);
            None
        }
        Intent::ChangeOrder { kind_word, unit } => {
            learn::change_order(state, ctx, kind_word, unit);
            None
        }
        Intent::ShellCommand => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatShell),
                vec![chip(
                    ctx,
                    Msg::HermesChatToCommandLine,
                    ChipReply::ToCommandLine(typed.to_owned()),
                    true,
                )],
            );
            None
        }
        Intent::Unknown => {
            fallback(state, ctx);
            None
        }
    }
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
        ChipReply::ToCommandLine(command) => return Some(Action::UseAsCommand(command)),
        ChipReply::IntentTidy => {
            return handle_intent(state, ctx, Intent::TidyFolder { folder: None }, "");
        }
        ChipReply::IntentStructure => {
            return handle_intent(state, ctx, Intent::LearnStructure { folder: None }, "");
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
